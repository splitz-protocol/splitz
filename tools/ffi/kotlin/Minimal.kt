import uniffi.splitz_ffi.*

/// The smallest wallet that opens a bill and asks what it owes.
///
/// Seven interfaces, one session, four calls. Everything that touches a key, a
/// socket, a clock or a screen is the wallet's, and is passed in.
class Wallet(private val who: String, private val address: String) :
    SplitsWallet, SecretStore, BillStorage, SplitsRelay, WalletSender, ZecPrices, SwapProvider {

    private val secrets = HashMap<String, String>()
    private val bills = LinkedHashMap<String, String>()
    private var minute = 0
    private var counter = 0

    // §15.1 — who this device speaks as, its clock and its randomness.
    override fun accountId() = who
    override fun viewingKey() = "uview-$who"
    override fun now(): String {
        // A §9.3 instant: UTC, exactly three fractional digits, fixed width,
        // so a log sorts as text on every device (§10.2).
        minute += 1
        return "2026-10-28T19:%02d:00.000Z".format(minute)
    }
    override fun randomBytes(byteCount: UInt): ByteArray {
        // §9.4 derives a bill's id from a nonce. A shipped wallet uses the
        // platform's secure randomness here; two bills opened in one second
        // by one person are one bill if this can be guessed.
        counter += 1
        return ByteArray(byteCount.toInt()) { (counter + it).toByte() }
    }

    // §15.3 — the platform keychain in a shipped build.
    override fun read(key: String) = if (key in bills) bills[key] else secrets[key]
    override fun write(key: String, value: String) {
        if (key.startsWith("splitz_bill_key_") || key.startsWith("splitz_identity_")) {
            secrets[key] = value
        } else {
            bills[key] = value
        }
    }
    override fun delete(key: String) { secrets.remove(key); bills.remove(key) }

    // §15.4 — entries, never a folded summary: a stored summary is a second
    // source of truth that goes stale without saying so.
    override fun keys(prefix: String) = bills.keys.filter { it.startsWith(prefix) }
    override fun sweepUnfinishedWrites() = 0u

    // §15.5 — optional. A bill works with no relay; only the catch-up for
    // somebody who left early is missing without one.
    override fun push(channel: String, blobs: List<String>) {}
    override fun fetch(channel: String) = emptyList<String>()

    // §15.2 — one call takes the whole request, which is why a payer who owes
    // four people signs once.
    override fun send(paymentRequestUri: String) =
        WalletSendOutcome(WalletSendPhase.SUCCEEDED, "tx-1", null, null)
    override fun payToAddress() = address

    // §15.6 — minor units per ZEC, as an integer. Null is an ordinary answer.
    override fun minorUnitsPerZec(currency: String) = if (currency == "EUR") 300000L else null

    // §15.7 — a build that arranges no swaps says so rather than doing nothing.
    override fun tradableAssets() = emptyList<TradableAsset>()
    override fun quote(a: TradableAsset, amount: Long, recipient: String, refundTo: String):
        SwapQuote = throw SplitzException.Host("this build arranges no swaps", false)
    override fun statusOf(quote: SwapQuote): SwapStatus =
        throw SplitzException.Host("this build arranges no swaps", false)
}

fun main() {
    val ana = Wallet("ana", "u1ana")
    val ben = Wallet("ben", "u1ben")
    val session = SplitzSession(ben, ben, ben, ben, ben, ben, ben)

    // Ana opened the bill on her phone and handed over this payload.
    val fromAna = SplitzSession(ana, ana, ana, ana, ana, ana, ana).let { hers ->
        val bill = hers.createBill("Dinner", "EUR", "equal")
        hers.joinBill(bill, "Ana", "u1ana")
        hers.addExpense(
            bill, "x1", "ana", 9000,
            """{"type":"equal","among":["ana","ben"]}""", "dinner",
        )
        hers.setRate(bill, "EUR", 300000, "a fixed feed")
        hers.shareableBill(bill) ?: error("this bill is too large for one code")
    }

    val billId = session.acceptScan(fromAna)
    session.joinBill(billId, "Ben", "u1ben")

    // Nothing is owed until the bill says who is on it, so join first.
    val owed = session.obligation(billId, emptyList())
        ?: error("this bill carries no rate, so nothing can be priced")

    for (settlement in owed.settlements) {
        println("${settlement.from} owes ${settlement.to} ${settlement.amount}")
    }
    for (missing in owed.request.unpayable) {
        // Reported, never dropped: a dropped output settles less than the
        // plan says, and the payer cannot tell.
        println("cannot pay ${missing.id}: ${missing.reason}")
    }
    println(owed.request.uri ?: "nothing could be carried")
}
