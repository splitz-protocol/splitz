import 'package:splitz_core/splitz_core.dart' as splitz;
import 'package:test/test.dart';
import 'package:splitz_core/host.dart';

import 'support/fake_host.dart';

/// Ana pays [amount] on a bill in [currency] named [name], split evenly with
/// Ben, so Ben owes Ana half.
({BillLog log, FakeHost ana, FakeHost ben}) _bill(
  String name, {
  String currency = 'EUR',
  int amount = 9000,
}) {
  final ana = FakeHost(me: 'ana', payToAddress: 'u1ana');
  final ben = FakeHost(me: 'ben', payToAddress: 'u1ben');
  final create = createBill(
    host: ana,
    name: name,
    currency: currency,
    creatorKey: fakeKey('ana'),
  );
  ana.tick();
  final joinAna = joinBill(host: ana, name: 'Ana', payTo: 'u1ana');
  ben.tick();
  ben.tick();
  final joinBen = joinBill(host: ben, name: 'Ben', payTo: 'u1ben');
  ana.tick();
  final expense = addExpense(
    host: ana,
    expenseId: 'x1',
    paidBy: 'ana',
    amount: amount,
    split: const {
      'type': 'equal',
      'among': ['ana', 'ben'],
    },
  );
  final log = BillLog(ana);
  expect(log.add([create, joinAna, joinBen, expense]), isEmpty);
  ben.tick();
  ben.tick();
  return (log: log, ana: ana, ben: ben);
}

void _record(
  ({BillLog log, FakeHost ana, FakeHost ben}) b,
  String id,
  int amount,
) {
  expect(
    b.log.add([
      recordPayment(
        host: b.ben,
        paymentId: id,
        to: 'ana',
        amount: amount,
        method: 'cash',
      ),
    ]),
    isEmpty,
  );
}

void main() {
  test('one person across two bills is one standing', () {
    final x = _bill('X');
    final y = _bill('Y');
    final bills = [x.log.fold(), y.log.fold()];

    final forAna = totalsAcross(bills, 'ana');
    expect(forAna.uncounted, isEmpty);
    final withBen = forAna.standings.single;
    expect(withBen.withId, 'ben');
    expect(withBen.currency, 'EUR');
    expect(withBen.owedToMe, 9000);
    expect(withBen.owedByMe, 0);
    expect(withBen.net, 9000);
    expect(withBen.billIds, hasLength(2));

    final withAna = totalsAcross(bills, 'ben').standings.single;
    expect(withAna.owedByMe, 9000);
    expect(withAna.net, -9000);
  });

  test('two currencies are two standings, never one sum', () {
    final bills = [
      _bill('X').log.fold(),
      _bill('Y', currency: 'USD').log.fold(),
    ];
    final standings = totalsAcross(bills, 'ana').standings;
    expect(standings.map((s) => '${s.currency} ${s.owedToMe}'), [
      'EUR 4500',
      'USD 4500',
    ]);
  });

  test('a payment recorded and not confirmed is on its way, still owed', () {
    final x = _bill('X');
    _record(x, 'p1', 4500);
    final bills = [x.log.fold()];
    final forBen = totalsAcross(bills, 'ben').standings.single;
    expect(forBen.owedByMe, 4500, reason: 'a record moves nothing (§10.5)');
    expect(forBen.sentAwaiting, 4500);
    final forAna = totalsAcross(bills, 'ana').standings.single;
    expect(forAna.receivedAwaiting, 4500);
  });

  test('a confirmed payment is neither owed nor awaiting', () {
    final x = _bill('X');
    _record(x, 'p1', 4500);
    x.ana.tick();
    expect(
      x.log.add([
        confirmPayment(
          host: x.ana,
          paymentId: 'ben:p1',
          method: 'recipientConfirmed',
          record: x.log.fold().paymentDigests['ben:p1']!,
        ),
      ]),
      isEmpty,
    );
    expect(totalsAcross([x.log.fold()], 'ana').standings, isEmpty);
  });

  test('a bill that would carry a sum past 64 bits is left out whole', () {
    // Payments carry no cap (§2.2), so two unconfirmed records of 2^63 - 1
    // on two bills cannot both be counted.
    const most = 9223372036854775807;
    final x = _bill('X');
    final y = _bill('Y');
    _record(x, 'p1', most);
    _record(y, 'p1', most);
    final fx = x.log.fold();
    final fy = y.log.fold();
    final totals = totalsAcross([fx, fy], 'ana');
    expect(totals.uncounted.values, ['amount_overflow']);
    final kept = totals.standings.single;
    expect(kept.receivedAwaiting, most);
    expect(kept.owedToMe, 4500, reason: 'the left-out bill adds nothing');
    expect(kept.billIds, hasLength(1));
  });

  test('somebody on no bill with this device is not a standing', () {
    expect(totalsAcross([_bill('X').log.fold()], 'cat').standings, isEmpty);
  });

  /// [f] with [bound] as the ids §10.7 bound, as a verifying fold reports it.
  FoldedBill boundTo(FoldedBill f, Set<String> bound) => FoldedBill(
        bill: f.bill,
        setAside: f.setAside,
        withdrawn: f.withdrawn,
        replacedAddresses: f.replacedAddresses,
        identities: splitz.Identities({for (final id in bound) id: 'key-$id'}),
        paymentAuthors: f.paymentAuthors,
        paymentDigests: f.paymentDigests,
      );

  test('an id bound on one bill and not on another is two standings', () {
    // On X Ben is bound and owes Ana 45.00. On Y an unsigned join under Ben's
    // id is owed 45.00 by Ana: nothing says it is the same person.
    final x = boundTo(_bill('X').log.fold(), {'ana', 'ben'});
    final y = _bill('Y');
    final flipped = y.log.fold();
    final standings = totalsAcross([x, flipped], 'ana').standings;
    expect(standings.map((s) => (s.withId, s.owedToMe, s.billIds.length)), [
      ('ben', 4500, 1),
      ('ben', 4500, 1),
    ]);
    // Bound on both, one person: one standing.
    final both = totalsAcross(
      [
        x,
        boundTo(flipped, {'ana', 'ben'})
      ],
      'ana',
    ).standings;
    expect(both.single.owedToMe, 9000);
  });

  test('an id bound to two keys on two bills is two standings', () {
    // Y's creator binds `ben` to a key of their own: the same string, and
    // somebody else.
    final x = boundTo(_bill('X').log.fold(), {'ana', 'ben'});
    final y = _bill('Y').log.fold();
    final other = FoldedBill(
      bill: y.bill,
      setAside: y.setAside,
      withdrawn: y.withdrawn,
      replacedAddresses: y.replacedAddresses,
      identities: const splitz.Identities({'ana': 'key-ana', 'ben': 'key-mal'}),
      paymentAuthors: y.paymentAuthors,
      paymentDigests: y.paymentDigests,
    );
    final standings = totalsAcross([x, other], 'ana').standings;
    expect(standings.map((s) => (s.withId, s.owedToMe, s.billIds.length)), [
      ('ben', 4500, 1),
      ('ben', 4500, 1),
    ]);
  });

  test("a record the payee wrote in the payer's name is not on its way", () {
    final x = _bill('X');
    x.ana.tick();
    // Ana, the payee, records Ben paying her: §10.4 lets either party write
    // a payment, but §14.4 counts as sent only what its payer recorded.
    final written =
        recordPayment(host: x.ana, paymentId: 'p1', to: 'ana', amount: 4500);
    final inBensName = <String, dynamic>{
      ...written,
      'payment': <String, dynamic>{
        ...written['payment'] as Map<String, dynamic>,
        'from': 'ben',
      },
    };
    inBensName['id'] = splitz.deriveEntryId(inBensName);
    expect(x.log.add([inBensName]), isEmpty);
    final forBen = totalsAcross([x.log.fold()], 'ben').standings.single;
    expect(forBen.sentAwaiting, 0);
    expect(forBen.owedByMe, 4500);
    // Ben's own record of the same payment is on its way.
    _record(x, 'p2', 4500);
    expect(totalsAcross([x.log.fold()], 'ben').standings.single.sentAwaiting,
        4500);
  });
}
