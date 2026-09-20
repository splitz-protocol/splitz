/// A wallet, written against the generated Kotlin and nothing else.
///
/// It implements all seven of SPEC.md §15's interfaces and drives one bill
/// from opening it to settling it, across two devices sharing one relay. A
/// binding that compiles is not a binding that carries a callback: this calls
/// back into Kotlin for storage, secrets, the clock, randomness, the relay and
/// the send on every step.
import uniffi.splitz_ffi.*

var failures = 0

fun check(name: String, ok: Boolean, saw: String) {
    if (ok) println("  PASS  $name — $saw")
    else { failures += 1; println("  FAIL  $name — $saw") }
}

/// A clock that stands still until it is moved.
///
/// §9.3 instants order a log (§10.2), so a log that reordered between runs
/// could not be asserted. Fixed width, three fractional digits.
class Clock(private var minute: Int = 0) {
    fun tick() { minute += 1 }
    fun now(): String {
        val total = 19 * 60 + 30 + minute
        return "2026-10-28T%02d:%02d:00.000Z".format(total / 60, total % 60)
    }
}

class Device(val id: String, val payTo: String, val relay: Relay, seed: Byte) {
    val clock = Clock()
    private var counter = seed

    val wallet = object : SplitsWallet {
        override fun accountId() = id
        override fun viewingKey() = "uview-$id"
        override fun now() = clock.now()
        override fun randomBytes(byteCount: UInt): ByteArray {
            counter = (counter + 1).toByte()
            return ByteArray(byteCount.toInt()) { (counter + it).toByte() }
        }
    }

    val secrets = Keychain()
    val storage = Store()
    val sender = Sender(id, payTo)

    val prices = object : ZecPrices {
        override fun minorUnitsPerZec(currency: String): Long? =
            if (currency == "EUR") 3_000_00L else null
    }

    /// A build with no provider says so rather than doing nothing (§15.7).
    val swaps = object : SwapProvider {
        override fun tradableAssets() = listOf<TradableAsset>()
        override fun quote(
            asset: TradableAsset, amountInZatoshi: Long,
            recipient: String, refundTo: String,
        ): SwapQuote = throw SplitzException.Host("this build arranges no swaps", false)
        override fun statusOf(quote: SwapQuote): SwapStatus =
            throw SplitzException.Host("this build arranges no swaps", false)
    }

    val session: SplitzSession =
        SplitzSession(wallet, secrets, storage, sender, relay, prices, swaps)
}

/// The platform keychain, in a demonstration's memory. §15.3 requires a value
/// written to outlive the process that wrote it, so a shipped build puts these
/// where the platform does.
class Keychain : SecretStore {
    val values = LinkedHashMap<String, String>()
    override fun read(key: String) = values[key]
    override fun write(key: String, value: String) { values[key] = value }
    override fun delete(key: String) { values.remove(key) }
}

/// §15.4's store, for the length of this run.
class Store : BillStorage {
    val values = LinkedHashMap<String, String>()
    override fun read(key: String) = values[key]
    override fun write(key: String, value: String) { values[key] = value }
    override fun delete(key: String) { values.remove(key) }
    override fun keys(prefix: String) = values.keys.filter { it.startsWith(prefix) }
    /// Nothing to sweep: a map cannot be half written.
    override fun sweepUnfinishedWrites() = 0u
}

/// §15.2's send path. One call takes the whole request, which is why a payer
/// who owes four people signs once.
class Sender(val id: String, val payTo: String) : WalletSender {
    var sent: String? = null
    override fun send(paymentRequestUri: String): WalletSendOutcome {
        sent = paymentRequestUri
        return WalletSendOutcome(WalletSendPhase.SUCCEEDED, "tx-$id-1", null, null)
    }
    override fun payToAddress() = payTo
}

/// A relay that keeps blobs in memory, shared by the two devices.
class Relay : SplitsRelay {
    private val channels = LinkedHashMap<String, MutableList<String>>()
    var pushes = 0
    override fun push(channel: String, blobs: List<String>) {
        pushes += 1
        val held = channels.getOrPut(channel) { mutableListOf() }
        for (blob in blobs) if (blob !in held) held.add(blob)
    }
    override fun fetch(channel: String): List<String> = channels[channel] ?: emptyList()
    fun sizeOf(channel: String) = (channels[channel] ?: emptyList<String>()).size
    fun channels() = channels.keys.toList()
}

fun main() {
    val relay = Relay()
    val ana = Device("ana", "u1ana", relay, 1)
    val ben = Device("ben", "u1ben", relay, 90)

    println("the seam carries a callback in both directions")
    check("the session read an identity out of Kotlin's keychain",
          ana.secrets.values.isNotEmpty(), "keys=${ana.secrets.values.keys}")
    check("an identity from a viewing key is recoverable",
          ana.session.identityIsRecoverable(), "recoverable")
    check("the identity key is 43 unpadded base64url characters",
          ana.session.identityKey().length == 43, ana.session.identityKey())

    println("ana opens a bill and joins it")
    val billId = ana.session.createBill("Dinner", "EUR", "equal")
    ana.clock.tick()
    ana.session.joinBill(billId, "Ana", "u1ana")
    check("the bill is held", ana.session.billIds() == listOf(billId), billId)
    check("ana has joined", ana.session.hasJoined(billId), "joined")

    println("ben scans the invite and joins")
    val invite = ana.session.inviteFor(billId, "Ana")
    check("the invite names the bill and carries a key",
          invite.startsWith("splitz://join?") && invite.contains("k="), invite.take(48) + "…")
    ana.session.sync(billId)
    check("the relay was pushed to under a hashed channel",
          relay.channels().size == 1 && relay.channels()[0] != billId,
          "channel=${relay.channels()[0].take(16)}… billId=$billId")

    val scanned = ben.session.acceptScan(invite)
    check("the scan opened the same bill", scanned == billId, scanned)
    ben.session.sync(billId)
    ben.clock.tick(); ben.clock.tick()
    ben.session.joinBill(billId, "Ben", "u1ben")
    ben.session.sync(billId)

    println("ana adds an expense both share, and prices the bill")
    ana.session.sync(billId)
    ana.clock.tick()
    ana.session.addExpense(
        billId, "x1", "ana", 9000,
        """{"type":"equal","among":["ana","ben"]}""", "dinner",
    )
    ana.clock.tick()
    ana.session.setRate(billId, "EUR", ana.prices.minorUnitsPerZec("EUR")!!, "a fixed feed")
    ana.session.sync(billId)

    val folded = ana.session.fold(billId)
    check("both people are on the bill", folded.bill.participants.size == 2,
          folded.bill.participants.map { it.id }.toString())
    check("nothing was set aside", folded.setAside.isEmpty(), folded.setAside.toString())
    check("no identity is contested", folded.identities.contested.isEmpty(),
          folded.identities.contested.toString())
    check("both keys are bound under §10.7", folded.identities.bound.size == 2,
          folded.identities.bound.keys.toString())
    check("the expense is nine thousand minor units",
          folded.bill.expenses.single().amount == 9000L, folded.bill.expenses.single().amount.toString())
    check("the bill is priced", folded.bill.rate?.minorUnitsPerZec == 300000L,
          folded.bill.rate.toString())

    println("ben owes half of it, and settles")
    ben.session.sync(billId)
    val obligation = ben.session.obligation(billId, listOf())
    check("ben has an obligation", obligation != null, obligation?.request?.uri ?: "none")
    val settlement = obligation!!.settlements.single()
    check("it is four and a half thousand to ana",
          settlement.to == "ana" && settlement.amount == 4500L,
          "${settlement.to} ${settlement.amount}")
    check("the request is a ZIP 321 URI naming ana's address",
          obligation.request.uri!!.startsWith("zcash:u1ana"), obligation.request.uri!!)
    check("nothing is withheld", obligation.request.withheldMinorUnits == 0L,
          obligation.request.withheldMinorUnits.toString())
    check("nobody is unpayable", obligation.request.unpayable.isEmpty(),
          obligation.request.unpayable.toString())

    ben.clock.tick()
    val settled = ben.session.settle(billId, listOf())
    check("the wallet was handed the whole request in one call",
          ben.sender.sent == obligation.request.uri, ben.sender.sent ?: "nothing")
    check("one payment was recorded", settled.sent && settled.recorded == 1u,
          "sent=${settled.sent} recorded=${settled.recorded} txid=${settled.txid}")

    println("a record is a claim until the payee confirms")
    ben.session.sync(billId)
    ana.session.sync(billId)
    val afterPayment = ana.session.fold(billId)
    check("ana sees the payment", afterPayment.bill.payments.size == 1,
          afterPayment.bill.payments.toString())
    check("and it is not confirmed", afterPayment.bill.confirmedPayments.isEmpty(),
          afterPayment.bill.confirmedPayments.toString())
    val stillOwed = ben.session.obligation(billId, listOf())!!
    check("so ben is asked for nothing twice", stillOwed.settlements.isEmpty(),
          stillOwed.settlements.toString())
    check("and is told what is in flight", stillOwed.awaiting.single().paid == 4500L,
          stillOwed.awaiting.toString())

    ana.clock.tick()
    ana.session.confirmPayment(
        billId, afterPayment.bill.payments.single().id, "recipientConfirmed", null)
    ana.session.sync(billId)
    ben.session.sync(billId)
    check("once confirmed, the debt is gone",
          ben.session.obligation(billId, listOf())!!.let {
              it.settlements.isEmpty() && it.awaiting.isEmpty()
          },
          ben.session.obligation(billId, listOf())!!.toString().take(80))

    println("the log reads as a history")
    val history = ana.session.history(billId)
    val kinds = history.map { it.kind }
    check("every kind a person needs is there",
          kinds.containsAll(listOf(
              BillEventKind.OPENED, BillEventKind.JOINED, BillEventKind.EXPENSE_ADDED,
              BillEventKind.PRICED, BillEventKind.PAYMENT_RECORDED,
              BillEventKind.PAYMENT_CONFIRMED)),
          kinds.toString())
    check("newest first", history.first().at >= history.last().at,
          "${history.first().at} .. ${history.last().at}")
    check("every entry applied", history.all { it.applied }, "${history.size} lines")

    println("a build with no swap provider says so")
    check("assets are empty rather than invented", ana.session.swapAssets().isEmpty(), "[]")
    try {
        ana.session.swapQuote(TradableAsset("x", "USDC", "base", 6), 1, "0xcara", "u1ana")
        check("a quote is refused", false, "it was not")
    } catch (e: SplitzException.Host) {
        check("a quote is refused", true, e.message ?: "")
    }

    println("a refusal crosses as a §12 code")
    try {
        ana.session.acceptScan("not a bill")
        check("a scan that is nothing is refused", false, "it was not")
    } catch (e: SplitzException.Protocol) {
        check("a scan that is nothing is refused by its code", e.code.isNotEmpty(), e.code)
    }

    println(if (failures == 0) "CONSUMER RESULT: the binding carries a whole bill, $failures failures"
            else "CONSUMER RESULT: $failures check(s) failed")
    if (failures != 0) kotlin.system.exitProcess(1)
}
