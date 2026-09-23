/// The smallest Kotlin wallet that renders a payment request.
///
/// INTEGRATING.md quotes everything below the marker verbatim;
/// `tools/docs/blocks.py` fails when the two drift, and `tools/ffi/kotlin.sh`
/// runs it, so the document's sample is a sample that executed.
// docs:begin
import uniffi.splitz_ffi.*

/// The facts §15.1 says a wallet owns, for one call. A §9.3 instant and
/// sixteen unpredictable bytes are the wallet's to supply: this library reads
/// no clock (§13) and owns no entropy.
fun facts(me: String, payTo: String, at: String, nonce: Int) =
    HostFacts(me, payTo, at, ByteArray(16) { (nonce + it).toByte() })

/// The Ed25519 seed a wallet keeps in the platform keychain, as §9.4 writes a
/// key: 32 bytes, unpadded base64url.
fun seed(first: Int): String = java.util.Base64.getUrlEncoder().withoutPadding()
    .encodeToString(ByteArray(32) { (first + it).toByte() })

fun main() {
    val anaSeed = seed(1)
    val benSeed = seed(90)

    // Ana's device writes four entries. Each comes back as the JSON §9.3
    // canonicalises, with §9.5's id already derived; the wallet stores the
    // string and never inspects it.
    val create = createBillEntry(facts("ana", "u1ana", "2026-10-28T19:31:00.000Z", 1),
        "Dinner", "EUR", "equal", identityKeyFromSeed(anaSeed), anaSeed)

    // The bill these entries belong to, read back from the entry that opened
    // it. Every other entry is signed on it (§10.6), and every fold names it,
    // so a create for another bill pushed into the channel cannot make this
    // one unopenable.
    val billId = Regex("\"id\":\"([^\"]+)\"").find(create)!!.groupValues[1]

    val anaLog = listOf(
        create,
        joinBillEntry(facts("ana", "u1ana", "2026-10-28T19:32:00.000Z", 2), billId,
            "Ana", "u1ana", identityKeyFromSeed(anaSeed), anaSeed),
        addExpenseEntry(facts("ana", "u1ana", "2026-10-28T19:33:00.000Z", 3), billId,
            "x1", "ana", 9000, """{"type":"equal","among":["ana","ben"]}""",
            "dinner", anaSeed),
        // §7 snapshots one rate onto the bill, so six devices do not price one
        // dinner six ways. 300000 minor units per ZEC is €3000.00.
        setRateEntry(facts("ana", "u1ana", "2026-10-28T19:34:00.000Z", 4), billId,
            "EUR", 300000, "a fixed feed", anaSeed),
    )

    // Ben's own device writes Ben's join: §10.4 decides what an entry's author
    // may say, and a participant joins for themselves.
    val benLog = listOf(
        joinBillEntry(facts("ben", "u1ben", "2026-10-28T19:35:00.000Z", 5), billId,
            "Ben", "u1ben", identityKeyFromSeed(benSeed), benSeed),
    )

    // Merging is how two devices come to agree (§10.2). It is a set union by
    // id, in either direction, any number of times.
    val log = mergeEntries(anaLog, benLog).entries

    val benFacts = facts("ben", "u1ben", "2026-10-28T19:36:00.000Z", 6)
    val folded = foldEntries(benFacts, billId, log)
    println("on the bill: " + folded.bill.participants.joinToString { it.id })
    // Render these. An entry the fold set aside is one a person cannot see
    // otherwise, and its §12 code is what a wallet turns into a sentence.
    println("set aside: " + folded.setAside)

    // Null when the bill carries no rate: an unpriced bill is an ordinary
    // bill, not a refusal. The second argument names the contested
    // participants the payer has been shown and chosen to pay anyway (§10.7).
    val owed = obligationOf(benFacts, billId, log, listOf())
        ?: error("a bill with a rate owes something")

    val settlement = owed.settlements.single()
    println("ben pays ${settlement.amount} to ${settlement.to}")
    // The wallet broadcasts this; sending is not the library's (§13.3).
    println("request: ${owed.request.uri}")
    // Never dropped. A request that silently covers three debts of four is
    // indistinguishable, to the payer, from one that covers all of them.
    println("withheld: ${owed.request.withheldMinorUnits}")

    check(settlement.to == "ana" && settlement.amount == 4500L) {
        "half of 9000 is 4500 to ana, saw ${settlement.amount} to ${settlement.to}"
    }
    check(owed.request.uri!!.startsWith("zcash:u1ana")) { "${owed.request.uri}" }
    check(owed.request.withheldMinorUnits == 0L) { "${owed.request.withheldMinorUnits}" }
    println("DOC RESULT: kotlin")
}
