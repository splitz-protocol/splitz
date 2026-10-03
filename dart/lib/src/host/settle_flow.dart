/// Paying what this device owes, and recording that it did.
///
/// The protocol renders the payment request; the wallet sends it. What sits
/// between them is the part that has to survive a person walking away
/// mid-send, and that is what this file is for.
library;

import 'package:splitz_core/splitz_core.dart' as splitz;

import 'bill_log.dart';
import 'entries.dart';
import 'host.dart';

/// What this device owes, and the request that carries it.
///
/// Wraps the protocol's own [splitz.Obligation] rather than restating it: the
/// carried and withheld totals, the payments and the unpayable recipients are
/// its answers, and a second copy of them here would be a second thing to keep
/// right.
class PayerObligation {
  const PayerObligation({
    required this.settlements,
    required this.request,
    required this.rate,
    this.awaiting = const <splitz.Awaiting>[],
  });

  /// The settlements this device is the payer of and the request carries.
  final List<splitz.Settlement> settlements;

  /// Debts this device has already paid, whose payee has not yet confirmed.
  ///
  /// Left out of [settlements] and out of the request. A payment record does
  /// not discharge a debt (section 10.5) — the payee says when the money
  /// arrived — so these are still in the plan, and asking for them again
  /// would send the same money a second time.
  ///
  /// A payment that never landed sits here too, and no retry goes through
  /// this path until its record is voided.
  final List<splitz.Awaiting> awaiting;

  /// The protocol's answer: the request, what it carries, and who it could
  /// not carry with the reason for each.
  final splitz.Obligation request;

  /// The rate [request] was priced at: the bill's, when this was read.
  final splitz.ExchangeRate rate;

  String? get uri => request.uri;
  List<splitz.Unpayable> get unpayable => request.unpayable;

  /// True when every debt this device owes is in the request. A request that
  /// covers less must say so: the payer cannot tell from the URI (§8.5).
  bool get isComplete => request.unpayable.isEmpty && settlements.isNotEmpty;

  /// What the request will move, and what it leaves outstanding.
  int get carriedMinorUnits => request.carriedMinorUnits;
  int get withheldMinorUnits => request.withheldMinorUnits;

  /// What the request pays each recipient, in minor units: the amounts a
  /// send records, one record per recipient.
  ///
  /// **What the request carries, not what the payer owes.** The two differ
  /// whenever a recipient's preferred payout is not a Zcash address: §8.5
  /// leaves them out of the URI and reports them, and recording them would
  /// claim a transaction settled a debt it never paid. Settlements to one
  /// recipient sum, as they do in §14.
  Map<String, int> get carriedTo {
    final unpayable = {for (final u in request.unpayable) u.id};
    final owed = <String, int>{};
    for (final s in settlements) {
      if (unpayable.contains(s.to)) continue;
      owed[s.to] = (owed[s.to] ?? 0) + s.amount;
    }
    return owed;
  }

  /// What the request sends each recipient, in zatoshi: the ZEC side of
  /// [carriedTo], summed per recipient as the request carries it.
  Map<String, int> get carriedZatoshi {
    final sent = <String, int>{};
    for (var i = 0; i < request.payments.length; i++) {
      final to = request.recipients[i];
      sent[to] = (sent[to] ?? 0) + request.payments[i].zatoshi;
    }
    return sent;
  }
}

/// What a settlement attempt produced.
///
/// [records] is non-empty only when [result] is [SendResult.sent]. A send that
/// is still pending records nothing and is not a failure either: the caller
/// shows [detail] and does not retry until the wallet resolves it.
class Settled {
  const Settled({
    required this.result,
    this.txid,
    this.detail,
    this.records = const [],
  });

  final SendResult result;

  /// The transaction id, present when [result] is [SendResult.sent], and
  /// when it is [SendResult.pending] and the wallet named the transaction it
  /// built. A pending one records nothing.
  final String? txid;

  /// What to put in front of a person when nothing was recorded.
  final String? detail;

  /// The payment entries, one per recipient. Already appended to the log —
  /// returned so a caller can hand them to a peer, not so a caller can decide
  /// whether to keep them.
  final List<Map<String, dynamic>> records;
}

/// Reads what this device owes on [folded].
///
/// Returns null when the bill carries no rate: an unpriced bill is an ordinary
/// bill, not an error, and there is no refusal code to catch.
PayerObligation? obligationFor(BillHost host, FoldedBill folded) =>
    obligationVia(host, folded, const {});

/// Whether [reviewed] is still the request [folded] asks of this payer, with
/// the same payout choices [via] (§14.2).
///
/// Read immediately before the wallet is called: an entry merged after the
/// payer reviewed the request — an expense, a new rate, a changed address —
/// changes what is owed or where it goes, and the request they saw is no
/// longer the one the bill asks for. A host sends only while this holds.
bool requestStands(
  BillHost host,
  FoldedBill folded,
  PayerObligation reviewed, {
  Map<String, int> via = const {},
}) =>
    obligationVia(host, folded, via)?.uri == reviewed.uri;

/// [obligationFor], with the payer's choice of payout for the recipients
/// [via] names (§14.8).
///
/// [via] maps a participant id to the index of one of their declared payouts,
/// as `choosePayouts` takes it. Who owes what is read from the bill as
/// folded — a preference takes no part in it (§9.1) — and only the request is
/// rendered from the chosen payouts. Refuses as `choosePayouts` does.
PayerObligation? obligationVia(
  BillHost host,
  FoldedBill folded,
  Map<String, int> via,
) {
  final rate = folded.bill.rate;
  if (rate == null) return null;

  // §14 decides this, not this layer: which debts a request may carry, and
  // which wait on a confirmation, is a function of the bill and the records
  // the fold applied. A second implementation here is a second place for the
  // rule to drift.
  final plan = splitz.settleBill(folded.bill);
  final split = splitz.withholdings(
    plan.settlements,
    folded.bill,
    host.me,
    recordedBy: folded.paymentAuthors,
  );
  final mine = split.carried;
  final awaiting = split.awaiting;
  // Checked before anything is rendered, so a choice that names nobody is
  // refused whether or not this device owes anything.
  final chosen = splitz.choosePayouts(folded.bill, via);

  if (mine.isEmpty) {
    return PayerObligation(
      settlements: const [],
      awaiting: awaiting,
      rate: rate,
      request: splitz.renderObligation(
        const [],
        chosen,
        rate: rate,
        skipUnpayable: true,
        readsAddress: host.readsAddress,
      ),
    );
  }

  // `renderObligation` is the protocol's own answer to the hazard in §8.5:
  // either refuse the whole request, or render what can be carried and report
  // the rest. Writing the loop by hand is how a wallet ends up doing neither.
  final rendered = splitz.renderObligation(
    mine,
    chosen,
    rate: rate,
    skipUnpayable: true,
    readsAddress: host.readsAddress,
  );

  return PayerObligation(
      settlements: mine, awaiting: awaiting, rate: rate, request: rendered);
}

/// The id of the payment record for [to]'s share of the transaction [txid].
///
/// One transaction paying several people is several records, and §10.5
/// requires each to carry its own id: a confirmation names one record, so two
/// under one id would let one recipient's word settle a debt another never
/// vouched for, and the fold sets the second aside as `duplicate_payment` —
/// losing the record of a payment that was made. The transaction itself goes
/// in the record's `reference`, which is what `onChain` reads.
///
/// [from] is the payer who writes the record, and the id is theirs under
/// [authoredId].
String paymentIdForSend(String from, String txid, String to) =>
    authoredId(from, '$txid:$to');

/// Sends [obligation] and records that it was sent.
///
/// **Everything the record needs is read before the broadcast.** A send that
/// lands while its record is lost leaves the bill showing a debt that is paid
/// and the payee never seeing the payment, and the transaction cannot be
/// unsent. Nothing here reads state after the wallet is called.
///
/// A record is a claim, not a settlement (§10.5): the balance does not move
/// until somebody confirms it.
Future<Settled> settle(
  BillHost host,
  BillLog log,
  PayerObligation obligation,
) async {
  final uri = obligation.uri;
  if (uri == null) {
    return const Settled(
      result: SendResult.failed,
      detail: 'there is nothing to send',
    );
  }

  // Captured first, deliberately: after `broadcast` returns, this device may
  // be anywhere.
  final owed = obligation.carriedTo;
  if (owed.isEmpty) {
    return const Settled(
      result: SendResult.failed,
      detail: 'there is nothing this request can carry',
    );
  }

  final sent = await host.broadcast(uri);

  // A transaction that was built but not broadcast may still land. Recording
  // it as paid would settle a debt nothing on chain has settled; recording
  // nothing and letting a retry through would pay it twice. Neither is chosen
  // here — the caller is told, and the debt stays exactly as it was.
  if (sent.result != SendResult.sent) {
    return Settled(result: sent.result, detail: sent.detail, txid: sent.txid);
  }

  final txid = sent.txid;
  if (txid == null) {
    // A hand-built `Sent` can say sent with no id. Pending, not failed: the
    // wallet said money left, so a retry could pay twice.
    return const Settled(
      result: SendResult.pending,
      detail: 'the wallet reported a send with no transaction id',
    );
  }

  final records = await recordSend(host, log, owed, txid,
      zatoshi: obligation.carriedZatoshi, rate: obligation.rate);
  return Settled(result: SendResult.sent, txid: txid, records: records);
}

/// Records that the transaction [txid] paid [carried]: one signed payment
/// record per recipient, appended to [log] and returned.
///
/// Each record states what it sent in ZEC, from [zatoshi], and the rate it
/// was priced at, from [rate] (§9.2): the payee confirms against a figure
/// they can compare with what arrived, not a fiat amount alone, so a rate a
/// payer lowered before paying shows on the record they confirm.
///
/// What [settle] writes after a send that succeeded, and what a wallet writes
/// when a send it could not resolve at the time is later found on chain. The
/// two must be the same records: a payment recorded twice under different
/// ids is two payments to every reader.
///
/// Signed before they are kept. A verifying fold applies an entry written as
/// a bound participant only from a copy that verifies against their key
/// (§10.3), so an unsigned record of this payer's own payment would be set
/// aside on this device and the debt offered to them again.
Future<List<Map<String, dynamic>>> recordSend(
  BillHost host,
  BillLog log,
  Map<String, int> carried,
  String txid, {
  Map<String, int> zatoshi = const {},
  splitz.ExchangeRate? rate,
}) async {
  // The bill the records are signed for (§10.6): the one the log was opened
  // for, or the one its entries create.
  final billId = log.billId ?? log.fold().bill.id;
  final records = <Map<String, dynamic>>[];
  for (final entry in carried.entries) {
    final record = await signEntry(
      host: host,
      billId: billId,
      entry: recordPayment(
        host: host,
        paymentId: paymentIdForSend(host.me, txid, entry.key),
        to: entry.key,
        amount: entry.value,
        reference: txid,
        zatoshi: zatoshi[entry.key],
        paidAtRate: rate == null ? null : splitz.rateToJson(rate),
      ),
    );
    log.add([record]);
    records.add(record);
  }
  return records;
}

/// One payment a wallet is about to make: what its own ZIP 321 reader made of
/// a request, before anything is signed.
class ProposedOutput {
  const ProposedOutput(this.address, this.zatoshi);

  final String address;
  final int zatoshi;
}

/// How the payments a wallet is about to sign differ from the request (§14.6).
class ProposalCheck {
  const ProposalCheck({required this.missing, required this.unexpected});

  /// Payments the request carries that the proposal does not, in request
  /// order.
  final List<splitz.Zip321Payment> missing;

  /// Payments the proposal makes that the request does not carry, in the
  /// order they were given.
  final List<ProposedOutput> unexpected;

  /// True when the proposal pays exactly what the request asks.
  bool get matches => missing.isEmpty && unexpected.isEmpty;
}

/// Compares what a wallet is about to sign with the request [uri] (§14.6).
///
/// [outputs] are the payments the wallet's own reader produced from [uri],
/// without change: a reader that keeps only the first of several payments
/// pays one recipient while the payer was shown them all. Each requested
/// payment is matched to one proposed output with the same address and the
/// same zatoshi; order is not significant, and one output cannot answer for
/// two payments.
///
/// [uri] must be a request this protocol wrote: it is read with
/// [splitz.readRequest], which refuses anything else with
/// `zip321_not_canonical`.
ProposalCheck checkProposal(String uri, List<ProposedOutput> outputs) {
  final requested = splitz.readRequest(uri);
  final pool = List<ProposedOutput?>.of(outputs);
  final missing = <splitz.Zip321Payment>[];
  for (final payment in requested) {
    final at = pool.indexWhere((o) =>
        o != null &&
        o.address == payment.address &&
        o.zatoshi == payment.zatoshi);
    if (at < 0) {
      missing.add(payment);
    } else {
      pool[at] = null;
    }
  }
  return ProposalCheck(
    missing: missing,
    unexpected: [
      for (final o in pool)
        if (o != null) o
    ],
  );
}
