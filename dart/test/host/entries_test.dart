import 'package:test/test.dart';
import 'package:splitz_core/splitz_core.dart' as splitz;
import 'package:splitz_core/host.dart';

import 'support/fake_host.dart';

void main() {
  group('a bill begins', () {
    test('a createBill entry opens one, and its id is the bill', () {
      final ana = FakeHost(me: 'ana');
      final create = createBill(
        host: ana,
        name: 'Dinner',
        currency: 'EUR',
        creatorKey: fakeKey('ana'),
      );

      // §9.4: the bill's id IS the digest of the entry that opens it, so a
      // wallet cannot choose one and two wallets cannot disagree about it.
      expect(create['id'], splitz.deriveBillId(create));
      expect(() => splitz.checkEntry(create), returnsNormally);

      final log = BillLog(ana, entries: [create]);
      expect(log.opensABill, isTrue);
      expect(log.fold().bill.id, create['id']);
    });

    test('two bills opened at one instant by one person are two bills', () {
      final ana = FakeHost(me: 'ana');
      final first = createBill(
          host: ana, name: 'Dinner', currency: 'EUR', creatorKey: fakeKey('a'));
      final second = createBill(
          host: ana, name: 'Dinner', currency: 'EUR', creatorKey: fakeKey('a'));

      // Same author, same name, same clock — only the nonce differs, which is
      // what §9.4 puts it there for.
      expect(first['at'], second['at']);
      expect(first['id'], isNot(second['id']));
    });

    test('every entry kind this package writes passes ingress', () {
      final ana = FakeHost(me: 'ana');
      final built = <String, Map<String, dynamic>>{
        'createBill': createBill(
            host: ana,
            name: 'Dinner',
            currency: 'EUR',
            creatorKey: fakeKey('ana')),
        'joinBill': joinBill(host: ana, name: 'Ana', payTo: 'u1ana'),
        'addExpense': addExpense(
          host: ana,
          expenseId: 'x1',
          paidBy: 'ana',
          amount: 9000,
          split: const {
            'type': 'equal',
            'among': ['ana', 'ben'],
          },
        ),
        'recordPayment':
            recordPayment(host: ana, paymentId: 'tx1', to: 'ben', amount: 4500),
        'confirmPayment': confirmPayment(
            host: ana,
            paymentId: 'tx1',
            method: 'onChain',
            reference: 'tx:1',
            record: 'r'),
        'setRate': setRate(host: ana, currency: 'EUR', minorUnitsPerZec: 51234),
        'voidEntry': voidEntry(host: ana, targetId: 'whatever'),
      };

      for (final entry in built.entries) {
        expect(() => splitz.checkEntry(entry.value), returnsNormally,
            reason: '${entry.key} must pass §10.1 as written');
      }
    });
  });

  test('an id is written under its author once, whatever the author holds', () {
    expect(authoredId('ana', 'hotel'), 'ana:hotel');
    expect(authoredId('ana', 'ana:hotel'), 'ana:hotel');
    // §10.3 step 5 gives an author holding `:` no minted ids; the builder
    // still writes the same id for one, in both implementations.
    expect(authoredId('ben:t1', 'ben:t1:ana'), 'ben:t1:ana');
    expect(authoredId('ben:t1', 'ana'), 'ben:t1:ana');
  });
}
