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

/// What `body` threw as a host refusal, or nil when it threw nothing.
func refusal(_ body: () async throws -> Void) async -> (detail: String, transient: Bool)? {
    do {
        try await body()
        return nil
    } catch SplitzError.Host(let detail, let transient) {
        return (detail, transient)
    } catch {
        return ("threw something else: \(error)", false)
    }
}

/// `origin` is a running relay; `downOrigin` is one nothing answers.
func run(origin: String, downOrigin: String) async throws {
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

    print("the two logs move through a relay that holds only ciphertext")
    // The bill key is the wallet's to mint and to keep; §9.4's id is public.
    let billKey = "-_" + String(repeating: "A", count: 41)
    let channel = channelForBill(billId: billId)
    check("the channel is the bill id's hash, never the id",
          channel != billId && channel.count == 64, String(channel.prefix(16)) + "…")
    // Two clients, as two devices hold them: what one pushes the other fetches.
    let benRelay = try SplitzRelay(origin: origin)
    let anaRelay = try SplitzRelay(origin: origin)
    // Pushed as they are held: each was signed when it was written.
    let pushed = try blobsToPush(entries: ben.entries, billKey: billKey)
    try await benRelay.push(channel: channel, blobs: pushed)
    try await benRelay.push(channel: channel, blobs: pushed)
    let fetched = try await anaRelay.fetch(channel: channel)
    check("another client fetches every blob pushed, once, though it was pushed twice",
          fetched.sorted() == pushed.sorted(), "\(fetched.count) of \(pushed.count)")
    let opened = openBlobs(blobs: fetched, billKey: billKey)
    check("every blob opened", opened.unopenable == 0, "unopenable=\(opened.unopenable)")
    // Merged by entry id: what a blob opens to is the entry, not necessarily
    // the same text, so a round trip adds no entry to the log it came from.
    let roundTrip = try mergeEntries(held: ben.entries, incoming: opened.entries)
    check("to the entries ben holds",
          opened.entries.count == ben.entries.count
            && roundTrip.entries.count == ben.entries.count && roundTrip.refused.isEmpty,
          "\(opened.entries.count) opened, \(roundTrip.entries.count) after merging into \(ben.entries.count)")
    ana.entries = try mergeEntries(held: ana.entries, incoming: opened.entries).entries

    print("a relay that fails says whether retrying could succeed")
    let down = await refusal { _ = try await SplitzRelay(origin: downOrigin).fetch(channel: channel) }
    check("a relay that is down raises, transient", down?.transient == true, down?.detail ?? "nil")
    let notChannel = await refusal { try await anaRelay.push(channel: "not-a-channel", blobs: ["x"]) }
    check("a 4xx the relay answers raises, transient, as every client classifies it",
          notChannel?.transient == true && notChannel?.detail.contains("refused") == true,
          notChannel?.detail ?? "nil")
    let oversize = await refusal {
        try await anaRelay.push(channel: channel, blobs: [String(repeating: "x", count: 64 * 1024 + 1)])
    }
    check("a blob over the cap raises before it is sent, not transient",
          oversize?.transient == false, oversize?.detail ?? "nil")
    let queried = await refusal { _ = try SplitzRelay(origin: origin + "?t=1") }
    check("an origin carrying a query raises, not transient",
          queried?.transient == false, queried?.detail ?? "nil")

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
    let totals = try totalsOf(facts: ben.facts(),
                              bills: [HeldBill(billId: billId, entries: ben.entries)])
    check("across bills, ben still owes ana, with the payment on its way",
          totals.standings.count == 1 && totals.standings[0].withId == ana.me
            && totals.standings[0].owedByMe == 4500 && totals.standings[0].sentAwaiting == 4500
            && totals.uncounted.isEmpty,
          "\(totals.standings)")

    print("before signing, the wallet holds what it read against the request")
    let anaAddress = folded.bill.participants.first { $0.id == ana.me }!.payTo!
    let sent = owed!.request.payments[0].zatoshi
    let same = try checkProposal(uri: owed!.request.uri!,
                                 outputs: [ProposedOutput(address: anaAddress, zatoshi: sent)])
    check("what the request asks is what would be signed",
          same.missing.isEmpty && same.unexpected.isEmpty, "\(same)")
    let dropped = try checkProposal(uri: owed!.request.uri!, outputs: [])
    check("a reader that dropped the payment is caught",
          dropped.missing.map(\.address) == [anaAddress], "\(dropped)")

    print("ana's wallet saw the transaction arrive")
    let arrivals = try arrivalsOf(facts: ana.facts(),
                                  bills: [HeldBill(billId: billId, entries: ana.entries)],
                                  received: [IncomingTransaction(txid: "tx-ben-1", zatoshi: sent)])
    check("the payment is proposed for confirmation",
          arrivals.arrived.count == 1
            && arrivals.arrived[0].payment.id == afterPayment.bill.payments[0].id,
          "arrived=\(arrivals.arrived.count) short=\(arrivals.short.count)")
    let arrival = arrivals.arrived[0]
    try ana.add(try confirmPaymentEntry(facts: ana.facts(), billId: billId,
                                        paymentId: arrival.payment.id,
                                        method: "walletReceived",
                                        reference: arrival.txid,
                                        record: arrival.record,
                                        seed: ana.seed))
    try ben.take(ana)
    let settled = try obligationOf(facts: ben.facts(), billId: billId, entries: ben.entries)!
    check("once confirmed, the debt is gone",
          settled.settlements.isEmpty && settled.awaiting.isEmpty,
          "settlements=\(settled.settlements.count) awaiting=\(settled.awaiting.count)")

    print("a code has a sentence a person can read")
    check("a known code reads as a sentence, an unknown one as nothing",
          describeCode(code: "self_payment") == "You can't pay yourself."
            && describeCode(code: "no_such_code") == nil,
          "\(describeCode(code: "self_payment") ?? "nil")")

    print("an address is decoded before it is paid or published")
    let unified = try parseAddress(address: "u1ay3aawlldjrmxqnjf5medr5ma6p3acnet464ht8lmwplq5cd3ugytcmlf96rrmtgwldc75x94qn4n8pgen36y8tywlq6yjk7lkf3fa8wzjrav8z2xpxqnrnmjxh8tmz6jhfh425t7f3vy6p4pd3zmqayq49efl2c4xydc0gszg660q9p")
    check("a unified address, on main, that takes a memo",
          unified.network == "main" && unified.kind == "unified"
            && unified.receivers == [2, 3] && unified.canReceiveMemo, "\(unified)")
    let transparent = try parseAddress(address: "t1Hsc1LR8yKnbbe3twRp88p6vFfC5t7DLbs")
    check("a transparent one takes none",
          transparent.kind == "p2pkh" && !transparent.canReceiveMemo, "\(transparent)")
    var refusedAddress: String? = nil
    do { _ = try parseAddress(address: "u1ana") } catch SplitzError.Protocol(let code, _) { refusedAddress = code }
    check("anything else is refused by its code", refusedAddress == "address_invalid",
          refusedAddress ?? "nil")

    print("a price, asked and read without a callback")
    let priceOrigin = "https://api.coingecko.com/api/v3"
    check("the request names one currency",
          zecPriceRequest(origin: priceOrigin, currency: "EUR")
            == "https://api.coingecko.com/api/v3/simple/price?ids=zcash&vs_currencies=eur",
          zecPriceRequest(origin: priceOrigin, currency: "EUR") ?? "nil")
    check("a code with no exponent is not asked for",
          zecPriceRequest(origin: priceOrigin, currency: "XAU") == nil, "XAU")
    let eur = try zecPriceFromResponse(body: #"{"zcash":{"eur":1222.41}}"#, currency: "EUR")
    check("the answer reads as minor units, exactly", eur == 122241, "\(eur ?? -1)")
}

let arguments = CommandLine.arguments
guard arguments.count == 3 else {
    print("usage: Consumer <relay origin> <origin nothing answers>")
    exit(2)
}
do {
    try await run(origin: arguments[1], downOrigin: arguments[2])
} catch {
    failures += 1
    print("  FAIL  the run threw — \(error)")
}
print(failures == 0 ? "DOC RESULT: swift" : "\(failures) failure(s)")
exit(failures == 0 ? 0 : 1)
