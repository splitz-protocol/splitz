/// The three ways a debt settles: a Zcash output, a swap off this chain, cash.
///
/// §9.2 makes the method a label rather than a branch — the ledger arithmetic
/// is identical whichever happened — so what these assert is not different
/// money but different *routing*, and the different things a reader is
/// allowed to conclude from each.
library;

import 'package:test/test.dart';
import 'package:splitz_core/splitz_core.dart' as splitz;
import 'package:splitz_core/host.dart';

import 'support/fake_host.dart';

/// A bill where each payee declares a different payout preference.
///
/// Ana owes everybody: Ben wants ZEC, Cara wants USDC on Base, Dan wants cash,
/// and Eve has declared nothing at all.
({BillLog log, FoldedBill folded, FakeHost ana}) threeLaneBill({
  List<Map<String, dynamic>> extra = const [],
  List<Map<String, dynamic>> caraAlso = const [],
}) {
  final ana = FakeHost(me: 'ana', payToAddress: 'u1ana');
  final entries = <Map<String, dynamic>>[
    createBill(
        host: ana, name: 'Dinner', currency: 'USD', creatorKey: fakeKey('ana')),
    joinBill(host: ana, name: 'Ana', payTo: 'u1ana'),
    joinBill(
      host: FakeHost(me: 'ben'),
      name: 'Ben',
      payouts: [
        <String, dynamic>{'type': 'zec', 'address': 'u1ben'},
      ],
    ),
    joinBill(
      host: FakeHost(me: 'cara'),
      name: 'Cara',
      payouts: [
        <String, dynamic>{
          'type': 'swap',
          'asset': 'USDC',
          'chain': 'base',
          'address': '0xcara',
        },
        ...caraAlso,
      ],
    ),
    joinBill(
      host: FakeHost(me: 'dan'),
      name: 'Dan',
      payouts: [
        <String, dynamic>{'type': 'cash'},
      ],
    ),
    joinBill(host: FakeHost(me: 'eve'), name: 'Eve'),
    // Ana owes each of the four 10.00, by having each of them cover a cost
    // split between the two of them.
    for (final who in ['ben', 'cara', 'dan', 'eve'])
      addExpense(
        host: FakeHost(me: who),
        expenseId: 'x-$who',
        paidBy: who,
        amount: 2000,
        split: <String, dynamic>{
          'type': 'equal',
          'among': ['ana', who]..sort(),
        },
      ),
    setRate(
        host: ana,
        currency: 'USD',
        minorUnitsPerZec: 100000,
        source: 'fixed for this test'),
    ...extra,
  ];
  final log = BillLog(ana, entries: entries);
  return (log: log, folded: log.fold(), ana: ana);
}

void main() {
  group('a payout preference chooses a lane', () {
    test('each of the four payees lands in the lane they asked for', () {
      final bill = threeLaneBill().folded.bill;
      SettleLane lane(String id) => laneFor(bill.participant(id)!);

      expect(lane('ben'), SettleLane.zec);
      expect(lane('cara'), SettleLane.swap);
      expect(lane('dan'), SettleLane.cash);
      // Declared nothing and has no payTo: not a lane, a debt that cannot be
      // settled until she publishes somewhere.
      expect(lane('eve'), SettleLane.none);
    });

    test('a swap debt carries the asset AND the chain, never one alone', () {
      final cara = threeLaneBill().folded.bill.participant('cara')!;
      expect(laneFor(cara), SettleLane.swap);
      final swap = cara.payouts.first;

      expect(swap.asset, 'USDC');
      expect(swap.chain, 'base');
      expect(swap.address, '0xcara');
    });

    test('the first preference decides, not the most convenient one', () {
      // Cara ranks cash first and a Zcash address second. A reader that fell
      // through to the address because it is the one it can batch would pay
      // her somewhere she ranked lower.
      final cara = splitz.Participant(
        id: 'cara',
        name: 'Cara',
        payouts: const [
          splitz.Payout(type: 'cash'),
          splitz.Payout(type: 'zec', address: 'u1cara'),
        ],
      );
      expect(laneFor(cara), SettleLane.cash);
    });

    test('a participant with only payTo is a zec lane', () {
      expect(
        laneFor(
            const splitz.Participant(id: 'ben', name: 'Ben', payTo: 'u1ben')),
        SettleLane.zec,
      );
    });

    test('a zec payout with an empty address is nobody to pay', () {
      // An empty string is not an address; answering `zec` would send it into
      // the renderer, which refuses the whole request.
      expect(
        laneFor(const splitz.Participant(
          id: 'ben',
          name: 'Ben',
          payouts: [splitz.Payout(type: 'zec', address: '')],
        )),
        SettleLane.none,
      );
    });

    test('a swap payout missing its address, asset or chain is nobody to pay',
        () {
      SettleLane swap({String? asset, String? chain, String? address}) =>
          laneFor(splitz.Participant(id: 'cara', name: 'Cara', payouts: [
            splitz.Payout(
                type: 'swap', asset: asset, chain: chain, address: address),
          ]));
      expect(swap(asset: 'USDC', chain: 'near', address: ''), SettleLane.none);
      expect(
          swap(asset: 'USDC', chain: 'near', address: '  '), SettleLane.none);
      expect(swap(asset: 'USDC', chain: 'near'), SettleLane.none);
      expect(swap(asset: 'USDC', address: 'cai.near'), SettleLane.none);
      expect(swap(chain: 'near', address: 'cai.near'), SettleLane.none);
      expect(swap(asset: 'USDC', chain: 'near', address: 'cai.near'),
          SettleLane.swap);
    });
  });

  group('the request carries the zec lane and reports the rest (§8.5)', () {
    test('only Ben is in the URI; the other three are reported', () {
      final bill = threeLaneBill();
      final owed = obligationFor(bill.ana, bill.folded)!;

      // `settlements` is the whole debt this device owes; the URI carries
      // only what §8.5 could render. A screen that reads the first as the
      // second tells the payer they have paid everybody.
      expect(owed.settlements.map((s) => s.to), ['ben', 'cara', 'dan', 'eve']);
      expect(owed.request.payments.map((p) => p.label), ['Ben']);
      expect(owed.uri, contains('u1ben'));
      expect(owed.uri, isNot(contains('0xcara')));

      final reasons = {
        for (final u in owed.unpayable) u.id: u.reason,
      };
      // A swap and a cash payout are excluded for a reason that has nothing
      // to do with a missing address, and §8.5 requires the difference be
      // reported rather than flattened.
      expect(reasons['cara'], 'payout_not_zec');
      expect(reasons['dan'], 'payout_not_zec');
      expect(reasons['eve'], 'no_address');
    });

    test('the carried total is not the obligation, and says so', () {
      final bill = threeLaneBill();
      final owed = obligationFor(bill.ana, bill.folded)!;

      expect(owed.carriedMinorUnits, 1000); // Ben only
      expect(owed.withheldMinorUnits, 3000); // Cara, Dan, Eve
      expect(owed.isComplete, isFalse);
    });
  });

  group('a payer may settle by a lower preference (§14.8)', () {
    // Cara ranks USDC on Base first and a Zcash address second.
    ({BillLog log, FoldedBill folded, FakeHost ana}) bill() => threeLaneBill(
          caraAlso: [
            <String, dynamic>{'type': 'zec', 'address': 'u1cara'},
          ],
        );

    test('choosing her second payout carries it in the request', () {
      final b = bill();
      final first = obligationFor(b.ana, b.folded)!;
      final chosen = obligationVia(b.ana, b.folded, const {'cara': 1})!;

      // Who owes what does not move; only where Cara's share is sent does.
      expect(
        chosen.settlements.map((s) => (s.to, s.amount)),
        first.settlements.map((s) => (s.to, s.amount)),
      );
      expect(first.request.payments.map((p) => p.label), ['Ben']);
      expect(chosen.request.payments.map((p) => p.label), ['Ben', 'Cara']);
      expect(chosen.request.payments.last.address, 'u1cara');
      expect(chosen.unpayable.map((u) => u.id), ['dan', 'eve']);
      expect(chosen.carriedMinorUnits, 2000);
    });

    test('the send records her payment like any other, and her order stands',
        () async {
      final b = bill();
      final owed = obligationVia(b.ana, b.folded, const {'cara': 1})!;
      final settled = await settle(b.ana, b.log, owed);

      expect(settled.result, SendResult.sent);
      final cara = settled.records
          .map((r) => r['payment'] as Map<String, dynamic>)
          .singleWhere((p) => p['to'] == 'cara');
      expect(cara['method'], 'shieldedZec');
      expect(cara['id'], paymentIdForSend(b.ana.me, settled.txid!, 'cara'));
      expect(cara['reference'], settled.txid);
      // No entry rewrote her preferences: every device still reads USDC first.
      final after = b.log.fold().bill.participant('cara')!;
      expect(after.payouts.map((p) => p.type), ['swap', 'zec']);
    });

    test('a payout she never declared is refused', () {
      final b = bill();
      expect(
        () => obligationVia(b.ana, b.folded, const {'cara': 2}),
        throwsA(isA<splitz.SplitError>()
            .having((e) => e.code, 'code', splitz.SplitCode.payoutNotDeclared)),
      );
    });
  });

  group('recording what happened', () {
    test('a zec settlement records shieldedZec and the txid', () async {
      final bill = threeLaneBill();
      final owed = obligationFor(bill.ana, bill.folded)!;
      final settled = await settle(bill.ana, bill.log, owed);

      expect(settled.result, SendResult.sent);
      final payment = settled.records.single['payment'] as Map<String, dynamic>;
      expect(payment['method'], 'shieldedZec');
      expect(
          payment['id'], paymentIdForSend(bill.ana.me, settled.txid!, 'ben'));
      expect(payment['to'], 'ben');
      // §10.5: the record carries its own id and the transaction is the
      // reference, which is what an `onChain` confirmation is checked against.
      expect(payment['reference'], settled.txid);
    });

    test('a cash settlement records cash, sends nothing, and is folded', () {
      final bill = threeLaneBill();
      final record = recordPayment(
        host: bill.ana,
        paymentId: 'cash-dan-1',
        to: 'dan',
        amount: 1000,
        method: 'cash',
        note: 'handed over at the table',
      );
      bill.log.add([record]);

      final payment = record['payment'] as Map<String, dynamic>;
      expect(payment['method'], 'cash');
      expect(payment['note'], 'handed over at the table');
      // No transaction exists, so no reference pretends one does.
      expect(payment.containsKey('reference'), isFalse);

      // It is on the bill, and §10.5 has not settled it: a record is a claim.
      final folded = bill.log.fold();
      expect(folded.bill.payments.map((p) => p.id), contains('ana:cash-dan-1'));
      expect(folded.bill.confirmedPayments, isNot(contains('ana:cash-dan-1')));
    });

    test('a swap settlement records the intent id, not a txid', () {
      final bill = threeLaneBill();
      // The swap's own identifier is the payment id as well as the reference:
      // it is what a reader checks the record against.
      final record = recordPayment(
        host: bill.ana,
        paymentId: 'near-intent-7f3a',
        to: 'cara',
        amount: 1000,
        method: 'swap',
        reference: 'near-intent-7f3a',
        zatoshi: 1000000,
        note: 'USDC on base',
      );
      bill.log.add([record]);

      final payment = record['payment'] as Map<String, dynamic>;
      expect(payment['method'], 'swap');
      // §9.2: the reference identifies the swap. A reader that renders it as
      // a Zcash transaction is wrong for every swap.
      expect(payment['reference'], 'near-intent-7f3a');
      expect(payment['id'], 'ana:near-intent-7f3a');
      // Verifiable only in half: the ZEC leg is recorded, the delivery is not.
      expect(payment['zatoshi'], 1000000);
      expect(payment['note'], 'USDC on base');

      final folded = bill.log.fold();
      final paid = folded.bill.payments
          .firstWhere((p) => p.id == 'ana:near-intent-7f3a');
      expect(paid.method, 'swap');
      expect(paid.reference, 'near-intent-7f3a');
      expect(paid.zatoshi, 1000000);
      // The ZEC leg leaving is not the recipient being paid. Only Cara can
      // say that, and she has not.
      expect(folded.bill.confirmedPayments,
          isNot(contains('ana:near-intent-7f3a')));
    });

    test('all three methods coexist on one bill and net the same way', () {
      final bill = threeLaneBill();
      bill.log.add([
        recordPayment(
            host: bill.ana,
            paymentId: 'cash-dan-1',
            to: 'dan',
            amount: 1000,
            method: 'cash'),
        recordPayment(
            host: bill.ana,
            paymentId: 'near-intent-7f3a',
            to: 'cara',
            amount: 1000,
            method: 'swap',
            reference: 'near-intent-7f3a'),
      ]);

      final folded = bill.log.fold();
      expect(folded.setAside, isEmpty);
      expect(
        folded.bill.payments.map((p) => p.method).toSet(),
        {'cash', 'swap'},
      );
      // §9.2: a label, not a branch. Neither payment has moved a balance,
      // because neither is confirmed — the arithmetic does not care which
      // method it was.
      final debts = splitz.netBalances(folded.bill);
      expect(debts['ana'], -4000);
    });
  });

  group('what these paths refuse', () {
    test('a method the protocol does not define is set aside at the fold', () {
      // §10.1 admits the entry — it is well formed — and §10.3 sets it aside
      // when the bill is read. The debt stays owed rather than the whole log
      // becoming unreadable because one peer invented a method.
      final bill = threeLaneBill();
      bill.log.add([
        recordPayment(
          host: bill.ana,
          paymentId: 'p1',
          to: 'ben',
          amount: 100,
          method: 'venmo',
        )
      ]);
      final folded = bill.log.fold();
      expect(
        folded.setAside.map((s) => s.code),
        contains(splitz.SplitCode.billUnknownSettlementMethod),
      );
      expect(folded.bill.payments.map((p) => p.id), isNot(contains('p1')));
    });

    test('a payout type the protocol does not define is refused', () {
      final bill = threeLaneBill(extra: [
        joinBill(
          host: FakeHost(me: 'fay'),
          name: 'Fay',
          payouts: [
            <String, dynamic>{'type': 'venmo', 'address': '@fay'},
          ],
        ),
      ]);
      // Refused rather than skipped: skipping settles to the next preference
      // down, which is a different address.
      expect(
        bill.folded.setAside.map((s) => s.code),
        contains(splitz.SplitCode.billUnknownPayoutMethod),
      );
    });

    test('a payment to oneself is set aside whichever method it claims', () {
      for (final method in ['shieldedZec', 'swap', 'cash']) {
        final bill = threeLaneBill();
        bill.log.add([
          recordPayment(
            host: bill.ana,
            paymentId: 'p-$method',
            to: 'ana',
            amount: 100,
            method: method,
            reference: 'r-$method',
          )
        ]);
        expect(
          bill.log.fold().setAside.map((s) => s.code),
          contains(splitz.SplitCode.selfPayment),
          reason: 'a $method payment to oneself pads a settlement history',
        );
      }
    });

    test('a payout nobody could be paid by is not written', () {
      final ana = FakeHost(me: 'ana');
      Map<String, dynamic> join(List<Map<String, dynamic>> payouts) =>
          joinBill(host: ana, name: 'Ana', payouts: payouts);
      for (final bad in [
        {'type': 'zec'},
        {'type': 'zec', 'address': '  '},
        {'type': 'swap', 'asset': 'USDC', 'address': '0xa'},
        {'type': 'swap', 'asset': '', 'chain': 'base', 'address': '0xa'},
        {'type': 'swap', 'asset': 'USDC', 'chain': 'base', 'address': 7},
      ]) {
        expect(
          () => join([
            {'type': 'cash'},
            bad,
          ]),
          throwsA(
            isA<splitz.SplitError>().having(
              (e) => e.code,
              'code',
              splitz.SplitCode.billTypeError,
            ),
          ),
          reason: '$bad',
        );
      }
      final honest = [
        {'type': 'zec', 'address': 'u1ana'},
        {'type': 'swap', 'asset': 'USDC', 'chain': 'base', 'address': '0xa'},
        {'type': 'cash'},
      ];
      expect(join(honest)['participant']['payouts'], honest);
      // A type §9.1 does not define is left for every reader to refuse.
      expect(
        join([
          {'type': 'venmo'},
        ])['participant']['payouts'],
        [
          {'type': 'venmo'},
        ],
      );
    });

    test('a payment of nothing, or a swap naming none, is not written', () {
      final bill = threeLaneBill();
      Matcher refusedWith(String code) =>
          throwsA(isA<splitz.SplitError>().having((e) => e.code, 'code', code));
      for (final amount in [0, -1]) {
        expect(
          () => recordPayment(
            host: bill.ana,
            paymentId: 'p$amount',
            to: 'ben',
            amount: amount,
            method: 'cash',
          ),
          refusedWith(splitz.SplitCode.negativeAmount),
          reason: '$amount',
        );
      }
      for (final reference in [null, '', '  ']) {
        expect(
          () => recordPayment(
            host: bill.ana,
            paymentId: 's',
            to: 'ben',
            amount: 100,
            method: 'swap',
            reference: reference,
          ),
          refusedWith(splitz.SplitCode.billTypeError),
          reason: '$reference',
        );
      }
      // One unit, a swap that names its intent, and cash naming nothing are
      // all honest records.
      expect(
        recordPayment(
          host: bill.ana,
          paymentId: 'one',
          to: 'ben',
          amount: 1,
          method: 'cash',
        )['payment']['amount'],
        1,
      );
      expect(
        recordPayment(
          host: bill.ana,
          paymentId: 'sw',
          to: 'ben',
          amount: 100,
          method: 'swap',
          reference: 'intent-1',
        )['payment']['reference'],
        'intent-1',
      );
    });

    test('a zatoshi leg of zero is set aside', () {
      // A swap that sent nothing is not a swap. §9.2 makes zatoshi advisory,
      // which is not the same as unchecked.
      final bill = threeLaneBill();
      bill.log.add([
        recordPayment(
          host: bill.ana,
          paymentId: 'p1',
          to: 'ben',
          amount: 100,
          method: 'swap',
          reference: 'p1',
          zatoshi: 0,
        )
      ]);
      expect(
        bill.log.fold().setAside.map((s) => s.code),
        contains(splitz.SplitCode.negativeAmount),
      );
    });

    test('a settle records only what the request carried', () {
      // Dan wants cash, so §8.5 leaves him out of the URI. Recording him as
      // paid by that transaction claims it settled a debt it never paid, and
      // leaves Dan contesting a payment rather than simply still being owed.
      final bill = threeLaneBill();
      final owed = obligationFor(bill.ana, bill.folded)!;
      expect(owed.settlements.map((s) => s.to),
          containsAll(<String>['ben', 'cara', 'dan', 'eve']));

      return settle(bill.ana, bill.log, owed).then((settled) {
        expect(
            settled.records
                .map((r) => (r['payment'] as Map<String, dynamic>)['to']),
            ['ben']);
      });
    });

    test('a request that can carry nothing sends nothing', () async {
      // Everybody on this bill wants cash. There is no URI to broadcast, and
      // nothing may be recorded as sent.
      final ana = FakeHost(me: 'ana', payToAddress: 'u1ana');
      final log = BillLog(ana, entries: [
        createBill(
            host: ana, name: 'D', currency: 'USD', creatorKey: fakeKey('ana')),
        joinBill(host: ana, name: 'Ana', payTo: 'u1ana'),
        joinBill(
          host: FakeHost(me: 'dan'),
          name: 'Dan',
          payouts: [
            <String, dynamic>{'type': 'cash'},
          ],
        ),
        addExpense(
          host: FakeHost(me: 'dan'),
          expenseId: 'x-dan',
          paidBy: 'dan',
          amount: 2000,
          split: <String, dynamic>{
            'type': 'equal',
            'among': ['ana', 'dan'],
          },
        ),
        setRate(host: ana, currency: 'USD', minorUnitsPerZec: 100000),
      ]);
      final owed = obligationFor(ana, log.fold())!;
      expect(owed.uri, isNull);

      final settled = await settle(ana, log, owed);
      expect(settled.result, SendResult.failed);
      expect(settled.records, isEmpty);
      expect(log.fold().bill.payments, isEmpty);
    });
  });
}
