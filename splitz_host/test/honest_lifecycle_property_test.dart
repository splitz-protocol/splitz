/// The honest lifecycle around paying, as a property over generated bills:
/// people join (one sometimes by mistake), expenses are written and corrected,
/// somebody is taken off, the creator closes, reopens for a correction and
/// closes again, payers pay — a mistaken cash record withdrawn, one
/// transaction for the ZEC and a swap — and every payee confirms.
///
/// Every refusal in the host is written against somebody doing wrong. This
/// asks the inverse of all of them along that path: is anybody doing nothing
/// wrong ever turned away?
library;

import 'dart:math';
import 'dart:typed_data';

import 'package:splitz_core/host.dart' as splitz;
import 'package:splitz_core/splitz_core.dart' as core;
import 'package:splitz_host/splitz_host.dart';
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

const _names = ['ana', 'ben', 'cai', 'dee', 'eve', 'fay'];
const _rate = 100000;
const _usdc = TradableAsset(
  assetId: 'usdc-base',
  symbol: 'USDC',
  chain: 'base',
  decimals: 6,
);

/// An address from the corpus (`tools/corpus/_spec.py` ADDRESSES[0]), so the
/// request renders (§8.6).
const _zec =
    'u13j3q8q8f9hx2nx0w9l52dqksy4png7fgm0lqjh8ahn9enyvz5z9xnwzdcdjmpf756s2y88rnyr9px4f4k9w03sl6fr4vwsqcvg8ggfjx';

/// One bill as the property drives it: its entries, the devices writing them,
/// and how each person asked to be paid.
class _Bill {
  _Bill(this.people, this.lane) : hosts = {for (final p in people) p: _Host(p)};
  final List<String> people;
  final Map<String, String> lane;
  final Map<String, _Host> hosts;
  final entries = <Map<String, dynamic>>[];

  String get creator => people.first;
  splitz.FoldedBill fold() =>
      splitz.BillLog(hosts[creator]!, entries: entries).fold();

  /// Writes [entry] and asserts every device's fold applies it: an honest
  /// write is never set aside.
  void write(Map<String, dynamic> entry, String why) {
    entries.add(entry);
    final aside = fold().setAside.where((s) => s.id == entry['id']);
    expect(aside, isEmpty, reason: '$why: ${aside.map((s) => s.code)}');
  }
}

Map<String, dynamic> _join(_Host h, String p, String lane) => switch (lane) {
  'zec' => splitz.joinBill(host: h, name: p, payTo: _zec),
  'swap' => splitz.joinBill(
    host: h,
    name: p,
    payouts: [
      {
        'type': 'swap',
        'asset': 'USDC',
        'chain': 'base',
        'address': '0x${p.codeUnitAt(0).toRadixString(16) * 20}',
      },
    ],
  ),
  _ => splitz.joinBill(
    host: h,
    name: p,
    payouts: [
      {'type': 'cash'},
    ],
  ),
};

SwapQuote _quote(splitz.Bill bill, String to, int minorUnits, int n) =>
    SwapQuote(
      depositAddress: 't1deposit$n',
      amountInZatoshi: core.fiatToZatoshi(
        minorUnits,
        bill.rate!,
        amountCurrency: 'USD',
      ),
      amountOut: '1',
      asset: _usdc,
      deadline: '2099-01-01T00:00:00.000Z',
      recipient: bill.participant(to)!.payouts.first.address,
      reference: 'swap-$n',
    );

const _at = '2026-10-28T19:30:00.000Z';

void main() {
  test('the honest lifecycle around paying is never refused: removing a '
      'person, correcting an expense, reopening, withdrawing a mistaken '
      'record, one send for ZEC and a swap', () {
    final seen = <String, int>{};
    void saw(String what) => seen[what] = (seen[what] ?? 0) + 1;

    for (var seed = 1; seed <= 300; seed++) {
      final rnd = Random(seed);
      final people = _names.sublist(0, 3 + rnd.nextInt(4));
      final b = _Bill(people, {
        for (final p in people)
          p: const ['zec', 'swap', 'cash'][rnd.nextInt(3)],
      });
      final why = 'seed $seed';
      b.write(
        splitz.createBill(
          host: b.hosts[b.creator]!,
          name: 'Trip',
          currency: 'USD',
          creatorKey: 'A' * 43,
        ),
        '$why create',
      );
      for (final p in people) {
        b.write(_join(b.hosts[p]!, p, b.lane[p]!), '$why join $p');
      }
      // Somebody who joined by mistake, named by nothing.
      final stray = rnd.nextInt(3) == 0;
      if (stray) {
        final h = b.hosts['zed'] = _Host('zed');
        b.write(_join(h, 'zed', 'zec'), '$why stray join');
      }
      final count = 2 + rnd.nextInt(5);
      for (var i = 0; i < count; i++) {
        final payer = people[rnd.nextInt(people.length)];
        final among = [
          for (final p in people)
            if (p == payer || rnd.nextBool()) p,
        ]..sort();
        b.write(
          splitz.addExpense(
            host: b.hosts[payer]!,
            expenseId: 'x$i',
            paidBy: payer,
            amount: 100 + rnd.nextInt(5000),
            split: {'type': 'equal', 'among': among},
          ),
          '$why expense $i',
        );
      }

      // While open: an author corrects their own expense.
      void correctOne(String when) {
        final f = b.fold();
        expect(splitz.expenseRefusal(f), isNull, reason: '$why $when');
        final e = f.bill.expenses[rnd.nextInt(f.bill.expenses.length)];
        final author = f.expenseAuthors[e.id]!;
        b.write(
          splitz.amendExpense(
            host: b.hosts[author]!,
            folded: f,
            expenseId: e.id,
            amount: e.amount + 1 + rnd.nextInt(300),
          ),
          '$why $when: $author corrects ${e.id}',
        );
        saw('corrected');
      }

      if (rnd.nextBool()) correctOne('before the close');

      // While open: the creator takes somebody off.
      void remove(String who) {
        final f = b.fold();
        final plan = planRemoval(
          folded: f,
          creatorId: f.creatorId,
          log: b.entries,
          id: who,
          me: b.creator,
        );
        expect(
          plan.complete,
          isTrue,
          reason: '$why removing $who: ${plan.blockers.map((x) => x.block)}',
        );
        for (final e in removalEntries(host: b.hosts[b.creator]!, plan: plan)) {
          b.write(e, '$why removing $who (${e['kind']})');
        }
        expect(
          b.fold().bill.participant(who),
          isNull,
          reason: '$why $who is still on the bill',
        );
      }

      if (stray) {
        remove('zed');
        saw('stray removed');
      }
      final paidFor = {for (final e in b.fold().bill.expenses) e.paidBy};
      final removable = [
        for (final p in people.skip(1))
          if (!paidFor.contains(p)) p,
      ];
      if (removable.isNotEmpty && people.length > 3 && rnd.nextInt(3) == 0) {
        final who = removable[rnd.nextInt(removable.length)];
        remove(who);
        people.remove(who);
        saw('sharer removed');
      }

      b.write(
        splitz.setRate(
          host: b.hosts[b.creator]!,
          currency: 'USD',
          minorUnitsPerZec: _rate,
        ),
        '$why rate',
      );
      b.write(splitz.closeFor(b.hosts[b.creator]!, b.fold()), '$why close');
      expect(splitz.settleRefusal(b.fold()), isNull, reason: '$why closed');
      expect(splitz.expenseRefusal(b.fold()), 'bill_closed');

      // Somebody spots a mistake: the creator reopens, the author corrects
      // it, the creator closes again.
      if (rnd.nextBool()) {
        b.write(
          splitz.reopenFor(b.hosts[b.creator]!, b.fold())!,
          '$why reopen',
        );
        expect(splitz.settleRefusal(b.fold()), 'bill_not_closed');
        correctOne('after reopening');
        b.write(
          splitz.closeFor(b.hosts[b.creator]!, b.fold()),
          '$why re-close',
        );
        expect(
          splitz.settleRefusal(b.fold()),
          isNull,
          reason: '$why re-closed',
        );
        saw('reopened');
      }

      var n = 0;
      final payers = [...people]..shuffle(rnd);
      for (final payer in payers) {
        final me = b.hosts[payer]!;
        while (true) {
          final o = splitz.obligationFor(me, b.fold());
          if (o == null) break;
          final apart = [
            for (final u in o.unpayable)
              if (u.reason == 'payout_not_zec') u,
          ];
          final zecTo = o.carriedTo;
          if (zecTo.isEmpty && apart.isEmpty) {
            expect(o.unpayable, isEmpty, reason: '$why $payer stuck');
            break;
          }
          final held = b.fold().bill;
          final swaps = [
            for (final u in apart)
              if (b.lane[u.id] == 'swap') u,
          ];

          // One transaction for every ZEC payee and the one swap (§14.10).
          if (zecTo.isNotEmpty && swaps.length == 1 && rnd.nextInt(4) > 0) {
            final u = swaps.single;
            final quote = _quote(held, u.id, u.minorUnits, n);
            final SwapDeposit sent;
            try {
              sent = combinedSend(
                billId: held.id,
                bill: held,
                obligation: o,
                quote: quote,
                to: u.id,
                amountMinorUnits: u.minorUnits,
                at: _at,
              );
            } on SwapException catch (e) {
              fail('$why $payer combined send refused: ${e.message}');
            } on SwapRefused catch (e) {
              fail('$why $payer combined send refused: $e');
            }
            // Nothing else this wallet built: the word that nothing left
            // stands.
            expect(
              unsentClaimRefusal(sent.note, stillSending: false, own: []),
              isNull,
              reason: '$why unsent claim',
            );
            for (final MapEntry(key: to, value: amount) in zecTo.entries) {
              b.write(
                splitz.recordPayment(
                  host: me,
                  paymentId: 'p${n++}',
                  to: to,
                  amount: amount,
                ),
                '$why $payer pays $to in ZEC (combined)',
              );
            }
            b.write(
              splitz.recordPayment(
                host: me,
                paymentId: 'p${n++}',
                to: u.id,
                amount: u.minorUnits,
                method: 'swap',
                reference: quote.reference,
              ),
              '$why $payer swaps to ${u.id} (combined)',
            );
            saw('combined');
            continue;
          }

          final steps = [if (zecTo.isNotEmpty) 'zec', for (final u in apart) u];
          final step = steps[rnd.nextInt(steps.length)];
          if (step == 'zec') {
            for (final MapEntry(key: to, value: amount) in zecTo.entries) {
              b.write(
                splitz.recordPayment(
                  host: me,
                  paymentId: 'p${n++}',
                  to: to,
                  amount: amount,
                ),
                '$why $payer pays $to in ZEC',
              );
            }
            continue;
          }
          final u = step as core.Unpayable;
          if (b.lane[u.id] == 'swap') {
            final quote = _quote(held, u.id, u.minorUnits, n);
            final refused = swapSendRefusal(
              quote,
              now: _at,
              bill: held,
              obligation: o,
              to: u.id,
              amountMinorUnits: u.minorUnits,
            );
            expect(refused, isNull, reason: '$why swap ${refused?.refused}');
            b.write(
              splitz.recordPayment(
                host: me,
                paymentId: 'p${n++}',
                to: u.id,
                amount: u.minorUnits,
                method: 'swap',
                reference: quote.reference,
              ),
              '$why $payer swaps to ${u.id}',
            );
            continue;
          }
          // Cash, sometimes recorded wrong first and withdrawn.
          if (rnd.nextInt(3) == 0) {
            final wrong = splitz.recordPayment(
              host: me,
              paymentId: 'p${n++}',
              to: u.id,
              amount: u.minorUnits + 100,
              method: 'cash',
            );
            b.write(wrong, '$why $payer records cash wrong');
            final record = b.fold().bill.payments.singleWhere(
              (p) => p.id == (wrong['payment'] as Map)['id'],
            );
            expect(
              ownPaymentWithdrawalRefusal(record, me: payer, state: null),
              isNull,
            );
            b.write(
              splitz.voidEntry(host: me, targetId: wrong['id'] as String),
              '$why $payer withdraws the wrong cash record',
            );
            saw('withdrawn');
            continue;
          }
          b.write(
            splitz.recordPayment(
              host: me,
              paymentId: 'p${n++}',
              to: u.id,
              amount: u.minorUnits,
              method: 'cash',
            ),
            '$why $payer pays ${u.id} cash',
          );
        }
      }

      final f = b.fold();
      for (final p in f.bill.payments) {
        b.write(
          splitz.confirmPayment(
            host: b.hosts[p.to]!,
            paymentId: p.id,
            method: 'recipientConfirmed',
            record: f.paymentDigests[p.id]!,
          ),
          '$why ${p.to} confirms',
        );
      }
      final done = b.fold();
      expect(done.closed, isTrue, reason: '$why reopened by paying');
      expect(done.setAside, isEmpty, reason: why);
      expect(
        core.netBalances(done.bill).values.every((v) => v == 0),
        isTrue,
        reason: '$why not square: ${core.netBalances(done.bill)}',
      );
    }
    // A step the property never took proves nothing about it.
    for (final step in const [
      'corrected',
      'stray removed',
      'sharer removed',
      'reopened',
      'combined',
      'withdrawn',
    ]) {
      expect(seen[step] ?? 0, greaterThan(20), reason: step);
    }
    print(seen);
  });
}
