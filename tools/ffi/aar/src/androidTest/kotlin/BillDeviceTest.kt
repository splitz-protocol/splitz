/// A wallet driving one bill through the packaged splitz AAR.
///
/// Every symbol below comes from `classes.jar` inside `splitz-release.aar`. No
/// generated source is on this module's source path, so a name the package
/// fails to carry is a compile error here rather than a device crash later.
///
/// The run is an INSTRUMENTED test: it executes on a device or emulator, so
/// the native code is the AAR's own `jni/<abi>/libsplitz_ffi.so`, extracted
/// from the APK by the platform. That is the claim the JVM twin cannot make,
/// and it is why this file exists beside it.
import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import com.sun.jna.NativeLibrary
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.splitz_ffi.BillEventKind
import uniffi.splitz_ffi.ExchangeRate
import uniffi.splitz_ffi.HeldBill
import uniffi.splitz_ffi.HostFacts
import uniffi.splitz_ffi.NamedSendRefusal
import uniffi.splitz_ffi.OwnPaymentWithdrawal
import uniffi.splitz_ffi.OwnTransaction
import uniffi.splitz_ffi.Participant
import uniffi.splitz_ffi.PaymentConcern
import uniffi.splitz_ffi.PaymentDraft
import uniffi.splitz_ffi.Payout
import uniffi.splitz_ffi.PayoutFallback
import uniffi.splitz_ffi.RandomBytes
import uniffi.splitz_ffi.RemovalBlock
import uniffi.splitz_ffi.RemovalPlanStanding
import uniffi.splitz_ffi.ReviewRule
import uniffi.splitz_ffi.SendEnded
import uniffi.splitz_ffi.SplitzException
import uniffi.splitz_ffi.SplitzRelay
import uniffi.splitz_ffi.SwapQuote
import uniffi.splitz_ffi.SwapSendRefusal
import uniffi.splitz_ffi.TradableAsset
import uniffi.splitz_ffi.TransactionState
import uniffi.splitz_ffi.UnsentClaimRefusal
import uniffi.splitz_ffi.addExpenseEntry
import uniffi.splitz_ffi.agreedPrice
import uniffi.splitz_ffi.amendExpenseEntry
import uniffi.splitz_ffi.billKeyProblem
import uniffi.splitz_ffi.blobsToPush
import uniffi.splitz_ffi.channelForBill
import uniffi.splitz_ffi.checkPayeeReview
import uniffi.splitz_ffi.concernsBeforeConfirming
import uniffi.splitz_ffi.confirmPaymentEntry
import uniffi.splitz_ffi.createBillEntry
import uniffi.splitz_ffi.declaredPayoutIndex
import uniffi.splitz_ffi.displayNames
import uniffi.splitz_ffi.entryRefusal
import uniffi.splitz_ffi.failedSwapWithdrawals
import uniffi.splitz_ffi.fiatToZatoshi
import uniffi.splitz_ffi.foldEntries
import uniffi.splitz_ffi.formatBaseUnits
import uniffi.splitz_ffi.historyOf
import uniffi.splitz_ffi.identityKeyFromSeed
import uniffi.splitz_ffi.identitySeedFromMnemonic
import uniffi.splitz_ffi.joinBillEntry
import uniffi.splitz_ffi.memoTxids
import uniffi.splitz_ffi.mergeEntries
import uniffi.splitz_ffi.nameSkeleton
import uniffi.splitz_ffi.newBillKey
import uniffi.splitz_ffi.obligationOf
import uniffi.splitz_ffi.openBlobs
import uniffi.splitz_ffi.ownPaymentWithdrawalRefusal
import uniffi.splitz_ffi.parseAmountIn
import uniffi.splitz_ffi.parseMinorUnits
import uniffi.splitz_ffi.participantIdForKey
import uniffi.splitz_ffi.paymentEntriesForSend
import uniffi.splitz_ffi.payoutFallback
import uniffi.splitz_ffi.pendingSendAfter
import uniffi.splitz_ffi.pendingSendNamedRefusal
import uniffi.splitz_ffi.pendingSendNote
import uniffi.splitz_ffi.pendingSendUnsentRefusal
import uniffi.splitz_ffi.planRemoval
import uniffi.splitz_ffi.rankedPayouts
import uniffi.splitz_ffi.rateFigure
import uniffi.splitz_ffi.ratePercentOff
import uniffi.splitz_ffi.readScanned
import uniffi.splitz_ffi.recordPaymentEntry
import uniffi.splitz_ffi.renderAmount
import uniffi.splitz_ffi.sameRemovalPlan
import uniffi.splitz_ffi.setRateEntry
import uniffi.splitz_ffi.shareableBillPayload
import uniffi.splitz_ffi.splitWithout
import uniffi.splitz_ffi.swapDeposit
import uniffi.splitz_ffi.swapPaymentEntry
import uniffi.splitz_ffi.swapSendRefusal
import uniffi.splitz_ffi.txidInSendOrder
import uniffi.splitz_ffi.voidEntryFor
import uniffi.splitz_ffi.zecAssetIn

/// One device: its log, its clock, its randomness. The library is handed
/// facts; it never calls back.
private class Device(private val seedByte: Int) {
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
    /// platform's own entropy.
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

/// A signed entry's own id: the one beside its signature, not its payload's.
private fun entryId(entry: String): String =
    Regex("\"id\":\"([^\"]+)\",\"sig\":").find(entry)!!.groupValues[1]

@RunWith(AndroidJUnit4::class)
class BillTest {
    private var failures = 0

    private fun check(name: String, ok: Boolean, saw: String) {
        if (ok) println("  PASS  $name — $saw")
        else { failures += 1; println("  FAIL  $name — $saw") }
    }

    @Test
    fun aWalletDrivesOneBillThroughThePackagedAar() {
        // Name the binary this run actually executes. The AAR's Android `.so`
        // files are not loadable by this JVM; the host cdylib is.
        val resolved = NativeLibrary.getInstance("splitz_ffi").file.absolutePath
        println("native library loaded: $resolved")

        // The host twin looks for a `.class` resource to prove the binding
        // came from the AAR. Android ships DEX, so that probe answers
        // "not on the classpath" on every device and settles nothing. What
        // does settle it here is the library: an Android ELF named
        // `libsplitz_ffi.so`, loaded from the APK, and never the host's
        // `.dylib` under `rust/target`.
        check("the native code is the AAR's Android library, not a host build",
              resolved.endsWith("libsplitz_ffi.so") &&
                  !resolved.contains("rust/target") &&
                  !resolved.endsWith(".dylib"),
              resolved)

        // The relay client the AAR ships, against a live tools/relay/server.py
        // on the host, reached through `adb reverse`.
        val relay = SplitzRelay(InstrumentationRegistry.getArguments().getString("relay")!!)
        val ana = Device(1)
        val ben = Device(90)
        val anaKey = ana.key
        val benKey = ben.key
        check("an identity key is 43 unpadded base64url characters",
              anaKey.length == 43, anaKey)

        println("ana opens a bill and joins it")
        // The bill key is the wallet's to keep, minted from the platform's own
        // entropy; §9.4's id is public.
        val billKey = newBillKey(RandomBytes(ByteArray(32).also { java.security.SecureRandom().nextBytes(it) }))
        // Minted first: the create entry commits to it (§9.4).
        val create = createBillEntry(ana.facts(), "Dinner", "EUR", "equal", anaKey, billKey, ana.seed)
        ana.add(create)
        val billId = Regex("\"id\":\"([^\"]+)\"").find(create)!!.groupValues[1]
        ana.add(joinBillEntry(ana.facts(), billId, "Ana", "u1ana", anaKey, listOf(), ana.seed))

        println("ana shares it, and ben takes it from the code")
        check("that key is one the cipher can use", billKeyProblem(billKey) == null, billKey)
        val payload = shareableBillPayload(ana.facts(), billId, ana.entries, billKey)
        check("the whole bill fits in one code", payload != null, "${payload?.length} characters")
        val scanned = readScanned(payload!!)
        check("the scan names the same bill", scanned.billId == billId, "${scanned.billId}")
        ben.entries = mergeEntries(ben.entries, scanned.entries).entries
        ben.add(joinBillEntry(ben.facts(), billId, "Ben", "u1ben", benKey, listOf(), ben.seed))

        println("the two logs move through a relay that holds only ciphertext")
        val channel = channelForBill(billId)
        check("the channel is the bill id's hash, never the id",
              channel != billId && channel.length == 64, channel.take(16) + "…")
        // Pushed as they are held: each was signed when it was written.
        val blobs = blobsToPush(ben.entries, billKey)
        relay.push(channel, blobs)
        relay.push(channel, blobs)
        val fetched = relay.fetch(channel)
        check("the relay holds each blob once, though it was pushed twice",
              fetched.size == blobs.size && fetched.toSet() == blobs.toSet(),
              "${fetched.size} of ${blobs.size}")
        val opened = openBlobs(fetched, billId, billKey)
        check("every blob opened", opened.unopenable == 0u, "unopenable=${opened.unopenable}")
        ana.entries = mergeEntries(ana.entries, opened.entries).entries

        println("ana adds an expense they share, and prices it")
        val dinner = addExpenseEntry(ana.facts(), billId, "x1", ana.me, 9000,
            """{"type":"equal","among":["${ana.me}","${ben.me}"]}""", "dinner", ana.seed)
        ana.add(dinner)
        ana.add(setRateEntry(ana.facts(), billId, "EUR", 300000, "a fixed feed", ana.seed))

        val folded = foldEntries(ana.facts(), billId, ana.entries)
        check("both people are on the bill", folded.bill.participants.size == 2,
              folded.bill.participants.joinToString { it.id })
        check("nothing was set aside", folded.setAside.isEmpty(), "${folded.setAside}")

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
        val notSplit = try { splitWithout("{", ben.me); null } catch (e: SplitzException.Host) { e }
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

        println("a send the wallet wrote down is not cleared on a person's word (§14.3)")
        val txid = "ab".repeat(32)
        val note = pendingSendNote(billId, owed, ben.now())
        check("nobody may say it never left while the wallet is still sending",
              pendingSendUnsentRefusal(billId, note, true, listOf()) == UnsentClaimRefusal.StillSending,
              "${pendingSendUnsentRefusal(billId, note, true, listOf())}")
        val builtSince = pendingSendUnsentRefusal(billId, note, false, listOf(OwnTransaction(txid, ben.now())))
        check("nor once the wallet built a transaction after the note was written",
              builtSince == UnsentClaimRefusal.BuiltSince(txid), "$builtSince")
        check("one built before it does not hold the note",
              pendingSendUnsentRefusal(billId, note, false,
                  listOf(OwnTransaction("cd".repeat(32), "2026-10-28T19:30:00.000Z"))) == null,
              "none")
        val named = pendingSendAfter(billId, note, SendEnded.UNRESOLVED, txid, false)!!
        check("a note naming its transaction is not cleared while the wallet may still send it",
              pendingSendNamedRefusal(billId, named, TransactionState.WAITING) == NamedSendRefusal.WAITING,
              "${pendingSendNamedRefusal(billId, named, TransactionState.WAITING)}")
        check("nor once it went through, and may be once it expired",
              pendingSendNamedRefusal(billId, named, TransactionState.MINED) == NamedSendRefusal.MINED &&
                  pendingSendNamedRefusal(billId, named, TransactionState.EXPIRED) == null,
              "${pendingSendNamedRefusal(billId, named, TransactionState.EXPIRED)}")

        println("the wallet sends, then records what §14.3 allows")
        val records = paymentEntriesForSend(ben.facts(), billId, owed, "tx-ben-1", ben.seed)
        check("one record, for what the request carried", records.size == 1,
              "${records.size} record(s)")
        for (record in records) ben.add(record)

        ana.take(ben)
        val afterPayment = foldEntries(ana.facts(), billId, ana.entries)
        check("ana sees the payment", afterPayment.bill.payments.size == 1,
              "${afterPayment.bill.payments.map { it.id }}")
        check("and it is not confirmed", afterPayment.bill.confirmedPayments.isEmpty(),
              "${afterPayment.bill.confirmedPayments}")
        val paid = afterPayment.bill.payments.single()
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

        println("ana confirms the payment, and the debt clears")
        val toConfirm = afterPayment.bill.payments.single().id
        ana.add(confirmPaymentEntry(ana.facts(), billId, toConfirm, "recipientConfirmed", null,
        afterPayment.paymentDigests[toConfirm]!!, ana.seed))
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

        println("a refusal crosses as a §12 code")
        check("a scan that is nothing is refused by its code",
              readScanned("not a bill").refusedCode?.isNotEmpty() == true,
              "${readScanned("not a bill").refusedCode}")

        println("two price sources agree, or give no price")
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
        val needsMemo = try {
            swapDeposit(taxiId, quote("0xbenbase", "base").copy(depositMemo = "123"), ben.me, 4000L, eur, ana.now())
            null
        } catch (e: SplitzException.Host) { e }
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
        val noMnemonic = try { identitySeedFromMnemonic("", "", 0u); null } catch (e: SplitzException.Host) { e }
        check("an empty mnemonic is refused", noMnemonic != null, "${noMnemonic?.detail}")
        check("a txid in digest order is reversed (§14.7)",
              txidInSendOrder("00".repeat(31) + "ab") == "ab" + "00".repeat(31) && txidInSendOrder("abc") == null,
              "${txidInSendOrder("00".repeat(31) + "ab")}")
        check("a typed figure is read in integers (§2.1)",
              parseAmountIn("12.34", "EUR") == 1234L && parseAmountIn("1,000", "KWD") == null &&
                  parseMinorUnits("12.5", 2u) == 1250L,
              "${parseAmountIn("12.34", "EUR")}")
        val gold = try {
            createBillEntry(ana.facts(), "Gold", "XAU", "equal", anaKey, null, ana.seed); null
        } catch (e: SplitzException.Host) { e }
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

        println("AAR DEVICE RESULT: $failures failure(s)")
        assertEquals("checks that failed", 0, failures)
    }
}
