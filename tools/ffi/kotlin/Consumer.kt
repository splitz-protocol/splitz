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
          identitySeedFromSecret(secret) == identitySeedFromSecret(secret),
          identitySeedFromSecret(secret))
    check("and is the one the protocol pins",
          identitySeedFromSecret(byteArrayOf(1, 2, 3)) ==
              "MNp3HJmtVUpkGFp2KXoi4ysYoDqKi9Sf4upQw5qvOps",
          identitySeedFromSecret(byteArrayOf(1, 2, 3)))
    check("a key of the wrong length is named, not accepted",
          billKeyProblem("AAAA") == "wrong_length" && billKeyProblem(anaKey) == null,
          "${billKeyProblem("AAAA")}")

    println("ana opens a bill and joins it")
    val create = createBillEntry(ana.facts(), "Dinner", "EUR", "equal", anaKey, ana.seed)
    ana.add(create)
    val billId = Regex("\"id\":\"([^\"]+)\"").find(create)!!.groupValues[1]
    ana.add(joinBillEntry(ana.facts(), billId, "Ana", "u1ana", anaKey, listOf(), ana.seed))

    println("ana shares it, and ben takes it from the code")
    // The bill key is the wallet's to mint and to keep; §9.4's id is public.
    val billKey = "-_" + "A".repeat(41)
    check("that key is one the cipher can use", billKeyProblem(billKey) == null, billKey)
    val payload = shareableBillPayload(ana.facts(), billId, ana.entries, billKey)
    check("the whole bill fits in one code", payload != null, "${payload?.length} characters")
    val scanned = readScanned(payload!!)
    check("the scan names the same bill", scanned.billId == billId, "${scanned.billId}")
    check("and carries the key", scanned.billKey == billKey, "${scanned.billKey}")
    ben.entries = mergeEntries(ben.entries, scanned.entries).entries
    ben.add(joinBillEntry(ben.facts(), billId, "Ben", "u1ben", benKey, listOf(), ben.seed))

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
    val opened = openBlobs(fetched, billKey)
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
    ana.add(addExpenseEntry(ana.facts(), billId, "x1", ana.me, 9000,
        """{"type":"equal","among":["${ana.me}","${ben.me}"]}""", "dinner", ana.seed))
    ana.add(setRateEntry(ana.facts(), billId, "EUR", 300000, "a fixed feed", ana.seed))

    val folded = foldEntries(ana.facts(), billId, ana.entries)
    check("both people are on the bill", folded.bill.participants.size == 2,
          folded.bill.participants.joinToString { it.id })
    check("nothing was set aside", folded.setAside.isEmpty(), "${folded.setAside}")
    check("both keys are bound under §10.7",
          folded.identities.bound == mapOf(ana.me to anaKey, ben.me to benKey),
          "${folded.identities.bound.keys}")

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
    val shown = checkPayerReview(ben.facts(), billId, ben.entries, owed, screen, mapOf(), mapOf(), "")
    check("a screen showing every fact passes", shown.isEmpty(), "$screen")
    val noAddress = checkPayerReview(ben.facts(), billId, ben.entries, owed,
                                     screen.map { if (it == "to u1ana") "to your contact" else it },
                                     mapOf(), mapOf(), "")
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
    check("a note that does not read blocks as well",
          pendingSendBlocks(billId, "{not json")?.damaged == true, "damaged")
    val lost = refusal { pendingSendRecords(ben.facts(), billId, ben.entries, "{not json", txid, ben.seed) }
    check("and nothing is recorded from it", lost != null, "${lost?.detail}")

    println("a person says the send landed; its records come from the note alone")
    val records = pendingSendRecords(ben.facts(), billId, ben.entries, note!!, txid, ben.seed)
    check("one record, for what the request carried", records.size == 1,
          "${records.size} record(s)")
    check("under the payment id a send that succeeded records",
          records.single().contains("\"$txid:${ana.me}\"") &&
              paymentEntriesForSend(ben.facts(), billId, owed, txid, ben.seed)
                  .single().contains("\"$txid:${ana.me}\""),
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

    println("ana's wallet saw the transaction arrive")
    val arrivals = arrivalsOf(ana.facts(), listOf(HeldBill(billId, ana.entries)),
        listOf(IncomingTransaction(txid, sent)))
    val arrival = arrivals.arrived.singleOrNull()
    check("the payment is proposed for confirmation",
          arrival?.payment?.id == afterPayment.bill.payments.single().id,
          "arrived=${arrivals.arrived.size} short=${arrivals.short.size}")

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

    println(if (failures == 0)
        "CONSUMER RESULT: kotlin drives a whole bill with no callbacks, $failures failures"
    else "CONSUMER RESULT: $failures check(s) failed")
    if (failures != 0) kotlin.system.exitProcess(1)
}
