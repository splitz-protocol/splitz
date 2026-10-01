import 'package:test/test.dart';
import 'package:splitz_core/splitz_core.dart' as splitz;
import 'package:splitz_core/host.dart';

import 'support/fake_host.dart';

const _t1 = 'aa00000000000000000000000000000000000000000000000000000000000001';
const _t2 = 'bb00000000000000000000000000000000000000000000000000000000000002';

/// Ana and Ben on one bill named [name]; Ben owes Ana.
({BillLog log, FakeHost ana, FakeHost ben}) _bill(String name,
    {bool priced = true}) {
  final ana = FakeHost(me: 'ana', payToAddress: 'u1ana');
  final ben = FakeHost(me: 'ben', payToAddress: 'u1ben');
  final create = createBill(
      host: ana, name: name, currency: 'EUR', creatorKey: fakeKey('ana'));
  ana.tick();
  final joinAna = joinBill(host: ana, name: 'Ana', payTo: 'u1ana');
  ana.tick();
  // 1,000,000.00 EUR a ZEC: 1000 zatoshi pays for the 10.00 EUR each record
  // settles.
  final rate = setRate(host: ana, currency: 'EUR', minorUnitsPerZec: 100000000);
  ben.tick();
  ben.tick();
  final joinBen = joinBill(host: ben, name: 'Ben', payTo: 'u1ben');
  final log = BillLog(ana);
  expect(log.add([create, joinAna, if (priced) rate, joinBen]), isEmpty);
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

/// [f] with [bound] as the keys §10.7 bound on it, for tests about who a
/// payer is rather than how a key comes to be bound.
FoldedBill _bound(FoldedBill f, Map<String, String> bound) => FoldedBill(
      bill: f.bill,
      setAside: f.setAside,
      withdrawn: f.withdrawn,
      replacedAddresses: f.replacedAddresses,
      identities: splitz.Identities(bound),
      paymentAuthors: f.paymentAuthors,
      paymentDigests: f.paymentDigests,
    );

/// Ben pays [_t1] on bill X and on bill Y, each bound as [x] and [y] (null:
/// unbound); returns what Ana's wallet proposes for both bills.
Arrivals _twoBills({String? x, String? y}) {
  final bx = _bill('X');
  final by = _bill('Y');
  _paid(bx, 'p1', _t1, zatoshi: 10000);
  _paid(by, 'p1', _t1, zatoshi: 10000);
  return arrivalsFor(
    [
      _bound(bx.log.fold(), {if (x != null) 'ben': x}),
      _bound(by.log.fold(), {if (y != null) 'ben': y}),
    ],
    'ana',
    const [IncomingTransaction(_t1, 20000)],
  );
}

void main() {
  test('a record whose transaction arrived with its ZEC is proposed', () {
    final b = _bill('Dinner');
    _paid(b, 'p1', _t1);
    final found = arrivalsFor(
      [b.log.fold()],
      'ana',
      const [IncomingTransaction(_t1, 20000)],
    );
    expect(found.arrived.map((a) => a.payment.id), ['ben:p1']);
    expect(found.arrived.single.txid, _t1);
    expect(found.arrived.single.record, b.log.fold().paymentDigests['ben:p1']);
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
    expect(b.log.fold().bill.confirmedPayments, {'ben:p1'});
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
    expect(found.short.map((a) => a.payment.id), ['ben:p1']);
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
    expect(found.unstated.map((a) => a.payment.id), ['ben:p1']);
  });

  test('one transaction is evidence once, across bills', () {
    final x = _bill('X');
    final y = _bill('Y');
    _paid(x, 'p1', _t1);
    _paid(y, 'p1', _t1);
    final bills = [
      _bound(y.log.fold(), {'ben': fakeKey('ben')}),
      _bound(x.log.fold(), {'ben': fakeKey('ben')}),
    ];
    final found = arrivalsFor(
      bills,
      'ana',
      const [IncomingTransaction(_t1, 20000)],
    );
    final first =
        splitz.compareUtf8(x.log.fold().bill.id, y.log.fold().bill.id) < 0
            ? x.log.fold().bill.id
            : y.log.fold().bill.id;
    expect(_ids(found.arrived), ['$first/ben:p1']);
    expect(found.short, hasLength(1));
  });

  test('a record already confirmed uses its share first', () {
    final x = _bill('X');
    final y = _bill('Y');
    _paid(x, 'p1', _t1);
    _confirm(x, 'ben:p1');
    _paid(y, 'p2', _t1);
    final found = arrivalsFor(
      [
        _bound(x.log.fold(), {'ben': fakeKey('ben')}),
        _bound(y.log.fold(), {'ben': fakeKey('ben')}),
      ],
      'ana',
      const [IncomingTransaction(_t1, 30000)],
    );
    expect(found.arrived, isEmpty, reason: '20000 of 30000 is already used');
    expect(_ids(found.short), ['${y.log.fold().bill.id}/ben:p2']);
  });

  test('records stating the most ZEC an integer holds cannot wrap the count',
      () {
    // Two confirmed records naming one transaction, each stating 2^63 - 1
    // zatoshi: a subtraction that wrapped would leave the transaction with
    // ZEC to spare, and the next record would arrive.
    final b = _bill('Dinner');
    const most = 9223372036854775807;
    _paid(b, 'p1', _t1, zatoshi: most);
    _confirm(b, 'ben:p1');
    _paid(b, 'p2', _t1, zatoshi: most);
    _confirm(b, 'ben:p2');
    _paid(b, 'p3', _t1, zatoshi: 1000);
    final found = arrivalsFor(
      [b.log.fold()],
      'ana',
      const [IncomingTransaction(_t1, 20000)],
    );
    expect(found.arrived, isEmpty);
    expect(found.short.map((a) => a.payment.id), ['ben:p3']);
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
    expect(found.arrived.map((a) => a.payment.id), ['ben:p1', 'ben:p2']);
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

  test(
      'a txid is compared with ASCII space trimmed and ASCII lower-cased, '
      'nothing wider', () {
    expect(txidKey(' \tABCD\r\n'), 'abcd');
    expect(txidKey('abcd\u{FEFF}'), 'abcd\u{FEFF}');
    expect(txidKey('\u{00A0}abcd'), '\u{00A0}abcd');
    expect(txidKey('\u{0130}BC'), '\u{0130}bc');
  });

  test('a transaction two payers name is evidence for neither', () {
    final b = _bill('Dinner');
    _paid(b, 'p1', _t1);
    final mal = FakeHost(me: 'mal', payToAddress: 'u1mal');
    mal.tick();
    expect(
        b.log.add([joinBill(host: mal, name: 'Mal', payTo: 'u1mal')]), isEmpty);
    mal.tick();
    expect(
      b.log.add([
        recordPayment(
          host: mal,
          paymentId: '0',
          to: 'ana',
          amount: 1000,
          reference: _t1,
          zatoshi: 20000,
        ),
      ]),
      isEmpty,
    );
    final found = arrivalsFor(
      [b.log.fold()],
      'ana',
      const [IncomingTransaction(_t1, 20000)],
    );
    expect(found.arrived, isEmpty);
    expect(
        found.disputed.map((a) => a.payment.id).toSet(), {'ben:p1', 'mal:0'});
  });

  group('a payer is who §10.7 bound, not the id string', () {
    test('an unbound id on another bill is not the bound payer it names', () {
      final found = _twoBills(x: fakeKey('ben'));
      expect(found.arrived, isEmpty);
      expect(found.disputed, hasLength(2));
    });

    test('one id bound to two keys on two bills is two payers', () {
      final found = _twoBills(x: fakeKey('ben'), y: fakeKey('mal'));
      expect(found.arrived, isEmpty);
      expect(found.disputed, hasLength(2));
    });

    test('one id unbound on two bills is two payers', () {
      final found = _twoBills();
      expect(found.arrived, isEmpty);
      expect(found.disputed, hasLength(2));
    });

    test('one key on two bills is one payer, and both arrive', () {
      final found = _twoBills(x: fakeKey('ben'), y: fakeKey('ben'));
      expect(found.disputed, isEmpty);
      expect(found.arrived, hasLength(2));
    });
  });

  test('one payer naming a transaction twice is still counted once', () {
    final b = _bill('Dinner');
    _paid(b, 'p1', _t1);
    _paid(b, 'p2', _t1);
    final found = arrivalsFor(
      [b.log.fold()],
      'ana',
      const [IncomingTransaction(_t1, 20000)],
    );
    expect(found.disputed, isEmpty);
    expect(found.arrived.map((a) => a.payment.id), ['ben:p1']);
    expect(found.short.map((a) => a.payment.id), ['ben:p2']);
  });

  group('a record its ZEC does not pay for (§14.7)', () {
    /// One 10.00 EUR record on a bill priced at [rate], paid with [zatoshi].
    Arrivals priced(int? rate, int zatoshi) {
      final b = _bill('priced', priced: false);
      if (rate != null) {
        b.ana.tick();
        expect(
            b.log.add([
              setRate(host: b.ana, currency: 'EUR', minorUnitsPerZec: rate),
            ]),
            isEmpty);
      }
      _paid(b, 'p1', _t1, zatoshi: zatoshi);
      return arrivalsFor(
          [b.log.fold()], 'ana', [IncomingTransaction(_t1, zatoshi)]);
    }

    List<String> paymentIds(List<Arrival> a) =>
        [for (final x in a) x.payment.id];

    test('one zatoshi for 10.00 EUR is not proposed', () {
      final found = priced(100000000, 1);
      expect(paymentIds(found.underpriced), ['ben:p1']);
      expect(found.arrived, isEmpty);
    });

    test('95% of the amount pays for it, one zatoshi less does not', () {
      // 950 x 100000000 x 100 = 9.5e12 = 1000 x 95 x 10^8.
      expect(paymentIds(priced(100000000, 950).arrived), ['ben:p1']);
      expect(paymentIds(priced(100000000, 949).underpriced), ['ben:p1']);
    });

    test('a bill with no rate vouches for no record', () {
      final found = priced(null, 20000);
      expect(paymentIds(found.underpriced), ['ben:p1']);
      expect(found.arrived, isEmpty);
    });
  });

  group('a transaction whose memos the wallet read (§8.5, §14.7)', () {
    Arrivals withMemos(List<String>? memos) {
      final b = _bill('memo');
      _paid(b, 'p1', _t1);
      final folded = b.log.fold();
      return arrivalsFor(
          [folded],
          'ana',
          [
            IncomingTransaction(_t1, 20000,
                memos: memos
                    ?.map((m) => m.replaceAll('<bill>', folded.bill.id))
                    .toList()),
          ]);
    }

    List<String> ids(List<Arrival> a) => [for (final x in a) x.payment.id];

    test('a memo naming the bill is proposed', () {
      expect(ids(withMemos(['splitz:<bill>']).arrived), ['ben:p1']);
    });

    test("another bill's memo is not", () {
      final found = withMemos(['splitz:SomeOtherBill00000000']);
      expect(ids(found.unbound), ['ben:p1']);
      expect(found.arrived, isEmpty);
    });

    test('no memo at all is not', () {
      expect(ids(withMemos(const []).unbound), ['ben:p1']);
    });

    test('memos the wallet could not read decide nothing', () {
      expect(ids(withMemos(null).arrived), ['ben:p1']);
    });
  });
}
