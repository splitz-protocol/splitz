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
    this.awaiting = const <splitz.Awaiting>[],
    this.contested = const <splitz.Contested>[],
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

  /// Debts to a participant whose identity two keys claim.
  ///
  /// Left out of [settlements] and out of the request. The payer decides,
  /// having been shown the contest — nothing here decides for them by paying
  /// whichever record happens to be on the bill.
  final List<splitz.Contested> contested;

  /// The protocol's answer: the request, what it carries, and who it could
  /// not carry with the reason for each.
  final splitz.Obligation request;

  String? get uri => request.uri;
  List<splitz.Unpayable> get unpayable => request.unpayable;

  /// True when every debt this device owes is in the request. A request that
  /// covers less must say so: the payer cannot tell from the URI (§8.5).
  bool get isComplete => request.unpayable.isEmpty && settlements.isNotEmpty;

  /// What the request will move, and what it leaves outstanding.
  int get carriedMinorUnits => request.carriedMinorUnits;
  int get withheldMinorUnits => request.withheldMinorUnits;
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

  /// The transaction id, present when [result] is [SendResult.sent].
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
PayerObligation? obligationFor(
  BillHost host,
  FoldedBill folded, {
  Set<String> payAnyway = const {},
}) {
  final rate = folded.bill.rate;
  if (rate == null) return null;

  // §14 decides this, not this layer: which debts a request may carry, and
  // which wait on a confirmation or on a contest, is a function of the bill
  // and the identities the fold resolved. A second implementation here is a
  // second place for the rule to drift.
  final plan = splitz.settleBill(folded.bill);
  final split = splitz.withholdings(
    plan.settlements,
    folded.bill,
    host.me,
    contestedIds: folded.identities.contested,
    payAnyway: payAnyway,
  );
  final mine = split.carried;
  final awaiting = split.awaiting;
  final contested = split.contested;

  if (mine.isEmpty) {
    return PayerObligation(
      settlements: const [],
      awaiting: awaiting,
      contested: contested,
      request: splitz.renderObligation(const [], folded.bill,
          rate: rate, skipUnpayable: true),
    );
  }

  // `renderObligation` is the protocol's own answer to the hazard in §8.5:
  // either refuse the whole request, or render what can be carried and report
  // the rest. Writing the loop by hand is how a wallet ends up doing neither.
  final rendered = splitz.renderObligation(
    mine,
    folded.bill,
    rate: rate,
    skipUnpayable: true,
  );

  return PayerObligation(
      settlements: mine,
      awaiting: awaiting,
      contested: contested,
      request: rendered);
}

/// The id of the payment record for [to]'s share of the transaction [txid].
///
/// One transaction paying several people is several records, and §10.5
/// requires each to carry its own id: a confirmation names one record, so two
/// under one id would let one recipient's word settle a debt another never
/// vouched for, and the fold sets the second aside as `duplicate_payment` —
/// losing the record of a payment that was made. The transaction itself goes
/// in the record's `reference`, which is what `onChain` reads.
String paymentIdForSend(String txid, String to) => '$txid:$to';

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
  //
  // **What the request carries, not what the payer owes.** The two differ
  // whenever a recipient's preferred payout is not a Zcash address: §8.5
  // leaves them out of the URI and reports them, and recording them here
  // would claim a transaction settled a debt it never paid — a debt the
  // payee then has to contest rather than simply still be owed.
  final unpayable = {for (final u in obligation.unpayable) u.id};
  // Records to one recipient sum, as they do in §14. A map literal would keep
  // the last settlement and drop the rest, recording less than was sent.
  final owed = <String, int>{};
  for (final s in obligation.settlements) {
    if (unpayable.contains(s.to)) continue;
    owed[s.to] = (owed[s.to] ?? 0) + s.amount;
  }
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
    return Settled(result: sent.result, detail: sent.detail);
  }

  final txid = sent.txid;
  if (txid == null) {
    throw StateError('a send that succeeded must carry a transaction id');
  }

  final records = <Map<String, dynamic>>[];
  for (final entry in owed.entries) {
    final record = recordPayment(
      host: host,
      paymentId: paymentIdForSend(txid, entry.key),
      to: entry.key,
      amount: entry.value,
      reference: txid,
    );
    log.add([record]);
    records.add(record);
  }
  return Settled(result: SendResult.sent, txid: txid, records: records);
}

/// Records a debt settled in cash (§9.2).
///
/// Nothing is sent and nothing is verified: cash moved outside this protocol,
/// and the only evidence it ever has is the recipient's confirmation (§10.5).
/// **A caller must not present this with the confidence of an on-chain
/// payment** — anyone on the bill can write one.
///
/// [paymentId] is the caller's, because there is no transaction to take one
/// from. It MUST be unique on the bill: two cash payments sharing an id are
/// one payment to every reader that folds the log.
Map<String, dynamic> settleCash(
  BillHost host,
  BillLog log, {
  required String paymentId,
  required String to,
  required int amount,
  String? note,
}) {
  final record = recordPayment(
    host: host,
    paymentId: paymentId,
    to: to,
    amount: amount,
    method: 'cash',
    note: note,
  );
  log.add([record]);
  return record;
}

/// Records a debt settled by a swap off this chain (§9.2).
///
/// **Verifiable only in half.** What left this wallet is ZEC and is recorded
/// in [zatoshi]; what the recipient was owed arrives as another asset on
/// another chain, which this bill cannot see. A caller MUST NOT present this
/// as confirmed on the strength of the ZEC leg alone — that the deposit was
/// sent is not that the recipient was paid, and only the recipient can say
/// the latter (§10.5).
///
/// [reference] identifies the swap: the provider's intent id, or the
/// transaction on the destination chain. **It is not a Zcash txid**, and a
/// reader that renders it as one is wrong for every swap. The chain it names
/// is the chain of the `swap` payout being settled.
///
/// Where a participant's payout may change after this is recorded, that chain
/// stops being derivable from the current payout; pass it in [note].
Map<String, dynamic> settleSwap(
  BillHost host,
  BillLog log, {
  required String reference,
  required String to,
  required int amount,
  int? zatoshi,
  Map<String, dynamic>? paidAtRate,
  String? note,
}) {
  final record = recordPayment(
    host: host,
    // The swap's own identifier is what a reader checks this record against,
    // so it is the payment id as well as the reference. A second identifier
    // here would be one nothing outside this bill has ever heard of.
    paymentId: reference,
    to: to,
    amount: amount,
    method: 'swap',
    reference: reference,
    zatoshi: zatoshi,
    paidAtRate: paidAtRate,
    note: note,
  );
  log.add([record]);
  return record;
}
