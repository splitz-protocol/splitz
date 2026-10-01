/// Payments to this device that its wallet has seen arrive (SPEC.md §14.7).
///
/// A payment record is the payer's claim, and only the payee's confirmation
/// moves a balance (§10.5). A wallet that received the transaction a record
/// names already holds the evidence, so it can propose the confirmation
/// instead of asking the payee to find the payment by hand.
library;

import 'package:splitz_core/splitz_core.dart' as splitz;

import 'bill_log.dart';

/// A transaction id as §14.7 compares it: ASCII space, tab, carriage return
/// and line feed removed from both ends, and ASCII letters lower-cased.
///
/// Nothing wider: a txid is hexadecimal, and each language's own `trim` and
/// lower-casing reach different sets of Unicode characters, so one record
/// would match on one device and not on another.
String txidKey(String txid) {
  const space = {0x20, 0x09, 0x0d, 0x0a};
  var start = 0, end = txid.length;
  while (start < end && space.contains(txid.codeUnitAt(start))) {
    start++;
  }
  while (end > start && space.contains(txid.codeUnitAt(end - 1))) {
    end--;
  }
  return String.fromCharCodes([
    for (final u in txid.substring(start, end).codeUnits)
      u >= 0x41 && u <= 0x5a ? u + 0x20 : u,
  ]);
}

/// Money this wallet received in one transaction: the sum of that
/// transaction's outputs to this account, in zatoshi.
class IncomingTransaction {
  const IncomingTransaction(this.txid, this.zatoshi);

  final String txid;
  final int zatoshi;
}

/// A payment record to this device, and the transaction it names.
class Arrival {
  const Arrival({
    required this.billId,
    required this.payment,
    required this.record,
    required this.txid,
  });

  final String billId;

  /// The record, as the bill folds it: what the payee is shown before
  /// confirming (§14.2) — its ZEC, the rate it was priced at, its reference.
  final splitz.PaymentRecord payment;

  /// The digest a confirmation of [payment] carries as `record` (§10.5).
  final String record;

  /// The transaction, lower-cased.
  final String txid;
}

/// What [arrivalsFor] found.
class Arrivals {
  const Arrivals({
    required this.arrived,
    required this.short,
    required this.unstated,
    this.disputed = const [],
  });

  /// Records whose transaction arrived carrying at least the ZEC they state.
  /// Each may be confirmed with `walletReceived` once the payee has been
  /// shown it.
  final List<Arrival> arrived;

  /// Records whose transaction arrived, but with less ZEC than they state
  /// once every other record naming it is counted. Not confirmed: a record
  /// claiming more than arrived is not evidence of the rest.
  final List<Arrival> short;

  /// Records whose transaction arrived and which state no ZEC, so nothing
  /// can be checked against it.
  final List<Arrival> unstated;

  /// Records naming a transaction that records from another payer also name.
  /// None of them is proposed: a shielded transaction does not say who sent
  /// it, and any participant can copy a reference they have seen, so the
  /// payee has to settle which record it pays before confirming any.
  final List<Arrival> disputed;
}

/// Matches unconfirmed ZEC payment records to [me] against the transactions
/// [received], across every one of [bills] at once (§14.7).
///
/// A record matches when its `reference` names a received transaction. The
/// ZEC that transaction brought is counted **once, across all bills**:
/// records already confirmed that name it use their share first, then the
/// rest in order of bill id and payment id. Without that, one real payment
/// recorded on two bills is evidence for both.
///
/// A transaction named by records from more than one payer is evidence for
/// none of them: every such record is [Arrivals.disputed].
Arrivals arrivalsFor(
  List<FoldedBill> bills,
  String me,
  List<IncomingTransaction> received,
) {
  // What each transaction brought, and then what of it is not yet used.
  // Held inside [0, 21000000 ZEC]: no transaction brings more than exists,
  // and a record's `zatoshi` may be as large as §2.2 allows, so subtracting
  // floors at zero rather than wrapping.
  final left = <String, int>{};
  for (final t in received) {
    final id = txidKey(t.txid);
    left[id] = _add(left[id] ?? 0, t.zatoshi);
  }

  final ordered = [...bills]
    ..sort((a, b) => splitz.compareUtf8(a.bill.id, b.bill.id));
  // Who each received transaction is claimed to be from, over every record
  // to [me] that names it, confirmed or not. A payer is the key §10.7 bound
  // to them, or, unbound, their id on that one bill: an id is chosen by
  // whoever joins, so the same string on two bills can be two people.
  final payers = <String, Set<Object>>{};
  for (final folded in ordered) {
    for (final p in folded.bill.payments) {
      if (p.to != me || p.method != 'shieldedZec') continue;
      final reference = p.reference;
      if (reference == null) continue;
      final txid = txidKey(reference);
      if (!left.containsKey(txid)) continue;
      final key = folded.identities.bound[p.from];
      payers
          .putIfAbsent(txid, () => {})
          .add(key != null ? (key,) : (folded.bill.id, p.from));
    }
  }
  final candidates = <Arrival>[];
  for (final folded in ordered) {
    final payments = [...folded.bill.payments]
      ..sort((a, b) => splitz.compareUtf8(a.id, b.id));
    for (final p in payments) {
      if (p.to != me || p.method != 'shieldedZec') continue;
      final reference = p.reference;
      final txid = reference == null ? null : txidKey(reference);
      if (txid == null || !left.containsKey(txid)) continue;
      if (folded.bill.confirmedPayments.contains(p.id)) {
        left[txid] = _use(left[txid]!, p.zatoshi ?? 0);
        continue;
      }
      final record = folded.paymentDigests[p.id];
      if (record == null) continue;
      candidates.add(Arrival(
        billId: folded.bill.id,
        payment: p,
        record: record,
        txid: txid,
      ));
    }
  }

  final arrived = <Arrival>[];
  final short = <Arrival>[];
  final unstated = <Arrival>[];
  final disputed = <Arrival>[];
  for (final a in candidates) {
    final stated = a.payment.zatoshi;
    if (payers[a.txid]!.length > 1) {
      disputed.add(a);
    } else if (stated == null) {
      unstated.add(a);
    } else if (stated <= left[a.txid]!) {
      left[a.txid] = _use(left[a.txid]!, stated);
      arrived.add(a);
    } else {
      short.add(a);
    }
  }
  return Arrivals(
    arrived: arrived,
    short: short,
    unstated: unstated,
    disputed: disputed,
  );
}

int _clamp(int zatoshi) => zatoshi < 0
    ? 0
    : (zatoshi > splitz.maxZatoshi ? splitz.maxZatoshi : zatoshi);

/// [a] plus [b], both held in [0, 21000000 ZEC]; the sum is capped there too.
int _add(int a, int b) => _clamp(_clamp(a) + _clamp(b));

/// What is left of [left] once [used] of it is spoken for; never below zero.
int _use(int left, int used) => used >= left ? 0 : left - used;
