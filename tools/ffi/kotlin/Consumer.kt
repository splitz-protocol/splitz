/// A Kotlin wallet over the generated binding.
///
/// It holds its own log, keeps its own clock and its own randomness, and hands
/// the library the facts it owns. Nothing calls back into Kotlin: SPEC.md §15
/// names seven interfaces a wallet implements, and across a foreign boundary
/// those would be seven sets of callbacks.
import uniffi.splitz_ffi.*

var failures = 0

fun check(name: String, ok: Boolean, saw: String) {
    if (ok) println("  PASS  $name — $saw")
    else { failures += 1; println("  FAIL  $name — $saw") }
}

/// One device: its log, its clock, its randomness, and the key it signs with.
class Device(private val seedByte: Int) {
    var entries: List<String> = emptyList()
    private var minute = 0

    /// A §9.3 instant: UTC, exactly three fractional digits, fixed width, so a
    /// log sorts as text on every device (§10.2).
    fun now(): String {
        minute += 1
        val total = 19 * 60 + 30 + minute
        return "2026-10-28T%02d:%02d:00.000Z".format(total / 60, total % 60)
    }

    /// §9.4 derives a bill's id from this. A shipped wallet uses the
    /// platform's own entropy: two bills opened in one second by one person
    /// are one bill if it can be guessed.
    fun nonce(): ByteArray = ByteArray(16) { (seedByte + minute + it).toByte() }

    /// The Ed25519 seed this account signs with, as §9.4 writes a key. A
    /// shipped wallet keeps this in the platform keychain.
    val seed: String = java.util.Base64.getUrlEncoder().withoutPadding()
        .encodeToString(ByteArray(32) { (seedByte + it).toByte() })

    /// The key this account publishes, and the participant id it derives
    /// (§10.7): a wallet that publishes a key writes every entry under the id
    /// that key derives, or the key binds nothing.
    val key: String = identityKeyFromSeed(seed)
    val me: String = participantIdForKey(key)

    fun facts() = HostFacts(me, now(), nonce())

    fun add(entry: String) {
        entries = mergeEntries(entries, listOf(entry)).entries
    }

    fun take(other: Device) {
        entries = mergeEntries(entries, other.entries).entries
    }
}

/// What `block` raised as a host refusal, or null when it raised nothing.
fun refusal(block: () -> Unit): SplitzException.Host? =
    try { block(); null } catch (e: SplitzException.Host) { e }

/// A signed entry's own id: the one beside its signature, not its payload's.
fun entryId(entry: String): String =
    Regex("\"id\":\"([^\"]+)\",\"sig\":").find(entry)!!.groupValues[1]

/// `args` are a running relay's origin and an origin nothing answers.
fun main(args: Array<String>) {
    val (origin, downOrigin) = args
    val ana = Device(1)
    val ben = Device(90)
    val anaKey = ana.key
    val benKey = ben.key

    println("a wallet passes facts, not callbacks")
    check("an identity key is 43 unpadded base64url characters",
          anaKey.length == 43, anaKey)
    val secret = "mnemonic words".toByteArray()
    check("a seed derived from a spending secret survives a reinstall",
          identitySeedFromSecret(SecretBytes(secret)) == identitySeedFromSecret(SecretBytes(secret)),
          identitySeedFromSecret(SecretBytes(secret)))
    check("and is the one the protocol pins",
          identitySeedFromSecret(SecretBytes(byteArrayOf(1, 2, 3))) ==
              "MNp3HJmtVUpkGFp2KXoi4ysYoDqKi9Sf4upQw5qvOps",
          identitySeedFromSecret(SecretBytes(byteArrayOf(1, 2, 3))))
    check("and a long secret whose first byte is high crosses whole",
          identitySeedFromSecret(SecretBytes(ByteArray(64) { 0xAB.toByte() })) ==
              "zHlJI6Xb7tQXjLuwAoEwVjjVEB8jPs5DA_NhM50W1YM",
          "zHlJ…")
    check("a key of the wrong length is named, not accepted",
          billKeyProblem("AAAA") == "wrong_length" && billKeyProblem(anaKey) == null,
          "${billKeyProblem("AAAA")}")

    println("ana opens a bill and joins it")
    // The bill key is the wallet's to keep, minted from the platform's own
    // entropy; §9.4's id is public.
    val billKey = newBillKey(RandomBytes(ByteArray(32).also { java.security.SecureRandom().nextBytes(it) }))
    check("that key is one the cipher can use", billKeyProblem(billKey) == null, billKey)
    // The key is minted first: the create entry commits to it (§9.4).
    val create = createBillEntry(ana.facts(), "Dinner", "EUR", "equal", anaKey, billKey, ana.seed)
    ana.add(create)
    val billId = Regex("\"id\":\"([^\"]+)\"").find(create)!!.groupValues[1]
    ana.add(joinBillEntry(ana.facts(), billId, "Ana", "u1ana", anaKey, listOf(), ana.seed))

    println("ana shares it, and ben takes it from the code")
    val invite = inviteForBill(ana.facts(), billId, ana.entries, billKey, "Dinner", 1_800_000_000L)
    val link = renderInviteLink(invite, "https://example.org/join")
    val stranger = newBillKey(RandomBytes(ByteArray(32).also { java.security.SecureRandom().nextBytes(it) }))
    check("a bill code carrying some other key is refused",
          readScanned(shareableBillPayload(ana.facts(), billId, ana.entries, stranger)!!).refusedCode
              == "invite_key_mismatch",
          "invite_key_mismatch")
    check("the invite reads back from an https link", readScanned(link).billId == billId, link)
    check("and expires by the wallet's clock",
          !inviteExpiry(invite, 1_799_999_999L).expired && inviteExpiry(invite, 1_800_000_001L).expired,
          "1800000000")
    val payload = shareableBillPayload(ana.facts(), billId, ana.entries, billKey)
    check("the whole bill fits in one code", payload != null, "${payload?.length} characters")
    val scanned = readScanned(payload!!)
    check("the scan names the same bill", scanned.billId == billId, "${scanned.billId}")
    check("and carries the key", scanned.billKey == billKey, "${scanned.billKey}")
    ben.entries = mergeEntries(ben.entries, scanned.entries).entries
    ben.add(joinBillEntry(ben.facts(), billId, "Ben", "u1ben", benKey, listOf(), ben.seed))
    // What ana holds, as she would report it: one key per copy (§14.5).
    val anaHolds = ana.entries.map { copyKey(it) }
    val behind = deltaForPeer(ben.facts(), billId, ben.entries, anaHolds)
    check("ana lacks only ben's join, and it fits one code",
          behind.missing == 1uL && behind.uri != null && behind.tooBigCode == null, "$behind")

    println("the two logs move through a relay that holds only ciphertext")
    val channel = channelForBill(billId)
    check("the channel is the bill id's hash, never the id",
          channel != billId && channel.length == 64, channel.take(16) + "…")
    // Two clients, as two devices hold them: what one pushes the other fetches.
    val benRelay = SplitzRelay(origin)
    val anaRelay = SplitzRelay(origin)
    // Pushed as they are held: each was signed when it was written.
    val pushed = blobsToPush(ben.entries, billKey)
    benRelay.push(channel, pushed)
    benRelay.push(channel, pushed)
    val fetched = anaRelay.fetch(channel)
    check("another client fetches every blob pushed, once, though it was pushed twice",
          fetched.sorted() == pushed.sorted(), "${fetched.size} of ${pushed.size}")
    val opened = openBlobs(fetched, billId, billKey)
    check("every blob opened", opened.unopenable == 0u, "unopenable=${opened.unopenable}")
    // Merged by entry id: what a blob opens to is the entry, not necessarily
    // the same text, so a round trip adds no entry to the log it came from.
    val roundTrip = mergeEntries(ben.entries, opened.entries)
    check("to the entries ben holds",
          opened.entries.size == ben.entries.size &&
              roundTrip.entries.size == ben.entries.size && roundTrip.refused.isEmpty(),
          "${opened.entries.size} opened, ${roundTrip.entries.size} after merging into ${ben.entries.size}")
    ana.entries = mergeEntries(ana.entries, opened.entries).entries

    println("a relay that fails says whether retrying could succeed")
    val down = refusal { SplitzRelay(downOrigin).fetch(channel) }
    check("a relay that is down raises, transient", down?.transient == true, "${down?.detail}")
    val notChannel = refusal { anaRelay.push("not-a-channel", listOf("x")) }
    check("a 4xx the relay answers raises, transient, as every client classifies it",
          notChannel?.transient == true && notChannel.detail.contains("refused"),
          "${notChannel?.detail}")
    val oversize = refusal { anaRelay.push(channel, listOf("x".repeat(64 * 1024 + 1))) }
    check("a blob over the cap raises before it is sent, not transient",
          oversize?.transient == false, "${oversize?.detail}")
    val queried = refusal { SplitzRelay("$origin?t=1") }
    check("an origin carrying a query raises, not transient",
          queried?.transient == false, "${queried?.detail}")

    println("ana adds an expense they share, and prices it")
    val dinner = addExpenseEntry(ana.facts(), billId, "x1", ana.me, 9000,
        """{"type":"equal","among":["${ana.me}","${ben.me}"]}""", "dinner", ana.seed)
    ana.add(dinner)
    ana.add(setRateEntry(ana.facts(), billId, "EUR", 300000, "a fixed feed", ana.seed))

    val folded = foldEntries(ana.facts(), billId, ana.entries)
    check("both people are on the bill", folded.bill.participants.size == 2,
          folded.bill.participants.joinToString { it.id })
    check("nothing was set aside", folded.setAside.isEmpty(), "${folded.setAside}")
    check("both keys are bound under §10.7",
          folded.identities.bound == mapOf(ana.me to anaKey, ben.me to benKey),
          "${folded.identities.bound.keys}")

    println("ana plans taking ben off the bill (§10.8)")
    val dinnerId = entryId(dinner)
    val unpaid = planRemoval(ana.facts(), billId, ana.entries, ben.me, ana.me)
    check("her expense is offered, split without him, and nothing blocks it",
          unpaid.blockers.isEmpty() && unpaid.edits.map { it.entryId } == listOf(dinnerId) &&
              unpaid.edits.single().splitJson == """{"type":"equal","among":["${ana.me}"]}""",
          "${unpaid.edits.map { it.splitJson }}")
    check("and the plan still stands while the bill has not moved",
          sameRemovalPlan(unpaid, planRemoval(ana.facts(), billId, ana.entries, ben.me, ana.me)) ==
              RemovalPlanStanding.STANDS,
          "same")
    check("a split without him crosses as JSON",
          splitWithout("""{"type":"equal","among":["${ana.me}","${ben.me}"]}""", ben.me) ==
              """{"type":"equal","among":["${ana.me}"]}""",
          "${splitWithout("""{"type":"equal","among":["${ana.me}","${ben.me}"]}""", ben.me)}")
    check("and one only a person can redivide is answered with none",
          splitWithout("""{"type":"exact","amounts":{"${ana.me}":1,"${ben.me}":1}}""", ben.me) == null,
          "none")
    val notSplit = refusal { splitWithout("{", ben.me) }
    check("text that is not a split is refused", notSplit != null, "${notSplit?.detail}")

    println("ben owes half of it")
    ben.take(ana)
    val owed = obligationOf(ben.facts(), billId, ben.entries)
    check("ben has an obligation", owed != null, owed?.request?.uri ?: "none")
    val settlement = owed!!.settlements.single()
    check("it is four and a half thousand to ana",
          settlement.to == ana.me && settlement.amount == 4500L,
          "${settlement.to} ${settlement.amount}")
    check("the request is a ZIP 321 URI naming ana's address",
          owed.request.uri!!.startsWith("zcash:u1ana"), owed.request.uri!!)
    check("nothing is withheld", owed.request.withheldMinorUnits == 0L,
          "${owed.request.withheldMinorUnits}")

    println("ben's review screen shows what §14.2 says it must")
    // The screen is the wallet's; these are the strings it draws, with the
    // amount and the rate written as the binding writes them.
    val zec = renderAmount(owed.request.payments.single().zatoshi)
    val screen = listOf("Pay Ana $zec ZEC", "to u1ana",
                        "at ${rateFigure(owed.rate)} EUR per ZEC, set by Ana")
    val shown = checkPayerReview(ben.facts(), billId, ben.entries, owed, screen, mapOf(), mapOf(), "", "")
    check("a screen showing every fact passes", shown.isEmpty(), "$screen")
    val noAddress = checkPayerReview(ben.facts(), billId, ben.entries, owed,
                                     screen.map { if (it == "to u1ana") "to your contact" else it },
                                     mapOf(), mapOf(), "", "")
    check("one without the output's address is told exactly that",
          noAddress == listOf(ReviewFinding(ReviewRule.OUTPUT, "the address Ana is paid at", "u1ana")),
          "$noAddress")

    println("the wallet writes the send down before it sends (§14.3)")
    // One string per bill, kept where it outlives the process. The send is the
    // wallet's; these say what the note becomes.
    val txid = "ab".repeat(32)
    check("with no note, nothing blocks a send", pendingSendBlocks(billId, null) == null, "none")
    var note: String? = pendingSendNote(billId, owed, ben.now())
    check("with the note stored, a second send from this bill is blocked",
          pendingSendBlocks(billId, note)?.damaged == false, "${pendingSendBlocks(billId, note)}")
    check("a refused send takes its note with it",
          pendingSendAfter(billId, note!!, SendEnded.REFUSED, null, false) == null, "cleared")
    check("so does one that reached the network once its records are on the bill",
          pendingSendAfter(billId, note, SendEnded.REACHED_NETWORK, txid, true) == null, "cleared")
    note = pendingSendAfter(billId, note, SendEnded.UNRESOLVED, txid, false)
    check("one built and not broadcast keeps its note, naming the transaction",
          pendingSendBlocks(billId, note)?.txid == txid, "${pendingSendBlocks(billId, note)?.txid}")
    check("and the next send is still blocked", pendingSendBlocks(billId, note) != null, "blocked")
    check("a note naming its transaction is not cleared while the wallet may still send it",
          pendingSendNamedRefusal(billId, note!!, TransactionState.WAITING) == NamedSendRefusal.WAITING,
          "${pendingSendNamedRefusal(billId, note, TransactionState.WAITING)}")
    check("nor once it went through, and may be once it expired",
          pendingSendNamedRefusal(billId, note, TransactionState.MINED) == NamedSendRefusal.MINED &&
              pendingSendNamedRefusal(billId, note, TransactionState.EXPIRED) == null,
          "${pendingSendNamedRefusal(billId, note, TransactionState.EXPIRED)}")
    check("nobody may say it never left while the wallet is still sending",
          pendingSendUnsentRefusal(billId, note!!, true, listOf()) == UnsentClaimRefusal.StillSending,
          "${pendingSendUnsentRefusal(billId, note, true, listOf())}")
    val builtSince = pendingSendUnsentRefusal(billId, note, false, listOf(OwnTransaction(txid, ben.now())))
    check("nor once the wallet built a transaction after the note was written",
          builtSince == UnsentClaimRefusal.BuiltSince(txid), "$builtSince")
    check("one built before it does not hold the note",
          pendingSendUnsentRefusal(billId, note, false,
              listOf(OwnTransaction("cd".repeat(32), "2026-10-28T19:30:00.000Z"))) == null,
          "none")
    check("a note that does not read blocks as well",
          pendingSendBlocks(billId, "{not json")?.damaged == true, "damaged")
    val lost = refusal { pendingSendRecords(ben.facts(), billId, ben.entries, "{not json", txid, ben.seed) }
    check("and nothing is recorded from it", lost != null, "${lost?.detail}")

    println("a person says the send landed; its records come from the note alone")
    val records = pendingSendRecords(ben.facts(), billId, ben.entries, note!!, txid, ben.seed)
    check("one record, for what the request carried", records.size == 1,
          "${records.size} record(s)")
    check("under the payment id a send that succeeded records",
          records.single().contains("\"${ben.me}:$txid:${ana.me}\"") &&
              paymentEntriesForSend(ben.facts(), billId, owed, txid, ben.seed)
                  .single().contains("\"${ben.me}:$txid:${ana.me}\""),
          "$txid:${ana.me.take(8)}…")
    check("and it states the ZEC it sent and the rate it was priced at",
          records.single().contains("\"zatoshi\":${owed.request.payments.single().zatoshi}") &&
              records.single().contains("\"paidAtRate\""),
          records.single())
    for (record in records) ben.add(record)
    check("asked again, nothing is recorded twice",
          pendingSendRecords(ben.facts(), billId, ben.entries, note, txid, ben.seed).isEmpty(),
          "none")
    note = null // deleted, now the records are on the bill
    check("with the note deleted, the bill can be sent from again",
          pendingSendBlocks(billId, note) == null, "none")

    ana.take(ben)
    val afterPayment = foldEntries(ana.facts(), billId, ana.entries)
    check("ana sees the payment", afterPayment.bill.payments.size == 1,
          "${afterPayment.bill.payments.map { it.id }}")
    check("and it is not confirmed", afterPayment.bill.confirmedPayments.isEmpty(),
          "${afterPayment.bill.confirmedPayments}")
    val paid = afterPayment.bill.payments.single()
    val toConfirm = awaitingMyConfirmation(ana.facts(), billId, ana.entries)
    check("ana is shown it as hers to confirm", toConfirm.map { it.id } == listOf(paid.id),
          "${toConfirm.map { it.id }}")
    check("and ben, who paid it, is shown nothing to confirm",
          awaitingMyConfirmation(ben.facts(), billId, ben.entries).isEmpty(), "none")
    check("ben may not withdraw his record while its transaction is mined",
          ownPaymentWithdrawalRefusal(paid.from, paid.method, paid.reference, ben.me,
              TransactionState.MINED) == OwnPaymentWithdrawal.MINED,
          "${ownPaymentWithdrawalRefusal(paid.from, paid.method, paid.reference, ben.me, TransactionState.MINED)}")
    check("and may once it expired unmined",
          ownPaymentWithdrawalRefusal(paid.from, paid.method, paid.reference, ben.me,
              TransactionState.EXPIRED) == null,
          "none")
    check("ana's word on ben's record is not this rule's",
          ownPaymentWithdrawalRefusal(paid.from, paid.method, paid.reference, ana.me,
              TransactionState.MINED) == null,
          "none")
    val paidPlan = planRemoval(ana.facts(), billId, ana.entries, ben.me, ana.me)
    check("once he has paid, taking ben off is blocked by the payment",
          paidPlan.blockers.map { it.block } == listOf(RemovalBlock.PAYMENT) &&
              paidPlan.blockers.single().fromThem,
          "${paidPlan.blockers.map { it.block }}")
    check("so the plan ana saw before no longer stands",
          sameRemovalPlan(unpaid, paidPlan) == RemovalPlanStanding.CHANGED, "changed")
    val confirmScreen = listOf("Ben says he paid you",
                               "${renderAmount(paid.zatoshi!!)} ZEC",
                               "priced at ${rateFigure(paid.paidAtRate!!)} EUR a ZEC",
                               "transaction ${paid.reference}")
    check("ana's confirm screen shows what §14.2 says a payee must see",
          checkPayeeReview(paid, confirmScreen, "not recorded").isEmpty(), "$confirmScreen")
    val noReference = checkPayeeReview(paid, confirmScreen.dropLast(1), "not recorded")
    check("one without the transaction is told exactly that",
          noReference.map { it.rule } == listOf(ReviewRule.PAYEE_REFERENCE), "$noReference")
    val stillOwed = obligationOf(ben.facts(), billId, ben.entries)!!
    check("so ben is asked for nothing twice", stillOwed.settlements.isEmpty(),
          "${stillOwed.settlements}")
    check("and is told what is in flight", stillOwed.awaiting.single().paid == 4500L,
          "${stillOwed.awaiting}")
    val totals = totalsOf(ben.facts(), listOf(HeldBill(billId, ben.entries)))
    val withAna = totals.standings.single()
    check("across bills, ben still owes ana, with the payment on its way",
          withAna.withId == ana.me && withAna.owedByMe == 4500L &&
              withAna.sentAwaiting == 4500L && totals.uncounted.isEmpty(),
          "$withAna")

    println("before signing, the wallet holds what it read against the request")
    val anaAddress = folded.bill.participants.first { it.id == ana.me }.payTo!!
    val sent = owed.request.payments.single().zatoshi
    val same = checkProposal(owed.request.uri!!, listOf(ProposedOutput(anaAddress, sent)))
    check("what the request asks is what would be signed",
          same.missing.isEmpty() && same.unexpected.isEmpty(), "$same")
    val dropped = checkProposal(owed.request.uri!!, emptyList())
    check("a reader that dropped the payment is caught",
          dropped.missing.map { it.address } == listOf(anaAddress), "$dropped")
    check("as the sentence to show, the same reading has nothing to say",
          proposalProblem(owed.request.uri!!, listOf(ProposedOutput(anaAddress, sent))) == null,
          "none")
    val problem = proposalProblem(owed.request.uri!!, emptyList())
    check("and the dropped one is a sentence, so nothing is built",
          problem?.isNotEmpty() == true, "$problem")

    println("a swap provider's answer is read by its status first")
    check("a 2xx body is the answer", swapAnswer(200.toUShort(), "{\"quote\":1}") == "{\"quote\":1}",
          "read")
    val noRoute = refusal { swapAnswer(400.toUShort(), "{\"message\":\"no route\"}") }
    check("a 4xx is refused with the provider's own words, and waiting will not help",
          noRoute?.detail?.contains("no route") == true && noRoute.transient == false,
          "${noRoute?.detail}")
    val upstream = refusal { swapAnswer(503.toUShort(), "upstream down") }
    check("a 5xx is refused as one to try again", upstream?.transient == true,
          "${upstream?.detail}")

    println("ana's wallet saw the transaction arrive")
    val arrivals = arrivalsOf(ana.facts(), listOf(HeldBill(billId, ana.entries)),
        listOf(IncomingTransaction(txid, sent)))
    val arrival = arrivals.arrived.singleOrNull()
    check("the payment is proposed for confirmation",
          arrival?.payment?.id == afterPayment.bill.payments.single().id,
          "arrived=${arrivals.arrived.size} short=${arrivals.short.size}")
    check("and nothing is held back as disputed, underpriced or unbound",
          arrivals.disputed.isEmpty() && arrivals.underpriced.isEmpty() && arrivals.unbound.isEmpty(),
          "disputed=${arrivals.disputed.size} underpriced=${arrivals.underpriced.size}")

    // A payee confirms a payment they can see, by the id the bill carries. One
    // transaction paying several people writes one record each, so the id is
    // not the transaction's — the transaction is in `reference`.
    ana.add(confirmPaymentEntry(ana.facts(), billId, arrival!!.payment.id, "walletReceived",
        arrival.txid, arrival.record, ana.seed))
    ben.take(ana)
    val settled = obligationOf(ben.facts(), billId, ben.entries)!!
    check("once confirmed, the debt is gone",
          settled.settlements.isEmpty() && settled.awaiting.isEmpty(),
          "settlements=${settled.settlements.size} awaiting=${settled.awaiting.size}")

    println("the log reads as a history")
    val history = historyOf(ana.facts(), billId, ana.entries)
    check("every kind a person needs is there",
          history.map { it.kind }.containsAll(listOf(
              BillEventKind.OPENED, BillEventKind.JOINED, BillEventKind.EXPENSE_ADDED,
              BillEventKind.PRICED, BillEventKind.PAYMENT_RECORDED,
              BillEventKind.PAYMENT_CONFIRMED)),
          "${history.map { it.kind }.toSet()}")
    check("newest first", history.first().at >= history.last().at,
          "${history.first().at} .. ${history.last().at}")

    println("a refusal crosses as a §12 code")
    check("a scan that is nothing is refused by its code",
          readScanned("not a bill").refusedCode?.isNotEmpty() == true,
          "${readScanned("not a bill").refusedCode}")
    check("and a code has a sentence a person can read",
          describeCode(readScanned("not a bill").refusedCode!!)?.isNotEmpty() == true &&
              describeCode("self_payment") == "You can't pay yourself." &&
              describeCode("no_such_code") == null,
          "${describeCode("self_payment")}")

    println("an address is decoded before it is paid or published")
    val unified = parseAddress("u1ay3aawlldjrmxqnjf5medr5ma6p3acnet464ht8lmwplq5cd3ugytcmlf96rrmtgwldc75x94qn4n8pgen36y8tywlq6yjk7lkf3fa8wzjrav8z2xpxqnrnmjxh8tmz6jhfh425t7f3vy6p4pd3zmqayq49efl2c4xydc0gszg660q9p")
    check("a unified address, on main, that takes a memo",
          unified.network == "main" && unified.kind == "unified" &&
              unified.receivers == listOf(2u, 3u) && unified.canReceiveMemo, "$unified")
    val transparent = parseAddress("t1Hsc1LR8yKnbbe3twRp88p6vFfC5t7DLbs")
    check("a transparent one takes none",
          transparent.kind == "p2pkh" && !transparent.canReceiveMemo, "$transparent")
    val refusedAddress = try { parseAddress("u1ana"); null } catch (e: SplitzException.Protocol) { e.code }
    check("anything else is refused by its code", refusedAddress == "address_invalid",
          "$refusedAddress")

    println("a price, asked and read without a callback")
    check("the request names one currency",
          zecPriceRequest("https://api.coingecko.com/api/v3", "EUR") ==
              "https://api.coingecko.com/api/v3/simple/price?ids=zcash&vs_currencies=eur",
          "${zecPriceRequest("https://api.coingecko.com/api/v3", "EUR")}")
    check("a code with no exponent is not asked for",
          zecPriceRequest("https://api.coingecko.com/api/v3", "XAU") == null, "XAU")
    check("the answer reads as minor units, exactly",
          zecPriceFromResponse("""{"zcash":{"eur":1222.41}}""", "EUR") == 122241L,
          "${zecPriceFromResponse("""{"zcash":{"eur":1222.41}}""", "EUR")}")

    println("the fallback price sources, asked and read the same way")
    check("binance is asked for the ZECUSDC ticker",
          binancePriceRequest("https://data-api.binance.vision") ==
              "https://data-api.binance.vision/api/v3/ticker/price?symbol=ZECUSDC",
          binancePriceRequest("https://data-api.binance.vision"))
    check("and prices USD alone",
          zecPriceFromBinance("""{"symbol":"ZECUSDC","price":"1390.54000000"}""", "USD") == 139054L &&
              zecPriceFromBinance("""{"symbol":"ZECUSDC","price":"1390.54"}""", "EUR") == null,
          "139054")
    check("coinbase prices the rest from one answer",
          zecPriceFromCoinbase("""{"data":{"currency":"ZEC","rates":{"KES":"180159.79"}}}""", "KES") ==
              18015979L && coinbasePriceRequest("https://api.coinbase.com") ==
              "https://api.coinbase.com/v2/exchange-rates?currency=ZEC",
          "18015979")
    check("two sources within the tolerance agree on the higher",
          agreedPrice(138_819L, 138_905L, 200u) == 138_905L, "${agreedPrice(138_819L, 138_905L, 200u)}")
    check("and two that are not give no price",
          agreedPrice(138_819L, 152_701L, 200u) == null, "${agreedPrice(138_819L, 152_701L, 200u)}")

    println("a payout a person declares goes first, and replaces its own kind (§9.1)")
    val swapUsdc = Payout("swap", "0xa", "USDC", null)
    val ranked = rankedPayouts(Participant("p", "P", "zOld", null, listOf()), swapUsdc)
    check("a pay-to-only record keeps its ZEC behind the new swap",
          ranked == listOf(swapUsdc, Payout("zec", "zOld", null, null)), "$ranked")
    val replaced = rankedPayouts(
        Participant("p", "P", null, null, listOf(Payout("cash", null, null, null),
            Payout("zec", "zA", null, null), Payout("swap", "0xb", "USDT", null))),
        Payout("zec", "zB", null, null))
    check("a new ZEC payout replaces the old one, the rest keep their order",
          replaced.map { it.kind } == listOf("zec", "cash", "swap") && replaced.first().address == "zB",
          "$replaced")

    println("a swap deposit is checked against the bill before it is sent (§15.7)")
    val onBase = Payout("swap", "0xbenbase", "USDC", "base")
    val onArb = Payout("swap", "0xbenarb", "USDC", "arb")
    val taxiCreate = createBillEntry(ana.facts(), "Taxi", "EUR", "equal", anaKey, null, ana.seed)
    val taxiId = Regex("\"id\":\"([^\"]+)\"").find(taxiCreate)!!.groupValues[1]
    var taxi = listOf(taxiCreate,
        joinBillEntry(ana.facts(), taxiId, "Ana", "u1ana", anaKey, listOf(), ana.seed),
        joinBillEntry(ben.facts(), taxiId, "Ben", null, benKey, listOf(onBase, onArb), ben.seed))
    taxi = taxi + addExpenseEntry(ben.facts(), taxiId, "t1", ben.me, 8000,
        """{"type":"equal","among":["${ana.me}","${ben.me}"]}""", null, ben.seed)
    taxi = taxi + setRateEntry(ana.facts(), taxiId, "EUR", 51234, null, ana.seed)
    fun quote(recipient: String, chain: String) = SwapQuote("t1deposit", recipient, null, 7_807_316L,
        "39990000", null, TradableAsset("nep141:$chain-usdc", "USDC", chain, 6),
        "2026-10-29T23:00:00.000Z", "intent-1")
    check("a deposit to ben's first payout, for what ana owes, may go",
          swapSendRefusal(ana.facts(), taxiId, taxi, quote("0xbenbase", "base"), ben.me, 4000L, null) == null,
          "none")
    check("one asked for his second payout may go too",
          swapSendRefusal(ana.facts(), taxiId, taxi, quote("0xbenarb", "arb"), ben.me, 4000L, onArb) == null,
          "none")
    val wrongRecipient = swapSendRefusal(ana.facts(), taxiId, taxi, quote("0xbenbase", "base"),
        ben.me, 4000L, onArb)
    check("one whose recipient is not the payout chosen is refused",
          wrongRecipient == SwapSendRefusal.RecipientChanged, "$wrongRecipient")
    check("the payout chosen is found by type, address, asset and chain",
          declaredPayoutIndex(listOf(onBase, onArb), onArb) == 1u &&
              declaredPayoutIndex(listOf(onBase, onArb), onArb.copy(chain = null)) == null,
          "${declaredPayoutIndex(listOf(onBase, onArb), onArb)}")
    val swapRecord = recordPaymentEntry(ana.facts(), taxiId,
        PaymentDraft("intent-1", ben.me, 4000L, "swap", "intent-1", 7_807_316L, null, null), ana.seed)
    val swapRecordId = entryId(swapRecord)
    taxi = taxi + swapRecord
    val held = swapSendRefusal(ana.facts(), taxiId, taxi, quote("0xbenbase", "base"), ben.me, 4000L, null)
    check("once a payment covers the debt, a second deposit is held for ben to confirm",
          held == SwapSendRefusal.Held(listOf(ben.me)), "$held")
    check("a swap that failed names ana's record of it to withdraw",
          failedSwapWithdrawals(ana.facts(), taxiId, taxi, "intent-1") == listOf(swapRecordId),
          "${failedSwapWithdrawals(ana.facts(), taxiId, taxi, "intent-1")}")
    check("and nothing to ben, who did not write it",
          failedSwapWithdrawals(ben.facts(), taxiId, taxi, "intent-1").isEmpty(), "none")

    println("a swap's deposit and its record come from the binding (§15.7)")
    val eur = ExchangeRate("EUR", 51234L, "2026-10-28T19:30:00.000Z", null)
    check("a debt is sized in zatoshi at the bill's rate, rounding up",
          fiatToZatoshi(4000L, eur) == 7_807_316L, "${fiatToZatoshi(4000L, eur)}")
    val deposit = swapDeposit(taxiId, quote("0xbenbase", "base"), ben.me, 4000L, eur, ana.now())
    check("a deposit is one request to the quote's address for its zatoshi",
          deposit.uri.startsWith("zcash:t1deposit?amount=0.07807316"), deposit.uri)
    check("and its note carries the swap", deposit.note.contains("\"reference\":\"intent-1\""), deposit.note)
    val needsMemo = refusal {
        swapDeposit(taxiId, quote("0xbenbase", "base").copy(depositMemo = "123"), ben.me, 4000L, eur, ana.now())
    }
    check("one whose deposit needs a memo is refused", needsMemo != null, "${needsMemo?.detail}")
    val swapEntry = swapPaymentEntry(ana.facts(), taxiId,
        quote("0xbenbase", "base").copy(minAmountOut = "39500000"), ben.me, 4000L, eur, ana.seed)
    check("its record names the asset, the chain and the floor",
          swapEntry.contains("\"note\":\"at least 39.5 USDC on base\""), swapEntry)
    check("base units read as whole tokens",
          formatBaseUnits("39990000", 6) == "39.99" && formatBaseUnits("x", 6) == null, "39.99")

    println("what every wallet derives, reads and asks before writing")
    val mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
    check("a mnemonic derives the seed every wallet derives (§15.1)",
          identitySeedFromMnemonic(mnemonic, "", 1u) == "jWQijb3QECEvHgOUb4_W7UPiUujN7D-7Nq0jYg5mrKo",
          identitySeedFromMnemonic(mnemonic, "", 1u))
    val noMnemonic = refusal { identitySeedFromMnemonic("", "", 0u) }
    check("an empty mnemonic is refused", noMnemonic != null, "${noMnemonic?.detail}")
    check("a txid in digest order is reversed (§14.7)",
          txidInSendOrder("00".repeat(31) + "ab") == "ab" + "00".repeat(31) && txidInSendOrder("abc") == null,
          "${txidInSendOrder("00".repeat(31) + "ab")}")
    check("a typed figure is read in integers (§2.1)",
          parseAmountIn("12.34", "EUR") == 1234L && parseAmountIn("1,000", "KWD") == null &&
              parseMinorUnits("12.5", 2u) == 1250L,
          "${parseAmountIn("12.34", "EUR")}")
    val gold = refusal { createBillEntry(ana.facts(), "Gold", "XAU", "equal", anaKey, null, ana.seed) }
    check("no bill is opened in a currency with no minor unit", gold != null, "${gold?.detail}")
    val taxiFolded = foldEntries(ana.facts(), taxiId, taxi)
    check("the creator is the one the fold names", taxiFolded.creatorId == ana.me, taxiFolded.creatorId)
    val benOff = planRemoval(ana.facts(), taxiId, taxi, ben.me, ana.me)
    check("taking ben off lists his join to withdraw", benOff.joins.size == 1, "${benOff.joins}")
    val off = voidEntryFor(ana.facts(), taxiId, benOff.joins.single(), ana.seed)
    check("which is refused before it is written while the bill names him",
          entryRefusal(ana.facts(), taxiId, taxi, off) == "participant_still_named",
          "${entryRefusal(ana.facts(), taxiId, taxi, off)}")
    check("and ana withdrawing her own record is not",
          entryRefusal(ana.facts(), taxiId, taxi, voidEntryFor(ana.facts(), taxiId, swapRecordId, ana.seed)) == null,
          "none")

    println("and the rest of what every wallet needs from the protocol")
    check("a first payout this wallet cannot pay is passed over for the next it can (§14.8)",
          payoutFallback(listOf("not on base", null)) == PayoutFallback(1u, "not on base") &&
              payoutFallback(listOf(null, "x")) == null,
          "${payoutFallback(listOf("not on base", null))}")
    val wrapped = TradableAsset("nep141:near-zec", "ZEC", "near", 8)
    check("native ZEC is the one on its own chain",
          zecAssetIn(listOf(wrapped, TradableAsset("nep141:zec.omft.near", "ZEC", "zec", 8))) ==
              "nep141:zec.omft.near" && zecAssetIn(listOf(wrapped)) == null,
          "nep141:zec.omft.near")
    check("a rate 5% from the live price is told by how much",
          ratePercentOff(105L, 100L) == 5L && ratePercentOff(1L, 0L) == null, "${ratePercentOff(105L, 100L)}")
    check("names a reader cannot tell apart fold alike",
          nameSkeleton("\u0410na") == nameSkeleton("ana"), nameSkeleton("\u0410na"))
    check("every participant has a display name",
          displayNames(ana.facts(), taxiId, taxi).keys == setOf(ana.me, ben.me),
          "${displayNames(ana.facts(), taxiId, taxi)}")
    val corrected = amendExpenseEntry(ben.facts(), taxiId, taxi, "${ben.me}:t1", null, 9000L, null,
        "taxi home", ben.seed)
    check("an expense is corrected from what the bill applies now",
          entryRefusal(ben.facts(), taxiId, taxi, corrected) == null, "none")
    val unknownExpense = try {
        amendExpenseEntry(ben.facts(), taxiId, taxi, "${ben.me}:t9", null, 1L, null, null, ben.seed); null
    } catch (e: SplitzException.Protocol) { e }
    check("and one the bill does not apply is refused", unknownExpense?.code == "unknown_entry",
          "${unknownExpense?.code}")
    check("ana paying by a rate she set is a concern before ben confirms it",
          concernsBeforeConfirming(ben.facts(), taxiId, taxi, "${ana.me}:intent-1", null) ==
              listOf(PaymentConcern.RATE_SET_BY_PAYER),
          "${concernsBeforeConfirming(ben.facts(), taxiId, taxi, "${ana.me}:intent-1", null)}")
    val toAna = recordPaymentEntry(ben.facts(), taxiId,
        PaymentDraft("p-memo", ana.me, 1000L, "shieldedZec", "ab".repeat(32), 2_000_000L, null, null), ben.seed)
    check("memos are read for the transactions a record to this device names",
          memoTxids(ana.facts(), listOf(HeldBill(taxiId, taxi + toAna))) == listOf("ab".repeat(32)) &&
              memoTxids(ana.facts(), listOf(HeldBill(taxiId, taxi))).isEmpty(),
          "${memoTxids(ana.facts(), listOf(HeldBill(taxiId, taxi + toAna)))}")

    println(if (failures == 0)
        "CONSUMER RESULT: kotlin drives a whole bill with no callbacks, $failures failures"
    else "CONSUMER RESULT: $failures check(s) failed")
    if (failures != 0) kotlin.system.exitProcess(1)
}
