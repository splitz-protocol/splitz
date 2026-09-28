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
import com.sun.jna.NativeLibrary
import org.junit.Assert.assertEquals
import org.junit.Test
import org.junit.runner.RunWith
import uniffi.splitz_ffi.BillEventKind
import uniffi.splitz_ffi.HostFacts
import uniffi.splitz_ffi.addExpenseEntry
import uniffi.splitz_ffi.billKeyProblem
import uniffi.splitz_ffi.blobsToPush
import uniffi.splitz_ffi.channelForBill
import uniffi.splitz_ffi.confirmPaymentEntry
import uniffi.splitz_ffi.createBillEntry
import uniffi.splitz_ffi.foldEntries
import uniffi.splitz_ffi.historyOf
import uniffi.splitz_ffi.identityKeyFromSeed
import uniffi.splitz_ffi.joinBillEntry
import uniffi.splitz_ffi.mergeEntries
import uniffi.splitz_ffi.obligationOf
import uniffi.splitz_ffi.openBlobs
import uniffi.splitz_ffi.participantIdForKey
import uniffi.splitz_ffi.paymentEntriesForSend
import uniffi.splitz_ffi.readScanned
import uniffi.splitz_ffi.setRateEntry
import uniffi.splitz_ffi.shareableBillPayload

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

/// A relay holding ciphertext, shared by both devices. It holds no key.
private class Relay {
    private val channels = LinkedHashMap<String, MutableList<String>>()
    fun push(channel: String, blobs: List<String>) {
        val held = channels.getOrPut(channel) { mutableListOf() }
        for (blob in blobs) if (blob !in held) held.add(blob)
    }
    fun fetch(channel: String): List<String> = channels[channel] ?: emptyList()
}

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

        val relay = Relay()
        val ana = Device(1)
        val ben = Device(90)
        val anaKey = ana.key
        val benKey = ben.key
        check("an identity key is 43 unpadded base64url characters",
              anaKey.length == 43, anaKey)

        println("ana opens a bill and joins it")
        val create = createBillEntry(ana.facts(), "Dinner", "EUR", "equal", anaKey, ana.seed)
        ana.add(create)
        val billId = Regex("\"id\":\"([^\"]+)\"").find(create)!!.groupValues[1]
        ana.add(joinBillEntry(ana.facts(), billId, "Ana", "u1ana", anaKey, listOf(), ana.seed))

        println("ana shares it, and ben takes it from the code")
        val billKey = "-_" + "A".repeat(41)
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
        relay.push(channel, blobsToPush(ben.entries, billKey))
        val opened = openBlobs(relay.fetch(channel), billKey)
        check("every blob opened", opened.unopenable == 0u, "unopenable=${opened.unopenable}")
        ana.entries = mergeEntries(ana.entries, opened.entries).entries

        println("ana adds an expense they share, and prices it")
        ana.add(addExpenseEntry(ana.facts(), billId, "x1", ana.me, 9000,
            """{"type":"equal","among":["${ana.me}","${ben.me}"]}""", "dinner", ana.seed))
        ana.add(setRateEntry(ana.facts(), billId, "EUR", 300000, "a fixed feed", ana.seed))

        val folded = foldEntries(ana.facts(), billId, ana.entries)
        check("both people are on the bill", folded.bill.participants.size == 2,
              folded.bill.participants.joinToString { it.id })
        check("nothing was set aside", folded.setAside.isEmpty(), "${folded.setAside}")

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

        println("AAR DEVICE RESULT: $failures failure(s)")
        assertEquals("checks that failed", 0, failures)
    }
}
