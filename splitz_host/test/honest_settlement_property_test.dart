/// The complete honest flow, as a property over generated bills: every payer
/// pays every debt by the way its payee asked — ZEC through the request, USDC
/// through a swap, or cash — one at a time in a random order, every payee
/// confirms, and nothing honest is refused on the way.
///
/// Every guard on the payment path is written against somebody paying what
/// they should not. This asks the inverse of all of them at once: does a
/// person paying exactly what they owe ever get turned away? §14.4 once
/// held a payer's remaining debts after they paid some of them, and every
/// vector and audit passed it.
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

void main() {
  test('every honest payment goes through, and the bill ends square', () {
    var bills = 0, payments = 0, swaps = 0, cash = 0, zecSends = 0;
    for (var seed = 1; seed <= 300; seed++) {
      final rnd = Random(seed);
      final people = _names.sublist(0, 3 + rnd.nextInt(4));
      final hosts = {for (final p in people) p: _Host(p)};
      final lane = {
        for (final p in people)
          p: const ['zec', 'swap', 'cash'][rnd.nextInt(3)],
      };
      final entries = <Map<String, dynamic>>[
        splitz.createBill(
          host: hosts[people.first]!,
          name: 'Trip',
          currency: 'USD',
          creatorKey: 'A' * 43,
        ),
        for (final p in people)
          switch (lane[p]) {
            'zec' => splitz.joinBill(host: hosts[p]!, name: p, payTo: _zec),
            'swap' => splitz.joinBill(
              host: hosts[p]!,
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
              host: hosts[p]!,
              name: p,
              payouts: [
                {'type': 'cash'},
              ],
            ),
          },
      ];
      final count = 2 + rnd.nextInt(5);
      for (var i = 0; i < count; i++) {
        final payer = people[rnd.nextInt(people.length)];
        final among = [
          for (final p in people)
            if (p == payer || rnd.nextBool()) p,
        ]..sort();
        entries.add(
          splitz.addExpense(
            host: hosts[payer]!,
            expenseId: 'x$i',
            paidBy: payer,
            amount: 100 + rnd.nextInt(5000),
            split: {'type': 'equal', 'among': among},
          ),
        );
      }
      entries.add(
        splitz.setRate(
          host: hosts[people.first]!,
          currency: 'USD',
          minorUnitsPerZec: _rate,
        ),
      );
      bills++;

      splitz.FoldedBill fold() =>
          splitz.BillLog(hosts[people.first]!, entries: entries).fold();

      // §14.9: nobody pays until the creator closes it, and once closed
      // every honest payment below goes through.
      expect(splitz.settleRefusal(fold()), 'bill_not_closed');
      entries.add(splitz.closeFor(hosts[people.first]!, fold()));
      expect(
        splitz.settleRefusal(fold()),
        isNull,
        reason: 'seed $seed: the creator\'s close did not close it',
      );

      // Each payer, in a random order, pays one lane at a time.
      final payers = [...people]..shuffle(rnd);
      var n = 0;
      for (final payer in payers) {
        final me = hosts[payer]!;
        while (true) {
          final o = splitz.obligationFor(me, fold());
          if (o == null) break;
          final zecTo = [
            for (final s in o.settlements)
              if (!o.unpayable.any((u) => u.id == s.to)) s,
          ];
          final apart = [
            for (final u in o.unpayable)
              if (u.reason == 'payout_not_zec') u,
          ];
          // Nothing left this payer may pay: either all paid, or a debt
          // that is not payable at all, which would be a refusal.
          if (zecTo.isEmpty && apart.isEmpty) {
            expect(
              o.unpayable,
              isEmpty,
              reason:
                  'seed $seed: $payer is owed a lane it cannot use: '
                  '${o.unpayable.map((u) => '${u.id}:${u.reason}')}',
            );
            break;
          }
          // Pick one step: the whole ZEC request, or one swap, or one cash.
          final steps = [if (zecTo.isNotEmpty) 'zec', for (final u in apart) u];
          final step = steps[rnd.nextInt(steps.length)];
          if (step == 'zec') {
            for (final s in zecTo) {
              entries.add(
                splitz.recordPayment(
                  host: me,
                  paymentId: 'p${n++}',
                  to: s.to,
                  amount: s.amount,
                ),
              );
            }
            zecSends++;
            payments += zecTo.length;
            continue;
          }
          final u = step as core.Unpayable;
          final held = fold().bill;
          final payout = held.participant(u.id)!.payouts.first;
          if (payout.type == 'swap') {
            final quote = SwapQuote(
              depositAddress: 't1deposit$n',
              amountInZatoshi: core.fiatToZatoshi(
                u.minorUnits,
                held.rate!,
                amountCurrency: 'USD',
              ),
              amountOut: '1',
              asset: _usdc,
              deadline: '2099-01-01T00:00:00.000Z',
              recipient: payout.address,
              reference: 'swap-$n',
            );
            final refused = swapSendRefusal(
              quote,
              now: '2026-10-28T19:30:00.000Z',
              bill: held,
              obligation: o,
              to: u.id,
              amountMinorUnits: u.minorUnits,
            );
            expect(
              refused,
              isNull,
              reason:
                  'seed $seed: $payer swapping ${u.minorUnits} to '
                  '${u.id} was refused: ${refused?.refused}',
            );
            entries.add(
              splitz.recordPayment(
                host: me,
                paymentId: 'p${n++}',
                to: u.id,
                amount: u.minorUnits,
                method: 'swap',
                reference: quote.reference,
              ),
            );
            swaps++;
          } else {
            entries.add(
              splitz.recordPayment(
                host: me,
                paymentId: 'p${n++}',
                to: u.id,
                amount: u.minorUnits,
                method: 'cash',
              ),
            );
            cash++;
          }
          payments++;
        }
      }

      // Every payee says it arrived.
      final f = fold();
      for (final p in f.bill.payments) {
        entries.add(
          splitz.confirmPayment(
            host: hosts[p.to]!,
            paymentId: p.id,
            method: 'recipientConfirmed',
            record: f.paymentDigests[p.id]!,
          ),
        );
      }
      final done = fold();
      expect(
        done.closed,
        isTrue,
        reason: 'seed $seed: paying and confirming reopened the bill',
      );
      expect(
        done.setAside,
        isEmpty,
        reason:
            'seed $seed: refused ${[for (final r in done.setAside) '${r.code} ${entries.firstWhere((e) => e['id'] == r.id)['kind']}']}',
      );
      expect(
        core.netBalances(done.bill).values.every((v) => v == 0),
        isTrue,
        reason: 'seed $seed: not square: ${core.netBalances(done.bill)}',
      );
    }
    // A property that never paid anything proves nothing.
    expect(swaps, greaterThan(50));
    expect(cash, greaterThan(50));
    expect(zecSends, greaterThan(50));
    print(
      '$bills bills: $payments payments, $zecSends ZEC sends, '
      '$swaps swaps, $cash cash',
    );
  });
}
