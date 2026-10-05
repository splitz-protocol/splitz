/// §14.4, as a property over generated bills: paying some of one's debts
/// exactly never holds back the ones not yet paid.
///
/// The withholding rule exists to stop a debt being asked for twice. Its
/// failure mode is the opposite one, an honest payer refused: a rule that
/// counts each payment against every settlement covering its payee leaves a
/// payer who owes three people unable to pay the third after paying two.
/// Every case in `vectors/withholdings.json` is one bill; this is many.
///
/// The second property is the other direction: whatever expenses arrive while
/// a payment is unconfirmed, what a request carries plus what the payer has
/// pending never exceeds what they owe, so paying every request and having
/// every payment confirmed never leaves the payer overpaid by a request.
library;

import 'dart:math';
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
  splitz.Clock get now =>
      () => DateTime.utc(2026, 10, 28, 19).add(Duration(seconds: _n++));
  @override
  splitz.Randomness get randomBytes => (n) =>
      Uint8List.fromList(List<int>.generate(n, (i) => i + _me.codeUnitAt(0)));
  @override
  splitz.Broadcast get broadcast =>
      (uri) async => throw StateError('this test sends nothing');
}

const _names = ['ana', 'ben', 'cai', 'dee', 'eve', 'fay'];

void main() {
  test('paying some debts exactly leaves every other one payable', () {
    var bills = 0, checked = 0;
    for (var seed = 1; seed <= 500; seed++) {
      final rnd = Random(seed);
      final people = _names.sublist(0, 3 + rnd.nextInt(4));
      final hosts = {for (final p in people) p: _Host(p)};
      final entries = <Map<String, dynamic>>[
        splitz.createBill(
            host: hosts[people.first]!,
            name: 'Trip',
            currency: 'USD',
            creatorKey: 'A' * 43),
        for (final p in people)
          splitz.joinBill(host: hosts[p]!, name: p, payTo: 'u1$p'),
      ];
      final count = 2 + rnd.nextInt(5);
      for (var i = 0; i < count; i++) {
        final payer = people[rnd.nextInt(people.length)];
        final among = [
          for (final p in people)
            if (p == payer || rnd.nextBool()) p
        ]..sort();
        entries.add(splitz.addExpense(
            host: hosts[payer]!,
            expenseId: 'x$i',
            paidBy: payer,
            amount: 100 + rnd.nextInt(5000),
            split: {'type': 'equal', 'among': among}));
      }
      entries.add(splitz.setRate(
          host: hosts[people.first]!,
          currency: 'USD',
          minorUnitsPerZec: 100000));

      for (final debtor in people) {
        final me = hosts[debtor]!;
        final before = splitz.obligationFor(
            me, splitz.BillLog(me, entries: entries).fold());
        final owed = before?.settlements ?? const <splitz.Settlement>[];
        if (owed.length < 2) continue;
        bills++;
        // A random nonempty proper subset, each paid exactly what it asks.
        final paid = <String>{};
        while (paid.isEmpty || paid.length == owed.length) {
          paid
            ..clear()
            ..addAll([
              for (final s in owed)
                if (rnd.nextBool()) s.to
            ]);
        }
        final withPayments = [
          ...entries,
          for (final s in owed)
            if (paid.contains(s.to))
              splitz.recordPayment(
                  host: me, paymentId: 'p-${s.to}', to: s.to, amount: s.amount),
        ];
        final after = splitz.obligationFor(
            me, splitz.BillLog(me, entries: withPayments).fold())!;
        final stillAsked = {for (final s in after.settlements) s.to};
        final waiting = {for (final a in after.awaiting) a.to};
        for (final s in owed) {
          checked++;
          if (paid.contains(s.to)) {
            expect(waiting, contains(s.to),
                reason: 'seed $seed: $debtor paid ${s.to}; it waits');
          } else {
            expect(stillAsked, contains(s.to),
                reason: 'seed $seed: $debtor paid ${paid.toList()..sort()} '
                    'exactly; ${s.to} must still be payable');
          }
        }
      }
    }
    // A property that never reached a bill proves nothing.
    expect(bills, greaterThan(200));
    print('$bills payers on generated bills, $checked settlements checked');
  });

  test('a request plus what is pending never exceeds what is owed', () {
    var payers = 0, held = 0;
    for (var seed = 1; seed <= 1500; seed++) {
      final rnd = Random(seed);
      final people = _names.sublist(0, 3 + rnd.nextInt(4));
      final hosts = {for (final p in people) p: _Host(p)};
      final entries = <Map<String, dynamic>>[
        splitz.createBill(
            host: hosts[people.first]!,
            name: 'Trip',
            currency: 'USD',
            creatorKey: 'A' * 43),
        for (final p in people)
          splitz.joinBill(host: hosts[p]!, name: p, payTo: 'u1$p'),
        splitz.setRate(
            host: hosts[people.first]!,
            currency: 'USD',
            minorUnitsPerZec: 100000),
      ];
      var n = 0;
      void expense() {
        final payer = people[rnd.nextInt(people.length)];
        final among = [
          for (final p in people)
            if (p == payer || rnd.nextBool()) p
        ]..sort();
        entries.add(splitz.addExpense(
            host: hosts[payer]!,
            expenseId: 'x${n++}',
            paidBy: payer,
            amount: 100 + rnd.nextInt(5000),
            split: {'type': 'equal', 'among': among}));
      }

      for (var i = 0; i < 2 + rnd.nextInt(4); i++) {
        expense();
      }
      final debtor = people[rnd.nextInt(people.length)];
      final me = hosts[debtor]!;
      final first = splitz.obligationFor(
          me, splitz.BillLog(me, entries: entries).fold())!;
      if (first.settlements.isEmpty) continue;
      payers++;
      // Some of what is asked, each paid exactly, and left unconfirmed.
      for (final (i, s) in first.settlements.indexed) {
        if (i == 0 || rnd.nextBool()) {
          entries.add(splitz.recordPayment(
              host: me, paymentId: 'p-${s.to}', to: s.to, amount: s.amount));
        }
      }
      // Expenses arrive while those payments wait; netting may move the
      // debt they paid onto somebody else.
      for (var i = 0; i < 1 + rnd.nextInt(3); i++) {
        expense();
      }
      final bill = splitz.BillLog(me, entries: entries).fold().bill;
      final pending = [
        for (final p in bill.payments)
          if (p.from == debtor && !bill.confirmedPayments.contains(p.id))
            p.amount
      ].fold(0, (a, b) => a + b);
      final balance = core.netBalances(bill)[debtor]!;
      final owes = balance < 0 ? -balance : 0;
      final asked = splitz.obligationFor(
          me, splitz.BillLog(me, entries: entries).fold())!;
      final carried = asked.settlements.fold(0, (a, s) => a + s.amount);
      if (asked.awaiting.isNotEmpty) held++;
      // Later expenses can leave what is pending above what is owed; the
      // request then adds nothing to it.
      final room = owes > pending ? owes - pending : 0;
      expect(carried, lessThanOrEqualTo(room),
          reason: 'seed $seed: $debtor owes $owes, has $pending pending, '
              'and is asked for $carried more');

      // Paying every request and confirming everything leaves nobody
      // overpaid.
      for (final s in asked.settlements) {
        entries.add(splitz.recordPayment(
            host: me, paymentId: 'q-${s.to}', to: s.to, amount: s.amount));
      }
      final f = splitz.BillLog(me, entries: entries).fold();
      for (final p in f.bill.payments) {
        entries.add(splitz.confirmPayment(
            host: hosts[p.to]!,
            paymentId: p.id,
            method: 'recipientConfirmed',
            record: f.paymentDigests[p.id]!));
      }
      final end = splitz.BillLog(me, entries: entries).fold().bill;
      expect(end.confirmedPayments, hasLength(end.payments.length));
      final over = balance + pending > 0 ? balance + pending : 0;
      expect(core.netBalances(end)[debtor]!, lessThanOrEqualTo(over),
          reason: 'seed $seed: $debtor ends overpaid by what was asked');
    }
    expect(payers, greaterThan(700));
    expect(held, greaterThan(100));
    print('$payers payers checked, $held with a debt held back');
  });
}
