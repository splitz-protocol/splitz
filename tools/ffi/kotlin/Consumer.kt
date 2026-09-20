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

/// One device: its log, its clock, its randomness.
class Device(val me: String, val payTo: String?, private val seedByte: Int) {
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

    fun facts() = HostFacts(me, payTo, now(), nonce())

    /// The Ed25519 seed this account signs with, as §9.4 writes a key. A
    /// shipped wallet keeps this in the platform keychain.
    val seed: String = java.util.Base64.getUrlEncoder().withoutPadding()
        .encodeToString(ByteArray(32) { (seedByte + it).toByte() })

    fun add(entry: String) {
        entries = mergeEntries(entries, listOf(entry)).entries
    }

    fun take(other: Device) {
        entries = mergeEntries(entries, other.entries).entries
    }
}

/// A relay holding ciphertext, shared by both devices. It holds no key.
class Relay {
    private val channels = LinkedHashMap<String, MutableList<String>>()
    fun push(channel: String, blobs: List<String>) {
        val held = channels.getOrPut(channel) { mutableListOf() }
        for (blob in blobs) if (blob !in held) held.add(blob)
    }
    fun fetch(channel: String): List<String> = channels[channel] ?: emptyList()
    fun names() = channels.keys.toList()
}

fun main() {
    val relay = Relay()
    val ana = Device("ana", "u1ana", 1)
    val ben = Device("ben", "u1ben", 90)
    val anaKey = identityKeyFromSeed(ana.seed)
    val benKey = identityKeyFromSeed(ben.seed)

    println("a wallet passes facts, not callbacks")
    check("an identity key is 43 unpadded base64url characters",
          anaKey.length == 43, anaKey)
    check("a seed derived from a viewing key survives a reinstall",
          identitySeedFromViewingKey("uview1abc") ==
              identitySeedFromViewingKey("uview1abc"),
          identitySeedFromViewingKey("uview1abc"))
    check("a key of the wrong length is named, not accepted",
          billKeyProblem("AAAA") == "wrong_length" && billKeyProblem(anaKey) == null,
          "${billKeyProblem("AAAA")}")

    println("ana opens a bill and joins it")
    val create = createBillEntry(ana.facts(), "Dinner", "EUR", "equal", anaKey, ana.seed)
    ana.add(create)
    val billId = Regex("\"id\":\"([^\"]+)\"").find(create)!!.groupValues[1]
    ana.add(joinBillEntry(ana.facts(), "Ana", "u1ana", anaKey, ana.seed))

    println("ana shares it, and ben takes it from the code")
    // The bill key is the wallet's to mint and to keep; §9.4's id is public.
    val billKey = "-_" + "A".repeat(41)
    check("that key is one the cipher can use", billKeyProblem(billKey) == null, billKey)
    val payload = shareableBillPayload(ana.facts(), ana.entries, billKey)
    check("the whole bill fits in one code", payload != null, "${payload?.length} characters")
    val scanned = readScanned(payload!!)
    check("the scan names the same bill", scanned.billId == billId, "${scanned.billId}")
    check("and carries the key", scanned.billKey == billKey, "${scanned.billKey}")
    ben.entries = mergeEntries(ben.entries, scanned.entries).entries
    ben.add(joinBillEntry(ben.facts(), "Ben", "u1ben", benKey, ben.seed))

    println("the two logs move through a relay that holds only ciphertext")
    val channel = channelForBill(billId)
    check("the channel is the bill id's hash, never the id",
          channel != billId && channel.length == 64, channel.take(16) + "…")
    relay.push(channel, blobsToPush(ben.entries, billKey, ben.seed, "ben"))
    val opened = openBlobs(relay.fetch(channel), billKey)
    check("every blob opened", opened.unopenable == 0u, "unopenable=${opened.unopenable}")
    ana.entries = mergeEntries(ana.entries, opened.entries).entries

    println("ana adds an expense they share, and prices it")
    ana.add(addExpenseEntry(ana.facts(), "x1", "ana", 9000,
        """{"type":"equal","among":["ana","ben"]}""", "dinner", ana.seed))
    ana.add(setRateEntry(ana.facts(), "EUR", 300000, "a fixed feed", ana.seed))

    val folded = foldEntries(ana.facts(), ana.entries)
    check("both people are on the bill", folded.bill.participants.size == 2,
          folded.bill.participants.joinToString { it.id })
    check("nothing was set aside", folded.setAside.isEmpty(), "${folded.setAside}")
    check("both keys are bound under §10.7", folded.identities.bound.size == 2,
          "${folded.identities.bound.keys}")
    check("no identity is contested", folded.identities.contested.isEmpty(),
          "${folded.identities.contested}")

    println("ben owes half of it")
    ben.take(ana)
    val owed = obligationOf(ben.facts(), ben.entries, listOf())
    check("ben has an obligation", owed != null, owed?.request?.uri ?: "none")
    val settlement = owed!!.settlements.single()
    check("it is four and a half thousand to ana",
          settlement.to == "ana" && settlement.amount == 4500L,
          "${settlement.to} ${settlement.amount}")
    check("the request is a ZIP 321 URI naming ana's address",
          owed.request.uri!!.startsWith("zcash:u1ana"), owed.request.uri!!)
    check("nothing is withheld", owed.request.withheldMinorUnits == 0L,
          "${owed.request.withheldMinorUnits}")

    println("the wallet sends, then records what §14.3 allows")
    // The send is the wallet's. These are written only because it reached the
    // network: a transaction built and not broadcast may still land.
    val records = paymentEntriesForSend(ben.facts(), owed, "tx-ben-1", ben.seed)
    check("one record, for what the request carried", records.size == 1,
          "${records.size} record(s)")
    for (record in records) ben.add(record)

    ana.take(ben)
    val afterPayment = foldEntries(ana.facts(), ana.entries)
    check("ana sees the payment", afterPayment.bill.payments.size == 1,
          "${afterPayment.bill.payments.map { it.id }}")
    check("and it is not confirmed", afterPayment.bill.confirmedPayments.isEmpty(),
          "${afterPayment.bill.confirmedPayments}")
    val stillOwed = obligationOf(ben.facts(), ben.entries, listOf())!!
    check("so ben is asked for nothing twice", stillOwed.settlements.isEmpty(),
          "${stillOwed.settlements}")
    check("and is told what is in flight", stillOwed.awaiting.single().paid == 4500L,
          "${stillOwed.awaiting}")

    ana.add(confirmPaymentEntry(ana.facts(), "tx-ben-1", "recipientConfirmed", null, ana.seed))
    ben.take(ana)
    val settled = obligationOf(ben.facts(), ben.entries, listOf())!!
    check("once confirmed, the debt is gone",
          settled.settlements.isEmpty() && settled.awaiting.isEmpty(),
          "settlements=${settled.settlements.size} awaiting=${settled.awaiting.size}")

    println("the log reads as a history")
    val history = historyOf(ana.facts(), ana.entries)
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

    println(if (failures == 0)
        "CONSUMER RESULT: kotlin drives a whole bill with no callbacks, $failures failures"
    else "CONSUMER RESULT: $failures check(s) failed")
    if (failures != 0) kotlin.system.exitProcess(1)
}
