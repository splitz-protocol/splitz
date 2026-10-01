/// A signed entry carried from one bill into another.
///
/// A participant's id and key are the same on every bill, so the only thing
/// that keeps a confirmation Ana gave on one bill from settling a debt on
/// another is §10.6 naming the bill in what she signed, and §10.5 binding the
/// confirmation to the record it was given for.
library;

import 'package:splitz_core/host.dart' as splitz;
import 'package:splitz_host/splitz_host.dart';
import 'package:test/test.dart';

import 'support/fake_wallet.dart';

final _signer = SplitsSigner();

List<int> _seed(String who) =>
    List<int>.generate(SplitsSigner.seedBytes, (i) => who.codeUnitAt(0) + i);

class _Person {
  _Person(this.id, this.wallet, this.host, this.key);
  final String id;
  final FakeWallet wallet;
  final WalletBillHost host;
  final String key;

  Future<Map<String, dynamic>> sign(Map<String, dynamic> e, String bill) =>
      splitz.signEntry(host: host, entry: e, billId: bill);
}

/// [name]'s phone, speaking as the id its key derives (§10.7).
Future<_Person> _person(String name, {int nudge = 0}) async {
  final seed = _seed(name);
  final key = await _signer.publicKeyFromSeed(seed);
  final id = splitz.participantId(key)!;
  final w = FakeWallet(id: id, payTo: 'u1$name${'0' * 20}');
  for (var i = 0; i < nudge; i++) {
    w.tick();
  }
  return _Person(id, w, WalletBillHost(w, sign: _signer.signerFor(seed)), key);
}

/// A bill [creator] opens and [other] joins, each signed on its own id.
Future<(String, List<Map<String, dynamic>>)> _bill(
  _Person creator,
  _Person other,
  String name,
) async {
  final create = splitz.createBill(
    host: creator.host,
    name: name,
    currency: 'EUR',
    creatorKey: creator.key,
  );
  final id = create['id'] as String;
  final entries = [await creator.sign(create, id)];
  for (final p in [creator, other]) {
    p.wallet.tick();
    entries.add(
      await p.sign(
        splitz.joinBill(
          host: p.host,
          name: p.id,
          payTo: p.wallet.sender.payToAddress,
          identityKey: p.key,
        ),
        id,
      ),
    );
  }
  return (id, entries);
}

Future<splitz.FoldedBill> _fold(
  _Person as,
  String id,
  List<Map<String, dynamic>> entries,
) => foldVerified(as.wallet, entries, billId: id, signer: SplitsSigner());

void main() {
  late _Person ana;
  late _Person mal;
  late String idA;
  late Map<String, dynamic> payA;
  late Map<String, dynamic> confirmA;
  late String idB;
  late List<Map<String, dynamic>> billB;

  setUp(() async {
    ana = await _person('ana');
    mal = await _person('mal', nudge: 1);

    // Bill A: Mal pays Ana a cent, and Ana confirms it.
    final (a, billA) = await _bill(mal, ana, 'A');
    idA = a;
    mal.wallet.tick();
    payA = await mal.sign(
      splitz.recordPayment(
        host: mal.host,
        paymentId: 'P',
        to: ana.id,
        amount: 1,
        reference: 'aa' * 32,
      ),
      idA,
    );
    final digest = (await _fold(ana, idA, [
      ...billA,
      payA,
    ])).paymentDigests['${mal.id}:P']!;
    ana.wallet.tick();
    confirmA = await ana.sign(
      splitz.confirmPayment(
        host: ana.host,
        paymentId: '${mal.id}:P',
        method: 'recipientConfirmed',
        record: digest,
      ),
      idA,
    );
    final foldedA = await _fold(ana, idA, [...billA, payA, confirmA]);
    // The control: on its own bill, the confirmation stands.
    expect(foldedA.bill.confirmedPayments, contains('${mal.id}:P'));

    // Bill B: Ana paid 100.00 for both of them, so Mal owes her 50.00.
    final (b, entriesB) = await _bill(ana, mal, 'B');
    idB = b;
    ana.wallet.tick();
    billB = [
      ...entriesB,
      await ana.sign(
        splitz.addExpense(
          host: ana.host,
          expenseId: 'dinner',
          paidBy: ana.id,
          amount: 10000,
          split: {
            'type': 'equal',
            'among': [ana.id, mal.id],
          },
        ),
        idB,
      ),
    ];
  });

  test(
    'the same record and its confirmation, carried over, settle nothing',
    () async {
      // Mal re-signs the very record of bill A on bill B, so the payment it
      // states is identical; only the bill in Ana's signature is not.
      final carried = await mal.sign({...payA}..remove('sig'), idB);
      final folded = await _fold(ana, idB, [...billB, carried, confirmA]);
      expect(folded.bill.confirmedPayments, isEmpty);
      expect(
        folded.setAside.map((a) => a.code),
        contains('unauthorized_entry'),
      );
    },
  );

  test('nor under a larger record written for it', () async {
    mal.wallet.tick();
    final forged = await mal.sign(
      splitz.recordPayment(
        host: mal.host,
        paymentId: 'P',
        to: ana.id,
        amount: 5000,
        reference: 'bb' * 32,
      ),
      idB,
    );
    final folded = await _fold(ana, idB, [...billB, forged, confirmA]);
    expect(folded.bill.confirmedPayments, isEmpty);
  });
}
