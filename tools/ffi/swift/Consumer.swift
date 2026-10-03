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

    check("a seed derived from a secret is the one the protocol pins",
          try identitySeedFromSecret(secret: SecretBytes(bytes: Data([1, 2, 3])))
              == "MNp3HJmtVUpkGFp2KXoi4ysYoDqKi9Sf4upQw5qvOps", "MNp3…")
    check("and a long secret whose first byte is high crosses whole",
          try identitySeedFromSecret(secret: SecretBytes(bytes: Data(repeating: 0xAB, count: 64)))
              == "zHlJI6Xb7tQXjLuwAoEwVjjVEB8jPs5DA_NhM50W1YM", "zHlJ…")
    print("ana opens a bill and joins it")
    // The bill key is the wallet's to keep, minted from the platform's own
    // entropy (SystemRandomNumberGenerator is cryptographically secure on
    // Apple platforms); §9.4's id is public.
    var entropy = SystemRandomNumberGenerator()
    let billKey = try newBillKey(entropy: RandomBytes(
        bytes: Data((0..<32).map { _ in UInt8.random(in: 0...255, using: &entropy) })))
    check("that key is one the cipher can use", billKeyProblem(key: billKey) == nil, billKey)
    // The key is minted first: the create entry commits to it (§9.4).
    let create = try createBillEntry(facts: ana.facts(), name: "Dinner",
                                     currency: "EUR", splitMode: "equal",
                                     creatorKey: anaKey, billKey: billKey, seed: ana.seed)
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
    // What ana holds, as she would report it: one key per copy (§14.5).
    let anaHolds = try ana.entries.map { try copyKey(entry: $0) }
    let behind = try deltaForPeer(facts: ben.facts(), billId: billId, entries: ben.entries,
                                  theyHave: anaHolds)
    check("ana lacks only ben's join, and it fits one code",
          behind.missing == 1 && behind.uri != nil && behind.tooBigCode == nil, "\(behind.missing)")

    print("the two logs move through a relay that holds only ciphertext")
    let invite = try inviteForBill(facts: ana.facts(), billId: billId, entries: ana.entries,
                                   billKey: billKey, name: "Dinner", expiry: 1_800_000_000)
    let link = try renderInviteLink(invite: invite, base: "https://example.org/join")
    let stranger = try newBillKey(entropy: RandomBytes(
        bytes: Data((0..<32).map { _ in UInt8.random(in: 0...255, using: &entropy) })))
    let strangerCode = try shareableBillPayload(facts: ana.facts(), billId: billId,
                                                entries: ana.entries, billKey: stranger)!
    check("a bill code carrying some other key is refused",
          readScanned(text: strangerCode).refusedCode == "invite_key_mismatch", "invite_key_mismatch")
    check("the invite reads back from an https link", readScanned(text: link).billId == billId, link)
    check("and expires by the wallet's clock",
          try !inviteExpiry(invite: invite, nowUnixSeconds: 1_799_999_999).expired
            && inviteExpiry(invite: invite, nowUnixSeconds: 1_800_000_001).expired, "1800000000")
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
    let opened = openBlobs(blobs: fetched, billId: billId, billKey: billKey)
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
    let dinner = try addExpenseEntry(
        facts: ana.facts(), billId: billId, expenseId: "x1", paidBy: ana.me, amount: 9000,
        splitJson: #"{"type":"equal","among":[""# + ana.me + #"",""# + ben.me + #""]}"#,
        description: "dinner", seed: ana.seed)
    try ana.add(dinner)
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

    print("ana plans taking ben off the bill (§10.8)")
    let bothOfThem = #"{"type":"equal","among":[""# + ana.me + #"",""# + ben.me + #""]}"#
    let onlyAna = #"{"type":"equal","among":[""# + ana.me + #""]}"#
    let unpaid = try planRemoval(facts: ana.facts(), billId: billId, entries: ana.entries,
                                 id: ben.me, me: ana.me)
    check("her expense is offered, split without him, and nothing blocks it",
          try unpaid.blockers.isEmpty && unpaid.edits.map(\.entryId) == [entryId(dinner)]
            && unpaid.edits[0].splitJson == onlyAna,
          "\(unpaid.edits.map(\.splitJson))")
    let replanned = try planRemoval(facts: ana.facts(), billId: billId, entries: ana.entries,
                                    id: ben.me, me: ana.me)
    check("and the plan still stands while the bill has not moved",
          try sameRemovalPlan(confirmed: unpaid, now: replanned) == .stands, "stands")
    let without = try splitWithout(splitJson: bothOfThem, id: ben.me)
    check("a split without him crosses as JSON", without == onlyAna, without ?? "nil")
    let byHand = #"{"type":"exact","amounts":{""# + ana.me + #"":1,""# + ben.me + #"":1}}"#
    check("and one only a person can redivide is answered with none",
          try splitWithout(splitJson: byHand, id: ben.me) == nil, "none")
    let notSplit = await refusal { _ = try splitWithout(splitJson: "{", id: ben.me) }
    check("text that is not a split is refused", notSplit != nil, notSplit?.detail ?? "nil")

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

    print("ben's review screen shows what §14.2 says it must")
    // The screen is the wallet's; these are the strings it draws, with the
    // amount and the rate written as the binding writes them.
    let zec = try renderAmount(zatoshi: owed!.request.payments[0].zatoshi)
    let screen = ["Pay Ana \(zec) ZEC", "to u1ana",
                  "at \(rateFigure(rate: owed!.rate)) EUR per ZEC, set by Ana"]
    let shown = try checkPayerReview(facts: ben.facts(), billId: billId, entries: ben.entries,
                                     obligation: owed!, visibleText: screen, reasonWords: [:],
                                     via: [:], lowerWords: "", unexplainedWords: "")
    check("a screen showing every fact passes", shown.isEmpty, "\(screen)")
    let noAddress = try checkPayerReview(
        facts: ben.facts(), billId: billId, entries: ben.entries, obligation: owed!,
        visibleText: screen.map { $0 == "to u1ana" ? "to your contact" : $0 }, reasonWords: [:],
        via: [:], lowerWords: "", unexplainedWords: "")
    check("one without the output's address is told exactly that",
          noAddress == [ReviewFinding(rule: .output, fact: "the address Ana is paid at",
                                      expected: "u1ana")],
          "\(noAddress)")

    print("the wallet writes the send down before it sends (§14.3)")
    // One string per bill, kept where it outlives the process. The send is the
    // wallet's; these say what the note becomes.
    let txid = String(repeating: "ab", count: 32)
    check("with no note, nothing blocks a send",
          pendingSendBlocks(billId: billId, note: nil) == nil, "none")
    var note: String? = try pendingSendNote(billId: billId, obligation: owed!, at: ben.now())
    check("with the note stored, a second send from this bill is blocked",
          pendingSendBlocks(billId: billId, note: note)?.damaged == false,
          pendingSendBlocks(billId: billId, note: note)?.uri ?? "nil")
    check("a refused send takes its note with it",
          pendingSendAfter(billId: billId, note: note!, how: .refused, txid: nil,
                           recorded: false) == nil, "cleared")
    check("so does one that reached the network once its records are on the bill",
          pendingSendAfter(billId: billId, note: note!, how: .reachedNetwork, txid: txid,
                           recorded: true) == nil, "cleared")
    note = pendingSendAfter(billId: billId, note: note!, how: .unresolved, txid: txid,
                            recorded: false)
    check("one built and not broadcast keeps its note, naming the transaction",
          pendingSendBlocks(billId: billId, note: note)?.txid == txid,
          pendingSendBlocks(billId: billId, note: note)?.txid ?? "nil")
    check("and the next send is still blocked",
          pendingSendBlocks(billId: billId, note: note) != nil, "blocked")
    check("a note naming its transaction is not cleared while the wallet may still send it",
          pendingSendNamedRefusal(billId: billId, note: note!, state: .waiting) == .waiting,
          "\(String(describing: pendingSendNamedRefusal(billId: billId, note: note!, state: .waiting)))")
    check("nor once it went through, and may be once it expired",
          pendingSendNamedRefusal(billId: billId, note: note!, state: .mined) == .mined
            && pendingSendNamedRefusal(billId: billId, note: note!, state: .expired) == nil,
          "\(String(describing: pendingSendNamedRefusal(billId: billId, note: note!, state: .expired)))")
    check("nobody may say it never left while the wallet is still sending",
          pendingSendUnsentRefusal(billId: billId, note: note!, stillSending: true, own: [])
            == .stillSending,
          "\(String(describing: pendingSendUnsentRefusal(billId: billId, note: note!, stillSending: true, own: [])))")
    let builtSince = pendingSendUnsentRefusal(
        billId: billId, note: note!, stillSending: false,
        own: [OwnTransaction(txid: txid, created: ben.now())])
    check("nor once the wallet built a transaction after the note was written",
          builtSince == .builtSince(txid: txid), "\(String(describing: builtSince))")
    let builtBefore = OwnTransaction(txid: String(repeating: "cd", count: 32),
                                     created: "2026-10-28T19:30:00.000Z")
    check("one built before it does not hold the note",
          pendingSendUnsentRefusal(billId: billId, note: note!, stillSending: false,
                                   own: [builtBefore]) == nil, "none")
    check("a note that does not read blocks as well",
          pendingSendBlocks(billId: billId, note: "{not json")?.damaged == true, "damaged")
    let lost = await refusal {
        _ = try pendingSendRecords(facts: ben.facts(), billId: billId, entries: ben.entries,
                                   note: "{not json", txid: txid, seed: ben.seed)
    }
    check("and nothing is recorded from it", lost != nil, lost?.detail ?? "nil")

    print("a person says the send landed; its records come from the note alone")
    let records = try pendingSendRecords(facts: ben.facts(), billId: billId,
                                         entries: ben.entries, note: note!, txid: txid,
                                         seed: ben.seed)
    check("one record, for what the request carried", records.count == 1,
          "\(records.count) record(s)")
    let paymentId = "\"\(ben.me):\(txid):\(ana.me)\""
    let succeeded = try paymentEntriesForSend(facts: ben.facts(), billId: billId,
                                              obligation: owed!, txid: txid, seed: ben.seed)
    check("under the payment id a send that succeeded records",
          records[0].contains(paymentId) && succeeded[0].contains(paymentId),
          "\(txid):\(ana.me.prefix(8))…")
    check("and it states the ZEC it sent and the rate it was priced at",
          records[0].contains("\"zatoshi\":\(owed!.request.payments[0].zatoshi)")
            && records[0].contains("\"paidAtRate\""),
          records[0])
    for record in records { try ben.add(record) }
    let again = try pendingSendRecords(facts: ben.facts(), billId: billId,
                                       entries: ben.entries, note: note!, txid: txid,
                                       seed: ben.seed)
    check("asked again, nothing is recorded twice", again.isEmpty, "\(again.count)")
    note = nil // deleted, now the records are on the bill
    check("with the note deleted, the bill can be sent from again",
          pendingSendBlocks(billId: billId, note: note) == nil, "none")

    try ana.take(ben)
    let afterPayment = try foldEntries(facts: ana.facts(), billId: billId, entries: ana.entries)
    check("ana sees the payment", afterPayment.bill.payments.count == 1,
          "\(afterPayment.bill.payments.map(\.id))")
    check("and it is not confirmed", afterPayment.bill.confirmedPayments.isEmpty,
          "\(afterPayment.bill.confirmedPayments)")
    let paid = afterPayment.bill.payments[0]
    let mine = try awaitingMyConfirmation(facts: ana.facts(), billId: billId, entries: ana.entries)
    check("ana is shown it as hers to confirm", mine.map(\.id) == [paid.id], "\(mine.map(\.id))")
    check("and ben, who paid it, is shown nothing to confirm",
          try awaitingMyConfirmation(facts: ben.facts(), billId: billId, entries: ben.entries).isEmpty,
          "none")
    func withdrawal(_ me: String, _ state: TransactionState) -> OwnPaymentWithdrawal? {
        ownPaymentWithdrawalRefusal(from: paid.from, method: paid.method,
                                    reference: paid.reference, me: me, state: state)
    }
    check("ben may not withdraw his record while its transaction is mined",
          withdrawal(ben.me, .mined) == .mined, "\(String(describing: withdrawal(ben.me, .mined)))")
    check("and may once it expired unmined", withdrawal(ben.me, .expired) == nil, "none")
    check("ana's word on ben's record is not this rule's", withdrawal(ana.me, .mined) == nil, "none")
    let paidPlan = try planRemoval(facts: ana.facts(), billId: billId, entries: ana.entries,
                                   id: ben.me, me: ana.me)
    check("once he has paid, taking ben off is blocked by the payment",
          paidPlan.blockers.map(\.block) == [.payment] && paidPlan.blockers[0].fromThem,
          "\(paidPlan.blockers.map(\.block))")
    check("so the plan ana saw before no longer stands",
          try sameRemovalPlan(confirmed: unpaid, now: paidPlan) == .changed, "changed")
    let confirmScreen = ["Ben says he paid you",
                         "\(try renderAmount(zatoshi: paid.zatoshi!)) ZEC",
                         "priced at \(rateFigure(rate: paid.paidAtRate!)) EUR a ZEC",
                         "transaction \(paid.reference!)"]
    check("ana's confirm screen shows what §14.2 says a payee must see",
          try checkPayeeReview(payment: paid, visibleText: confirmScreen,
                               absentWords: "not recorded").isEmpty, "\(confirmScreen)")
    let noReference = try checkPayeeReview(payment: paid, visibleText: Array(confirmScreen.dropLast()),
                                           absentWords: "not recorded")
    check("one without the transaction is told exactly that",
          noReference.map(\.rule) == [.payeeReference], "\(noReference)")
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
    check("as the sentence to show, the same reading has nothing to say",
          proposalProblem(uri: owed!.request.uri!,
                          outputs: [ProposedOutput(address: anaAddress, zatoshi: sent)]) == nil,
          "none")
    let problem = proposalProblem(uri: owed!.request.uri!, outputs: [])
    check("and the dropped one is a sentence, so nothing is built",
          problem?.isEmpty == false, "\(problem ?? "nil")")

    print("a swap provider's answer is read by its status first")
    check("a 2xx body is the answer",
          try swapAnswer(status: 200, body: "{\"quote\":1}") == "{\"quote\":1}", "read")
    let noRoute = await refusal { _ = try swapAnswer(status: 400, body: "{\"message\":\"no route\"}") }
    check("a 4xx is refused with the provider's own words, and waiting will not help",
          noRoute?.detail.contains("no route") == true && noRoute?.transient == false,
          "\(noRoute?.detail ?? "nil")")
    let upstream = await refusal { _ = try swapAnswer(status: 503, body: "upstream down") }
    check("a 5xx is refused as one to try again", upstream?.transient == true,
          "\(upstream?.detail ?? "nil")")

    print("ana's wallet saw the transaction arrive")
    let arrivals = try arrivalsOf(facts: ana.facts(),
                                  bills: [HeldBill(billId: billId, entries: ana.entries)],
                                  received: [IncomingTransaction(txid: txid, zatoshi: sent)])
    check("the payment is proposed for confirmation",
          arrivals.arrived.count == 1
            && arrivals.arrived[0].payment.id == afterPayment.bill.payments[0].id,
          "arrived=\(arrivals.arrived.count) short=\(arrivals.short.count)")
    check("and nothing is held back as disputed, underpriced or unbound",
          arrivals.disputed.isEmpty && arrivals.underpriced.isEmpty && arrivals.unbound.isEmpty,
          "disputed=\(arrivals.disputed.count) underpriced=\(arrivals.underpriced.count)")
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

    print("the fallback price sources, asked and read the same way")
    check("binance is asked for the ZECUSDC ticker",
          binancePriceRequest(origin: "https://data-api.binance.vision")
            == "https://data-api.binance.vision/api/v3/ticker/price?symbol=ZECUSDC",
          binancePriceRequest(origin: "https://data-api.binance.vision"))
    let usd = try zecPriceFromBinance(body: #"{"symbol":"ZECUSDC","price":"1390.54000000"}"#, currency: "USD")
    let notUsd = try zecPriceFromBinance(body: #"{"symbol":"ZECUSDC","price":"1390.54"}"#, currency: "EUR")
    check("and prices USD alone", usd == 139054 && notUsd == nil, "\(usd ?? -1)")
    let kes = try zecPriceFromCoinbase(
        body: #"{"data":{"currency":"ZEC","rates":{"KES":"180159.79"}}}"#, currency: "KES")
    check("coinbase prices the rest from one answer",
          kes == 18015979 && coinbasePriceRequest(origin: "https://api.coinbase.com")
            == "https://api.coinbase.com/v2/exchange-rates?currency=ZEC", "\(kes ?? -1)")
    check("two sources within the tolerance agree on the higher",
          agreedPrice(first: 138_819, second: 138_905, toleranceBp: 200) == 138_905,
          "\(agreedPrice(first: 138_819, second: 138_905, toleranceBp: 200) ?? -1)")
    check("and two that are not give no price",
          agreedPrice(first: 138_819, second: 152_701, toleranceBp: 200) == nil, "none")

    print("a payout a person declares goes first, and replaces its own kind (§9.1)")
    let swapUsdc = Payout(kind: "swap", address: "0xa", asset: "USDC", chain: nil)
    let ranked = rankedPayouts(
        who: Participant(id: "p", name: "P", payTo: "zOld", identityKey: nil, payouts: []),
        first: swapUsdc)
    check("a pay-to-only record keeps its ZEC behind the new swap",
          ranked == [swapUsdc, Payout(kind: "zec", address: "zOld", asset: nil, chain: nil)],
          "\(ranked)")
    let declared = [Payout(kind: "cash", address: nil, asset: nil, chain: nil),
                    Payout(kind: "zec", address: "zA", asset: nil, chain: nil),
                    Payout(kind: "swap", address: "0xb", asset: "USDT", chain: nil)]
    let replaced = rankedPayouts(
        who: Participant(id: "p", name: "P", payTo: nil, identityKey: nil, payouts: declared),
        first: Payout(kind: "zec", address: "zB", asset: nil, chain: nil))
    check("a new ZEC payout replaces the old one, the rest keep their order",
          replaced.map(\.kind) == ["zec", "cash", "swap"] && replaced[0].address == "zB",
          "\(replaced)")

    print("a swap deposit is checked against the bill before it is sent (§15.7)")
    let onBase = Payout(kind: "swap", address: "0xbenbase", asset: "USDC", chain: "base")
    let onArb = Payout(kind: "swap", address: "0xbenarb", asset: "USDC", chain: "arb")
    let taxiCreate = try createBillEntry(facts: ana.facts(), name: "Taxi", currency: "EUR",
                                         splitMode: "equal", creatorKey: anaKey, billKey: nil,
                                         seed: ana.seed)
    let taxiId = (try JSONSerialization.jsonObject(with: Data(taxiCreate.utf8))
        as! [String: Any])["id"] as! String
    var taxi = [
        taxiCreate,
        try joinBillEntry(facts: ana.facts(), billId: taxiId, name: "Ana", payTo: "u1ana",
                          identityKey: anaKey, payouts: [], seed: ana.seed),
        try joinBillEntry(facts: ben.facts(), billId: taxiId, name: "Ben", payTo: nil,
                          identityKey: benKey, payouts: [onBase, onArb], seed: ben.seed),
        try addExpenseEntry(facts: ben.facts(), billId: taxiId, expenseId: "t1", paidBy: ben.me,
                            amount: 8000, splitJson: bothOfThem, description: nil, seed: ben.seed),
        try setRateEntry(facts: ana.facts(), billId: taxiId, currency: "EUR",
                         minorUnitsPerZec: 51234, source: nil, seed: ana.seed),
    ]
    func quote(_ recipient: String, _ chain: String) -> SwapQuote {
        SwapQuote(depositAddress: "t1deposit", recipient: recipient, depositMemo: nil,
                  amountInZatoshi: 7_807_316, amountOut: "39990000", minAmountOut: nil,
                  asset: TradableAsset(assetId: "nep141:\(chain)-usdc", symbol: "USDC",
                                       chain: chain, decimals: 6),
                  deadline: "2026-10-29T23:00:00.000Z", reference: "intent-1")
    }
    func swapRefusal(_ q: SwapQuote, _ chosen: Payout?) throws -> SwapSendRefusal? {
        try swapSendRefusal(facts: ana.facts(), billId: taxiId, entries: taxi, quote: q,
                            to: ben.me, amountMinorUnits: 4000, chosen: chosen)
    }
    check("a deposit to ben's first payout, for what ana owes, may go",
          try swapRefusal(quote("0xbenbase", "base"), nil) == nil, "none")
    check("one asked for his second payout may go too",
          try swapRefusal(quote("0xbenarb", "arb"), onArb) == nil, "none")
    let wrongRecipient = try swapRefusal(quote("0xbenbase", "base"), onArb)
    check("one whose recipient is not the payout chosen is refused",
          wrongRecipient == .recipientChanged, "\(String(describing: wrongRecipient))")
    var noChain = onArb
    noChain.chain = nil
    check("the payout chosen is found by type, address, asset and chain",
          declaredPayoutIndex(payouts: [onBase, onArb], payout: onArb) == 1
            && declaredPayoutIndex(payouts: [onBase, onArb], payout: noChain) == nil,
          "\(String(describing: declaredPayoutIndex(payouts: [onBase, onArb], payout: onArb)))")
    let swapRecord = try recordPaymentEntry(
        facts: ana.facts(), billId: taxiId,
        payment: PaymentDraft(paymentId: "intent-1", to: ben.me, amount: 4000, method: "swap",
                              reference: "intent-1", zatoshi: 7_807_316, paidAtRate: nil,
                              note: nil),
        seed: ana.seed)
    taxi.append(swapRecord)
    let held = try swapRefusal(quote("0xbenbase", "base"), nil)
    check("once a payment covers the debt, a second deposit is held for ben to confirm",
          held == .held(paidTo: [ben.me]), "\(String(describing: held))")
    let failed = try failedSwapWithdrawals(facts: ana.facts(), billId: taxiId, entries: taxi,
                                           reference: "intent-1")
    check("a swap that failed names ana's record of it to withdraw",
          try failed == [entryId(swapRecord)], "\(failed)")
    check("and nothing to ben, who did not write it",
          try failedSwapWithdrawals(facts: ben.facts(), billId: taxiId, entries: taxi,
                                    reference: "intent-1").isEmpty, "none")

    print("a swap's deposit and its record come from the binding (§15.7)")
    let taxiRate = ExchangeRate(currency: "EUR", minorUnitsPerZec: 51234,
                           at: "2026-10-28T19:30:00.000Z", source: nil)
    check("a debt is sized in zatoshi at the bill's rate, rounding up",
          try fiatToZatoshi(amountMinorUnits: 4000, rate: taxiRate) == 7_807_316,
          "\(try fiatToZatoshi(amountMinorUnits: 4000, rate: taxiRate))")
    let deposit = try swapDeposit(billId: taxiId, quote: quote("0xbenbase", "base"), to: ben.me,
                                  amountMinorUnits: 4000, rate: taxiRate, at: ana.now())
    check("a deposit is one request to the quote's address for its zatoshi",
          deposit.uri.hasPrefix("zcash:t1deposit?amount=0.07807316"), deposit.uri)
    check("and its note carries the swap",
          deposit.note.contains(#""reference":"intent-1""#), deposit.note)
    var withMemo = quote("0xbenbase", "base")
    withMemo.depositMemo = "123"
    let needsMemo = await refusal {
        _ = try swapDeposit(billId: taxiId, quote: withMemo, to: ben.me, amountMinorUnits: 4000,
                            rate: taxiRate, at: ana.now())
    }
    check("one whose deposit needs a memo is refused", needsMemo != nil, needsMemo?.detail ?? "nil")
    var withFloor = quote("0xbenbase", "base")
    withFloor.minAmountOut = "39500000"
    let swapEntry = try swapPaymentEntry(facts: ana.facts(), billId: taxiId, quote: withFloor,
                                         to: ben.me, amountMinorUnits: 4000, rate: taxiRate,
                                         seed: ana.seed)
    check("its record names the asset, the chain and the floor",
          swapEntry.contains(#""note":"at least 39.5 USDC on base""#), swapEntry)
    check("base units read as whole tokens",
          formatBaseUnits(baseUnits: "39990000", decimals: 6) == "39.99"
            && formatBaseUnits(baseUnits: "x", decimals: 6) == nil, "39.99")

    print("what every wallet derives, reads and asks before writing")
    let mnemonic = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
    let derived = try identitySeedFromMnemonic(mnemonic: mnemonic, passphrase: "", accountIndex: 1)
    check("a mnemonic derives the seed every wallet derives (§15.1)",
          derived == "jWQijb3QECEvHgOUb4_W7UPiUujN7D-7Nq0jYg5mrKo", derived)
    let noMnemonic = await refusal {
        _ = try identitySeedFromMnemonic(mnemonic: "", passphrase: "", accountIndex: 0)
    }
    check("an empty mnemonic is refused", noMnemonic != nil, noMnemonic?.detail ?? "nil")
    let digest = String(repeating: "00", count: 31) + "ab"
    check("a txid in digest order is reversed (§14.7)",
          txidInSendOrder(digestOrderHex: digest) == "ab" + String(repeating: "00", count: 31)
            && txidInSendOrder(digestOrderHex: "abc") == nil,
          txidInSendOrder(digestOrderHex: digest) ?? "nil")
    check("a typed figure is read in integers (§2.1)",
          parseAmountIn(text: "12.34", currency: "EUR") == 1234
            && parseAmountIn(text: "1,000", currency: "KWD") == nil
            && parseMinorUnits(text: "12.5", exponent: 2) == 1250,
          "\(String(describing: parseAmountIn(text: "12.34", currency: "EUR")))")
    let gold = await refusal {
        _ = try createBillEntry(facts: ana.facts(), name: "Gold", currency: "XAU",
                                splitMode: "equal", creatorKey: anaKey, billKey: nil,
                                seed: ana.seed)
    }
    check("no bill is opened in a currency with no minor unit", gold != nil, gold?.detail ?? "nil")
    let taxiFolded = try foldEntries(facts: ana.facts(), billId: taxiId, entries: taxi)
    check("the creator is the one the fold names", taxiFolded.creatorId == ana.me,
          taxiFolded.creatorId)
    let benOff = try planRemoval(facts: ana.facts(), billId: taxiId, entries: taxi, id: ben.me,
                                 me: ana.me)
    check("taking ben off lists his join to withdraw", benOff.joins.count == 1, "\(benOff.joins)")
    let off = try voidEntryFor(facts: ana.facts(), billId: taxiId, targetId: benOff.joins[0],
                               seed: ana.seed)
    let offRefused = try entryRefusal(facts: ana.facts(), billId: taxiId, entries: taxi, entry: off)
    check("which is refused before it is written while the bill names him",
          offRefused == "participant_still_named", offRefused ?? "nil")
    let own = try voidEntryFor(facts: ana.facts(), billId: taxiId, targetId: try entryId(swapRecord),
                               seed: ana.seed)
    check("and ana withdrawing her own record is not",
          try entryRefusal(facts: ana.facts(), billId: taxiId, entries: taxi, entry: own) == nil,
          "none")

    print("and the rest of what every wallet needs from the protocol")
    let fallback = payoutFallback(cannotPay: ["not on base", nil])
    check("a first payout this wallet cannot pay is passed over for the next it can (§14.8)",
          fallback == PayoutFallback(index: 1, passedOver: "not on base")
            && payoutFallback(cannotPay: [nil, "x"]) == nil,
          "\(String(describing: fallback))")
    let wrapped = TradableAsset(assetId: "nep141:near-zec", symbol: "ZEC", chain: "near", decimals: 8)
    let native = TradableAsset(assetId: "nep141:zec.omft.near", symbol: "ZEC", chain: "zec", decimals: 8)
    check("native ZEC is the one on its own chain",
          zecAssetIn(assets: [wrapped, native]) == "nep141:zec.omft.near"
            && zecAssetIn(assets: [wrapped]) == nil, "nep141:zec.omft.near")
    check("a rate 5% from the live price is told by how much",
          ratePercentOff(rate: 105, live: 100) == 5 && ratePercentOff(rate: 1, live: 0) == nil, "5")
    check("names a reader cannot tell apart fold alike",
          nameSkeleton(name: "\u{410}na") == nameSkeleton(name: "ana"), nameSkeleton(name: "\u{410}na"))
    let names = try displayNames(facts: ana.facts(), billId: taxiId, entries: taxi)
    check("every participant has a display name", Set(names.keys) == [ana.me, ben.me], "\(names)")
    let corrected = try amendExpenseEntry(facts: ben.facts(), billId: taxiId, entries: taxi,
                                          expenseId: "\(ben.me):t1", paidBy: nil, amount: 9000,
                                          splitJson: nil, description: "taxi home", seed: ben.seed)
    check("an expense is corrected from what the bill applies now",
          try entryRefusal(facts: ben.facts(), billId: taxiId, entries: taxi, entry: corrected) == nil,
          "none")
    var unknownExpense: String?
    do {
        _ = try amendExpenseEntry(facts: ben.facts(), billId: taxiId, entries: taxi,
                                  expenseId: "\(ben.me):t9", paidBy: nil, amount: 1, splitJson: nil,
                                  description: nil, seed: ben.seed)
    } catch SplitzError.Protocol(let code, _) { unknownExpense = code }
    check("and one the bill does not apply is refused", unknownExpense == "unknown_entry",
          unknownExpense ?? "nil")
    let concerns = try concernsBeforeConfirming(facts: ben.facts(), billId: taxiId, entries: taxi,
                                                paymentId: "\(ana.me):intent-1", live: nil)
    check("ana paying by a rate she set is a concern before ben confirms it",
          concerns == [.rateSetByPayer], "\(concerns)")
    let toAna = try recordPaymentEntry(
        facts: ben.facts(), billId: taxiId,
        payment: PaymentDraft(paymentId: "p-memo", to: ana.me, amount: 1000, method: "shieldedZec",
                              reference: String(repeating: "ab", count: 32), zatoshi: 2_000_000,
                              paidAtRate: nil, note: nil),
        seed: ben.seed)
    let memos = try memoTxids(facts: ana.facts(), bills: [HeldBill(billId: taxiId, entries: taxi + [toAna])])
    check("memos are read for the transactions a record to this device names",
          try memos == [String(repeating: "ab", count: 32)]
            && memoTxids(facts: ana.facts(), bills: [HeldBill(billId: taxiId, entries: taxi)]).isEmpty,
          "\(memos)")
}

/// A signed entry's own id: the top-level one, not its payload's.
func entryId(_ entry: String) throws -> String {
    (try JSONSerialization.jsonObject(with: Data(entry.utf8)) as! [String: Any])["id"] as! String
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
