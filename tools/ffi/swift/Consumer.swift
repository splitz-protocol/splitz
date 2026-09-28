/// A Swift wallet over the generated binding, driving one whole bill.
///
/// It holds its own log, its own clock and its own randomness, and hands the
/// library the facts it owns. Nothing calls back into Swift: SPEC.md §15 names
/// seven interfaces a wallet implements, and across a foreign boundary those
/// would be seven sets of callbacks.
import Foundation
import SplitzFFI

var failures = 0

func check(_ name: String, _ ok: Bool, _ saw: String) {
    if ok { print("  PASS  \(name) — \(saw)") }
    else { failures += 1; print("  FAIL  \(name) — \(saw)") }
}

/// One device: its log, its clock, its randomness, and the key it signs with.
final class Device {
    private let seedByte: Int
    private var minute = 0
    var entries: [String] = []
    /// The key this account publishes, and the participant id it derives
    /// (§10.7): a wallet that publishes a key writes every entry under the id
    /// that key derives, or the key binds nothing.
    let key: String
    let me: String

    init(_ seedByte: Int) throws {
        self.seedByte = seedByte
        let seed = Device.seed(seedByte)
        key = try identityKeyFromSeed(seed: seed)
        me = try participantIdForKey(key: key)
    }

    /// A §9.3 instant: UTC, exactly three fractional digits, fixed width, so a
    /// log sorts as text on every device (§10.2).
    func now() -> String {
        minute += 1
        let total = 19 * 60 + 30 + minute
        return String(format: "2026-10-28T%02d:%02d:00.000Z", total / 60, total % 60)
    }

    /// §9.4 derives a bill's id from this. A shipped wallet uses the platform's
    /// own entropy: two bills opened in one second by one person are one bill
    /// if it can be guessed.
    func nonce() -> Data {
        Data((0..<16).map { UInt8(truncatingIfNeeded: seedByte + minute + $0) })
    }

    func facts() -> HostFacts {
        HostFacts(me: me, now: now(), nonce: nonce())
    }

    /// The Ed25519 seed this account signs with, as §9.4 writes a key. A
    /// shipped wallet keeps this in the platform keychain.
    var seed: String { Device.seed(seedByte) }

    static func seed(_ first: Int) -> String {
        Data((0..<32).map { UInt8(truncatingIfNeeded: first + $0) })
            .base64EncodedString()
            .replacingOccurrences(of: "+", with: "-")
            .replacingOccurrences(of: "/", with: "_")
            .replacingOccurrences(of: "=", with: "")
    }

    func add(_ entry: String) throws {
        entries = try mergeEntries(held: entries, incoming: [entry]).entries
    }

    func take(_ other: Device) throws {
        entries = try mergeEntries(held: entries, incoming: other.entries).entries
    }
}

func run() throws {
    let ana = try Device(1)
    let ben = try Device(90)
    let anaKey = ana.key
    let benKey = ben.key

    print("a wallet passes facts, not callbacks")
    check("an identity key is 43 unpadded base64url characters",
          anaKey.count == 43, anaKey)
    check("a key of the wrong length is named, not accepted",
          billKeyProblem(key: "AAAA") == "wrong_length"
            && billKeyProblem(key: anaKey) == nil,
          billKeyProblem(key: "AAAA") ?? "nil")

    print("ana opens a bill and joins it")
    let create = try createBillEntry(facts: ana.facts(), name: "Dinner",
                                     currency: "EUR", splitMode: "equal",
                                     creatorKey: anaKey, seed: ana.seed)
    try ana.add(create)
    let billId = (try JSONSerialization.jsonObject(with: Data(create.utf8))
        as! [String: Any])["id"] as! String
    try ana.add(try joinBillEntry(facts: ana.facts(), billId: billId, name: "Ana",
                                  payTo: "u1ana", identityKey: anaKey,
                                  payouts: [], seed: ana.seed))
    try ben.add(try joinBillEntry(facts: ben.facts(), billId: billId, name: "Ben",
                                  payTo: "u1ben", identityKey: benKey,
                                  payouts: [], seed: ben.seed))
    try ben.take(ana)
    try ana.take(ben)

    print("ana adds an expense they share, and prices it")
    try ana.add(try addExpenseEntry(
        facts: ana.facts(), billId: billId, expenseId: "x1", paidBy: ana.me, amount: 9000,
        splitJson: #"{"type":"equal","among":[""# + ana.me + #"",""# + ben.me + #""]}"#,
        description: "dinner", seed: ana.seed))
    try ana.add(try setRateEntry(facts: ana.facts(), billId: billId, currency: "EUR",
                                 minorUnitsPerZec: 300000,
                                 source: "a fixed feed", seed: ana.seed))

    let folded = try foldEntries(facts: ana.facts(), billId: billId, entries: ana.entries)
    check("both people are on the bill", folded.bill.participants.count == 2,
          folded.bill.participants.map(\.name).joined(separator: ", "))
    check("nothing was set aside", folded.setAside.isEmpty, "\(folded.setAside)")
    check("both keys are bound under §10.7",
          folded.identities.bound == [ana.me: anaKey, ben.me: benKey],
          "\(folded.identities.bound.keys)")

    print("ben owes half of it")
    try ben.take(ana)
    let owed = try obligationOf(facts: ben.facts(), billId: billId, entries: ben.entries)
    check("ben has an obligation", owed != nil, owed?.request.uri ?? "none")
    let settlement = owed!.settlements[0]
    check("it is four and a half thousand to ana",
          settlement.to == ana.me && settlement.amount == 4500,
          "\(settlement.to) \(settlement.amount)")
    check("the request is a ZIP 321 URI naming ana's address",
          owed!.request.uri!.hasPrefix("zcash:u1ana"), owed!.request.uri!)

    print("the wallet sends, then records what §14.3 allows")
    let records = try paymentEntriesForSend(facts: ben.facts(), billId: billId,
                                            obligation: owed!,
                                            txid: "tx-ben-1", seed: ben.seed)
    check("one record, for what the request carried", records.count == 1,
          "\(records.count) record(s)")
    check("and it states the ZEC it sent and the rate it was priced at",
          records[0].contains("\"zatoshi\":\(owed!.request.payments[0].zatoshi)")
            && records[0].contains("\"paidAtRate\""),
          records[0])
    for record in records { try ben.add(record) }

    try ana.take(ben)
    let afterPayment = try foldEntries(facts: ana.facts(), billId: billId, entries: ana.entries)
    check("ana sees the payment", afterPayment.bill.payments.count == 1,
          "\(afterPayment.bill.payments.map(\.id))")
    check("and it is not confirmed", afterPayment.bill.confirmedPayments.isEmpty,
          "\(afterPayment.bill.confirmedPayments)")

    let toConfirm = afterPayment.bill.payments[0].id
    try ana.add(try confirmPaymentEntry(facts: ana.facts(), billId: billId,
                                        paymentId: toConfirm,
                                        method: "recipientConfirmed",
                                        reference: nil,
                                        record: afterPayment.paymentDigests[toConfirm]!,
                                        seed: ana.seed))
    try ben.take(ana)
    let settled = try obligationOf(facts: ben.facts(), billId: billId, entries: ben.entries)!
    check("once confirmed, the debt is gone",
          settled.settlements.isEmpty && settled.awaiting.isEmpty,
          "settlements=\(settled.settlements.count) awaiting=\(settled.awaiting.count)")
}

do {
    try run()
} catch {
    failures += 1
    print("  FAIL  the run threw — \(error)")
}
print(failures == 0 ? "DOC RESULT: swift" : "\(failures) failure(s)")
exit(failures == 0 ? 0 : 1)
