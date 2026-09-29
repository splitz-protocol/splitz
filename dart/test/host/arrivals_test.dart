import 'package:test/test.dart';
import 'package:splitz_core/splitz_core.dart' as splitz;
import 'package:splitz_core/host.dart';

import 'support/fake_host.dart';

const _t1 = 'aa00000000000000000000000000000000000000000000000000000000000001';
const _t2 = 'bb00000000000000000000000000000000000000000000000000000000000002';

/// Ana and Ben on one bill named [name]; Ben owes Ana.
({BillLog log, FakeHost ana, FakeHost ben}) _bill(String name) {
  final ana = FakeHost(me: 'ana', payToAddress: 'u1ana');
  final ben = FakeHost(me: 'ben', payToAddress: 'u1ben');
  final create = createBill(
      host: ana, name: name, currency: 'EUR', creatorKey: fakeKey('ana'));
  ana.tick();
  final joinAna = joinBill(host: ana, name: 'Ana', payTo: 'u1ana');
  ben.tick();
  ben.tick();
  final joinBen = joinBill(host: ben, name: 'Ben', payTo: 'u1ben');
  final log = BillLog(ana);
  expect(log.add([create, joinAna, joinBen]), isEmpty);
  ben.tick();
  ben.tick();
  return (log: log, ana: ana, ben: ben);
}

/// Ben records paying Ana [amount] euro cents as [zatoshi] in transaction
/// [txid], under [paymentId].
void _paid(
  ({BillLog log, FakeHost ana, FakeHost ben}) b,
  String paymentId,
  String txid, {
  int amount = 1000,
  int? zatoshi = 20000,
  String method = 'shieldedZec',
  String to = 'ana',
}) {
  expect(
    b.log.add([
      recordPayment(
        host: b.ben,
        paymentId: paymentId,
        to: to,
        amount: amount,
        method: method,
        reference: txid,
        zatoshi: zatoshi,
      ),
    ]),
    isEmpty,
  );
}

void _confirm(({BillLog log, FakeHost ana, FakeHost ben}) b, String id) {
  b.ana.tick();
  expect(
    b.log.add([
      confirmPayment(
        host: b.ana,
        paymentId: id,
        method: 'recipientConfirmed',
        record: b.log.fold().paymentDigests[id]!,
      ),
    ]),
    isEmpty,
  );
}

List<String> _ids(List<Arrival> arrivals) =>
    [for (final a in arrivals) '${a.billId}/${a.payment.id}'];

void main() {
  test('a record whose transaction arrived with its ZEC is proposed', () {
    final b = _bill('Dinner');
    _paid(b, 'p1', _t1);
    final found = arrivalsFor(
      [b.log.fold()],
      'ana',
      const [IncomingTransaction(_t1, 20000)],
    );
    expect(found.arrived.map((a) => a.payment.id), ['p1']);
    expect(found.arrived.single.txid, _t1);
    expect(found.arrived.single.record, b.log.fold().paymentDigests['p1']);
    expect(found.short, isEmpty);
    expect(found.unstated, isEmpty);
  });

  test('the proposed confirmation is one the fold applies', () {
    final b = _bill('Dinner');
    _paid(b, 'p1', _t1);
    final a = arrivalsFor(
      [b.log.fold()],
      'ana',
      const [IncomingTransaction(_t1, 20000)],
    ).arrived.single;
    b.ana.tick();
    final refused = b.log.add([
      confirmPayment(
        host: b.ana,
        paymentId: a.payment.id,
        method: 'walletReceived',
        record: a.record,
        reference: a.txid,
      ),
    ]);
    expect(refused, isEmpty);
    expect(b.log.fold().bill.confirmedPayments, {'p1'});
  });

  test('a transaction id is matched whatever its case and padding', () {
    final b = _bill('Dinner');
    _paid(b, 'p1', _t1.toUpperCase());
    final found = arrivalsFor(
      [b.log.fold()],
      'ana',
      [IncomingTransaction(' $_t1 ', 20000)],
    );
    expect(found.arrived, hasLength(1));
  });

  test('a record claiming more ZEC than arrived is short', () {
    final b = _bill('Dinner');
    _paid(b, 'p1', _t1, zatoshi: 20001);
    final found = arrivalsFor(
      [b.log.fold()],
      'ana',
      const [IncomingTransaction(_t1, 20000)],
    );
    expect(found.arrived, isEmpty);
    expect(found.short.map((a) => a.payment.id), ['p1']);
  });

  test('a record stating no ZEC cannot be checked', () {
    final b = _bill('Dinner');
    _paid(b, 'p1', _t1, zatoshi: null);
    final found = arrivalsFor(
      [b.log.fold()],
      'ana',
      const [IncomingTransaction(_t1, 20000)],
    );
    expect(found.arrived, isEmpty);
    expect(found.unstated.map((a) => a.payment.id), ['p1']);
  });

  test('one transaction is evidence once, across bills', () {
    final x = _bill('X');
    final y = _bill('Y');
    _paid(x, 'p1', _t1);
    _paid(y, 'p1', _t1);
    final bills = [y.log.fold(), x.log.fold()];
    final found = arrivalsFor(
      bills,
      'ana',
      const [IncomingTransaction(_t1, 20000)],
    );
    final first =
        splitz.compareUtf8(x.log.fold().bill.id, y.log.fold().bill.id) < 0
            ? x.log.fold().bill.id
            : y.log.fold().bill.id;
    expect(_ids(found.arrived), ['$first/p1']);
    expect(found.short, hasLength(1));
  });

  test('a record already confirmed uses its share first', () {
    final x = _bill('X');
    final y = _bill('Y');
    _paid(x, 'p1', _t1);
    _confirm(x, 'p1');
    _paid(y, 'p2', _t1);
    final found = arrivalsFor(
      [x.log.fold(), y.log.fold()],
      'ana',
      const [IncomingTransaction(_t1, 30000)],
    );
    expect(found.arrived, isEmpty, reason: '20000 of 30000 is already used');
    expect(_ids(found.short), ['${y.log.fold().bill.id}/p2']);
  });

  test('records stating the most ZEC an integer holds cannot wrap the count',
      () {
    // Two confirmed records naming one transaction, each stating 2^63 - 1
    // zatoshi: a subtraction that wrapped would leave the transaction with
    // ZEC to spare, and the next record would arrive.
    final b = _bill('Dinner');
    const most = 9223372036854775807;
    _paid(b, 'p1', _t1, zatoshi: most);
    _confirm(b, 'p1');
    _paid(b, 'p2', _t1, zatoshi: most);
    _confirm(b, 'p2');
    _paid(b, 'p3', _t1, zatoshi: 1000);
    final found = arrivalsFor(
      [b.log.fold()],
      'ana',
      const [IncomingTransaction(_t1, 20000)],
    );
    expect(found.arrived, isEmpty);
    expect(found.short.map((a) => a.payment.id), ['p3']);
  });

  test('two transactions each pay for their own record', () {
    final b = _bill('Dinner');
    _paid(b, 'p1', _t1);
    _paid(b, 'p2', _t2);
    final found = arrivalsFor(
      [b.log.fold()],
      'ana',
      const [IncomingTransaction(_t1, 20000), IncomingTransaction(_t2, 20000)],
    );
    expect(found.arrived.map((a) => a.payment.id), ['p1', 'p2']);
  });

  test('records to somebody else, and other methods, are not matched', () {
    final b = _bill('Dinner');
    _paid(b, 'p1', _t1, method: 'cash');
    _paid(b, 'p2', _t1, method: 'swap');
    final found = arrivalsFor(
      [b.log.fold()],
      'ben',
      const [IncomingTransaction(_t1, 20000)],
    );
    expect(found.arrived, isEmpty);
    final forAna = arrivalsFor(
      [b.log.fold()],
      'ana',
      const [IncomingTransaction(_t1, 20000)],
    );
    expect(forAna.arrived, isEmpty);
    expect(forAna.short, isEmpty);
    expect(forAna.unstated, isEmpty);
  });

  test('a transaction nobody named proposes nothing', () {
    final b = _bill('Dinner');
    _paid(b, 'p1', _t1);
    final found = arrivalsFor(
      [b.log.fold()],
      'ana',
      const [IncomingTransaction(_t2, 20000)],
    );
    expect(found.arrived, isEmpty);
    expect(found.short, isEmpty);
    expect(found.unstated, isEmpty);
  });
}
