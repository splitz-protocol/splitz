/// §10.7: a device is on a bill as itself only when the fold binds its key to
/// its id. A record somebody else wrote under that id does not count.
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

Future<_Person> _person(String name) async {
  final seed = _seed(name);
  final key = await _signer.publicKeyFromSeed(seed);
  final id = splitz.participantId(key)!;
  final w = FakeWallet(id: id, payTo: 'u1$name${'0' * 20}');
  return _Person(id, w, WalletBillHost(w, sign: _signer.signerFor(seed)), key);
}

void main() {
  late _Person ana;
  late _Person mal;
  late _Person vic;
  late String billId;
  late List<Map<String, dynamic>> log;

  Future<splitz.FoldedBill> fold(_Person as) =>
      foldVerified(as.wallet, log, billId: billId, signer: SplitsSigner());

  Future<Map<String, dynamic>> ownJoin(_Person p) async {
    p.wallet.tick(const Duration(minutes: 5));
    return p.sign(
      splitz.joinBill(
        host: p.host,
        name: 'Vic',
        payTo: p.wallet.sender.payToAddress,
        identityKey: p.key,
      ),
      billId,
    );
  }

  setUp(() async {
    ana = await _person('ana');
    mal = await _person('mal');
    vic = await _person('vic');
    final create = splitz.createBill(
      host: ana.host,
      name: 'Trip',
      currency: 'EUR',
      creatorKey: ana.key,
    );
    billId = create['id'] as String;
    log = [
      await ana.sign(create, billId),
      await ana.sign(
        splitz.joinBill(
          host: ana.host,
          name: 'Ana',
          payTo: ana.wallet.sender.payToAddress,
          identityKey: ana.key,
        ),
        billId,
      ),
    ];
  });

  test('the creator, joined and signed, is on the bill as itself', () async {
    expect(joinedAsMe(await fold(ana), ana.id), isTrue);
  });

  test('nobody is on a bill they have not joined', () async {
    expect(joinedAsMe(await fold(vic), vic.id), isFalse);
  });

  test('a join somebody else planted under my id is not mine', () async {
    // Mal knows Vic's id from another bill, and writes an unsigned join under
    // it with his own payout before Vic joins.
    final plant = splitz.joinBill(
      host: HostAs(mal.host, vic.id),
      name: 'Vic',
      payTo: mal.wallet.sender.payToAddress,
    );
    log.add(plant);
    final planted = await fold(vic);
    expect(planted.bill.participant(vic.id)?.payTo, startsWith('u1mal'));
    expect(joinedAsMe(planted, vic.id), isFalse);

    // Vic's own signed join binds his key, and his payout is the one paid.
    log.add(await ownJoin(vic));
    final after = await fold(vic);
    expect(joinedAsMe(after, vic.id), isTrue);
    expect(after.bill.participant(vic.id)?.payTo, startsWith('u1vic'));
    expect(
      after.setAside.where((x) => x.id == plant['id']).map((x) => x.code),
      ['unauthorized_entry'],
    );
  });
}
