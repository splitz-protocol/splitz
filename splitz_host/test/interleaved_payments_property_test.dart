/// Payments and confirmations interleaved on a closed
/// bill, so §14.4's withholdings run while netting reroutes debts under
/// unconfirmed payments.
///
/// Invariants, at every step:
///  - what a request carries plus what the payer has pending never exceeds
///    what the payer owes (never asked for more than owed);
///  - nobody's balance changes sign: an honest flow over fixed expenses never
///    turns a debtor into a creditor (an overpayment would);
///  - the flow terminates with every balance zero (never stuck).
library;

import 'dart:io';
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
  splitz.Randomness get randomBytes =>
      (n) => Uint8List.fromList(
        List<int>.generate(n, (i) => i + _me.codeUnitAt(0)),
      );
  @override
  splitz.Broadcast get broadcast =>
      (uri) async => throw StateError('this test sends nothing');
}

final _trace = int.tryParse(Platform.environment['TRACE'] ?? '') ?? -1;
final _noPartial = Platform.environment['NOPARTIAL'] == '1';
final _oneAtATime = Platform.environment['ONE'] == '1';
final _correct = Platform.environment['CORRECT'] == '1';
const _names = ['ana', 'ben', 'cai', 'dee', 'eve', 'fay'];
const _zec =
    'u13j3q8q8f9hx2nx0w9l52dqksy4png7fgm0lqjh8ahn9enyvz5z9xnwzdcdjmpf756s2y88rnyr9px4f4k9w03sl6fr4vwsqcvg8ggfjx';

/// Runs one generated bill. [breakWithholding] replaces the obligation's
/// carried set with every settlement of the payer (the positive control: it
/// must trip the invariants).
Map<String, int> _run(int seed, {bool breakWithholding = false}) {
  final stats = <String, int>{};
  void saw(String s) => stats[s] = (stats[s] ?? 0) + 1;
  final rnd = Random(seed);
  final people = _names.sublist(0, 3 + rnd.nextInt(4));
  final hosts = {for (final p in people) p: _Host(p)};
  final creator = people.first;
  final cashOnly = {
    for (final p in people)
      if (rnd.nextInt(3) == 0) p,
  };
  final entries = <Map<String, dynamic>>[];
  splitz.FoldedBill fold() =>
      splitz.BillLog(hosts[creator]!, entries: entries).fold();
  void write(Map<String, dynamic> e, String why) {
    if (_trace == seed) print('  write: $why');
    entries.add(e);
    final aside = fold().setAside.where((s) => s.id == e['id']);
    expect(
      aside,
      isEmpty,
      reason: 'seed $seed $why ${aside.map((a) => a.code)}',
    );
  }

  write(
    splitz.createBill(
      host: hosts[creator]!,
      name: 'T',
      currency: 'USD',
      creatorKey: 'A' * 43,
    ),
    'create',
  );
  for (final p in people) {
    write(
      cashOnly.contains(p)
          ? splitz.joinBill(
              host: hosts[p]!,
              name: p,
              payouts: [
                {'type': 'cash'},
              ],
            )
          : splitz.joinBill(host: hosts[p]!, name: p, payTo: _zec),
      'join $p',
    );
  }
  final count = 2 + rnd.nextInt(6);
  for (var i = 0; i < count; i++) {
    final payer = people[rnd.nextInt(people.length)];
    final among = [
      for (final p in people)
        if (p == payer || rnd.nextBool()) p,
    ]..sort();
    // Sometimes a refund: a negative expense (§4).
    final amount = rnd.nextInt(6) == 0
        ? -(100 + rnd.nextInt(2000))
        : 100 + rnd.nextInt(5000);
    write(
      splitz.addExpense(
        host: hosts[payer]!,
        expenseId: 'x$i',
        paidBy: payer,
        amount: amount,
        split: {'type': 'equal', 'among': among},
      ),
      'expense $i',
    );
  }
  write(
    splitz.setRate(
      host: hosts[creator]!,
      currency: 'USD',
      minorUnitsPerZec: 100000,
    ),
    'rate',
  );
  write(splitz.closeFor(hosts[creator]!, fold()), 'close');

  var start = core.netBalances(fold().bill);
  var corrected = false;
  if (_trace == seed) {
    print('balances $start');
    print(
      'plan ${[
        for (final s in core.settleBill(fold().bill).settlements) '${s.from}->${s.to} ${s.amount} covers ${[for (final c in s.covers) '${c.to}:${c.amount}']}',
      ]}',
    );
  }
  var n = 0;
  for (var step = 0; step < 400; step++) {
    final f = fold();
    final net = core.netBalances(f.bill);
    for (final p in net.keys) {
      final s0 = start[p] ?? 0;
      final s1 = net[p]!;
      expect(
        s0 == 0 ? s1 == 0 : (s1 == 0 || (s1 > 0) == (s0 > 0)),
        isTrue,
        reason: 'seed $seed step $step: $p went from $s0 to $s1 (overpaid)',
      );
    }
    if (_trace == seed) {
      print('step $step net $net');
      print(
        '  plan ${[for (final s in core.settleBill(f.bill).settlements) '${s.from}->${s.to} ${s.amount}']}',
      );
    }
    final unconfirmed = [
      for (final p in f.bill.payments)
        if (!f.bill.confirmedPayments.contains(p.id)) p,
    ];
    // Who can act: a payer with something to pay, or a payee to confirm.
    final actions = <void Function()>[];
    for (final payer in people) {
      final me = hosts[payer]!;
      final o = splitz.obligationFor(me, f);
      if (o == null) continue;
      final carried = breakWithholding
          ? {
              for (final s in core.settleBill(f.bill).settlements)
                if (s.from == payer) s.to: s.amount,
            }
          : {...o.carriedTo, for (final u in o.unpayable) u.id: u.minorUnits};
      if (carried.isEmpty) continue;
      final pending = [
        for (final p in unconfirmed)
          if (p.from == payer) p.amount,
      ].fold(0, (a, b) => a + b);
      final owes = -(net[payer] ?? 0);
      final asked = carried.values.fold(0, (a, b) => a + b);
      expect(
        asked + pending <= owes,
        isTrue,
        reason:
            'seed $seed step $step: $payer asked $asked with $pending '
            'pending, owes $owes',
      );
      actions.add(() {
        final pick = carried.entries.toList();
        final paying = _oneAtATime ? [pick[rnd.nextInt(pick.length)]] : pick;
        for (final MapEntry(key: to, value: amount) in paying) {
          // An honest partial cash payment, now and then.
          final part =
              !_noPartial &&
                  cashOnly.contains(to) &&
                  amount > 1 &&
                  rnd.nextInt(4) == 0
              ? amount ~/ 2
              : amount;
          if (part != amount) saw('partial');
          write(
            splitz.recordPayment(
              host: me,
              paymentId: 'p${n++}',
              to: to,
              amount: part,
              method: cashOnly.contains(to) ? 'cash' : 'shieldedZec',
            ),
            '$payer pays $to $part',
          );
        }
        if (o.awaiting.isNotEmpty) saw('paid beside awaiting');
      });
    }
    for (final p in unconfirmed) {
      actions.add(() {
        write(
          splitz.confirmPayment(
            host: hosts[p.to]!,
            paymentId: p.id,
            method: 'recipientConfirmed',
            record: fold().paymentDigests[p.id]!,
          ),
          '${p.to} confirms ${p.id}',
        );
        saw('confirm');
      });
    }
    if (_correct && !corrected && step > 0) {
      actions.add(() {
        corrected = true;
        write(splitz.reopenFor(hosts[creator]!, fold())!, 'reopen');
        final g = fold();
        final e = g.bill.expenses[rnd.nextInt(g.bill.expenses.length)];
        final author = g.expenseAuthors[e.id]!;
        write(
          splitz.amendExpense(
            host: hosts[author]!,
            folded: g,
            expenseId: e.id,
            amount: e.amount + 1 + rnd.nextInt(3000),
          ),
          'correct ${e.id}',
        );
        write(splitz.closeFor(hosts[creator]!, fold()), 're-close');
        start = core.netBalances(fold().bill);
        saw('corrected');
      });
    }
    if (actions.isEmpty) {
      expect(
        net.values.every((v) => v == 0),
        isTrue,
        reason: 'seed $seed stuck at step $step: $net',
      );
      saw('square');
      return stats;
    }
    actions[rnd.nextInt(actions.length)]();
  }
  fail('seed $seed did not finish in 400 steps');
}

void main() {
  test('a part payment then confirmations pays nobody twice (seed 145)', () {
    // §14.4 across payers: dee's record to ana is in flight when a
    // confirmation re-plans eve's debt onto ana. Ana is not asked for again.
    expect(_run(145)['square'], 1);
  });

  test('interleaved payments and confirmations: never asked for more than '
      'owed, nobody overpaid, never stuck', () {
    final all = <String, int>{};
    for (var seed = 1; seed <= 400; seed++) {
      _run(seed).forEach((k, v) => all[k] = (all[k] ?? 0) + v);
    }
    expect(all['square'], 400);
    expect(all['paid beside awaiting'] ?? 0, greaterThan(20));
    expect(all['partial'] ?? 0, greaterThan(20));
  });

  test('control: carrying every settlement regardless of what is pending '
      'trips the invariants', () {
    var tripped = 0;
    for (var seed = 1; seed <= 100; seed++) {
      try {
        _run(seed, breakWithholding: true);
      } on TestFailure {
        tripped++;
      }
    }
    expect(tripped, greaterThan(0));
  });
}
