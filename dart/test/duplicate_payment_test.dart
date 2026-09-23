/// §10.5: a confirmation names one payment record.
///
/// One transaction paying three people is three records. If they share an id,
/// a confirmation from one recipient names all three, and a debt settles on
/// the word of somebody who was never owed it. The fold refuses the duplicate;
/// the host builders mint one id per recipient and carry the transaction in
/// `reference`.
import 'dart:typed_data';

import 'package:splitz_core/host.dart' as splitz;
import 'package:splitz_core/splitz_core.dart' as core;
import 'package:test/test.dart';

class _Host extends splitz.BillHost {
  _Host(this._me);
  final String _me;
  int _n = 0;
  @override
  String get me => _me;
  @override
  String? get payToAddress => 'u1$_me';
  @override
  splitz.Clock get now => () => DateTime.utc(2026, 10, 28, 19, 30 + (_n++));
  @override
  splitz.Randomness get randomBytes => (n) =>
      Uint8List.fromList(List<int>.generate(n, (i) => i + _me.codeUnitAt(0)));
  @override
  splitz.Broadcast get broadcast =>
      (uri) async => throw StateError('this test sends nothing');
}

/// A bill where Ana owes Ben, Cai and Dee 30 each, before any payment.
List<Map<String, dynamic>> _bill(splitz.BillHost ana) {
  final entries = <Map<String, dynamic>>[
    splitz.createBill(
        host: ana, name: 'Dinner', currency: 'USD', creatorKey: 'A' * 43),
    splitz.joinBill(host: ana, name: 'Ana', payTo: 'u1ana'),
  ];
  for (final who in ['ben', 'cai', 'dee']) {
    final h = _Host(who);
    entries.add(splitz.joinBill(host: h, name: who, payTo: 'u1$who'));
    entries.add(splitz.addExpense(
      host: h,
      expenseId: 'x-$who',
      paidBy: who,
      amount: 60,
      split: {
        'type': 'equal',
        'among': ['ana', who]..sort(),
      },
    ));
  }
  entries.add(
      splitz.setRate(host: ana, currency: 'USD', minorUnitsPerZec: 100000));
  return entries;
}

int _covered(splitz.FoldedBill f) => f.bill.payments
    .where((p) => f.bill.confirmedPayments.contains(p.id))
    .length;

void main() {
  const txid = 'tx-one-transaction';

  test('records sharing an id are refused, so one word settles one debt', () {
    final ana = _Host('ana');
    final entries = _bill(ana);
    final records = [
      for (final who in ['ben', 'cai', 'dee'])
        splitz.recordPayment(host: ana, paymentId: txid, to: who, amount: 30),
    ];
    entries.addAll(records);
    // Ben confirms the record addressed to him, which is the one that stands.
    entries.add(splitz.confirmPayment(
        host: _Host('ben'),
        paymentId: txid,
        method: 'recipientConfirmed',
        record: core.paymentDigest(
            (records.first['payment'] as Map).cast<String, dynamic>())));

    final folded = splitz.BillLog(ana, entries: entries).fold();

    expect(folded.bill.payments.length, 1,
        reason: 'the first record stands, the other two are refused');
    expect(
        folded.setAside.where((r) => r.code == 'duplicate_payment').length, 2,
        reason: 'each duplicate is reported, never dropped silently');
    expect(_covered(folded), 1,
        reason: "Ben's confirmation covers Ben's payment and no other");
  });

  test('one id per recipient is what a multi-recipient send must write', () {
    final ana = _Host('ana');
    final entries = _bill(ana);
    final records = [
      for (final who in ['ben', 'cai', 'dee'])
        splitz.recordPayment(
            host: ana,
            paymentId: '$txid:$who',
            to: who,
            amount: 30,
            reference: txid),
    ];
    entries.addAll(records);
    entries.add(splitz.confirmPayment(
        host: _Host('ben'),
        paymentId: '$txid:ben',
        method: 'recipientConfirmed',
        record: core.paymentDigest(
            (records.first['payment'] as Map).cast<String, dynamic>())));

    final folded = splitz.BillLog(ana, entries: entries).fold();

    expect(folded.bill.payments.length, 3, reason: 'all three records stand');
    expect(folded.setAside, isEmpty);
    expect(_covered(folded), 1, reason: 'only Ben confirmed');
    // And the two who said nothing are still owed, which is the whole point.
    final owed = splitz.obligationFor(ana, folded)!;
    expect(owed.awaiting.map((a) => a.to).toSet(), {'cai', 'dee'});
  });
}
