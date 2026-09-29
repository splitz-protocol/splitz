/// Payments to this device that its wallet has seen arrive (SPEC.md §14.7).
///
/// A payment record is the payer's claim, and only the payee's confirmation
/// moves a balance (§10.5). A wallet that received the transaction a record
/// names already holds the evidence, so it can propose the confirmation
/// instead of asking the payee to find the payment by hand.
library;

import 'package:splitz_core/splitz_core.dart' as splitz;

import 'bill_log.dart';

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
}

/// Matches unconfirmed ZEC payment records to [me] against the transactions
/// [received], across every one of [bills] at once (§14.7).
///
/// A record matches when its `reference` names a received transaction. The
/// ZEC that transaction brought is counted **once, across all bills**:
/// records already confirmed that name it use their share first, then the
/// rest in order of bill id and payment id. Without that, one real payment
/// recorded on two bills is evidence for both.
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
    final id = t.txid.trim().toLowerCase();
    left[id] = _add(left[id] ?? 0, t.zatoshi);
  }

  final ordered = [...bills]
    ..sort((a, b) => splitz.compareUtf8(a.bill.id, b.bill.id));
  final candidates = <Arrival>[];
  for (final folded in ordered) {
    final payments = [...folded.bill.payments]
      ..sort((a, b) => splitz.compareUtf8(a.id, b.id));
    for (final p in payments) {
      if (p.to != me || p.method != 'shieldedZec') continue;
      final txid = p.reference?.trim().toLowerCase();
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
  for (final a in candidates) {
    final stated = a.payment.zatoshi;
    if (stated == null) {
      unstated.add(a);
    } else if (stated <= left[a.txid]!) {
      left[a.txid] = _use(left[a.txid]!, stated);
      arrived.add(a);
    } else {
      short.add(a);
    }
  }
  return Arrivals(arrived: arrived, short: short, unstated: unstated);
}

int _clamp(int zatoshi) => zatoshi < 0
    ? 0
    : (zatoshi > splitz.maxZatoshi ? splitz.maxZatoshi : zatoshi);

/// [a] plus [b], both held in [0, 21000000 ZEC]; the sum is capped there too.
int _add(int a, int b) => _clamp(_clamp(a) + _clamp(b));

/// What is left of [left] once [used] of it is spoken for; never below zero.
int _use(int left, int used) => used >= left ? 0 : left - used;
