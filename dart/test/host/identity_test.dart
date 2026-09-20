import 'dart:convert';

import 'package:test/test.dart';
import 'package:splitz_core/host.dart';
import 'package:splitz_core/splitz_core.dart' as splitz;

import 'support/fake_host.dart';

/// A signer that is not cryptography: it names the key and digests the bytes
/// it was handed. §10.7 is about which key carries a validly signed
/// self-claim, not about which curve signs it, so a stand-in that is right
/// about that and about nothing else exercises the rule exactly.
///
/// The digest is what makes it a test of §10.6 as well: a signature over any
/// other bytes than [splitz.signingMessage]'s fails to verify, so a signer
/// that takes its message from the wrong place is caught here rather than
/// agreeing with itself.
Future<String> Function(List<int>) signerFor(String key) =>
    (message) async => 'sig-by-$key:${splitz.sha256Hex(message)}';

bool verifyByName(Map<String, dynamic> entry, String key) =>
    entry['sig'] ==
    'sig-by-$key:'
        '${splitz.sha256Hex(utf8.encode(splitz.signingMessage(entry)))}';

FakeHost signing(String me, String key, {String? payTo}) => FakeHost(
      me: me,
      payToAddress: payTo,
      sign: signerFor(key),
      verify: verifyByName,
    );

/// Signs an entry the way §10.6 asks and returns it.
///
/// Goes through [signEntry] rather than attaching a string: the host's signer
/// is the thing under test here, and a helper that writes the signature itself
/// would pass with nothing behind [BillHost.sign] at all.
Future<Map<String, dynamic>> signed(Map<String, dynamic> entry, String key) =>
    signEntry(host: signing(entry['author'] as String, key), entry: entry);

void main() {
  test('two keys claiming one id leaves that id contested', () async {
    final anaKey = fakeKey('ana');
    final benKey = fakeKey('ben');
    final impostorKey = fakeKey('zzz');

    final ana = signing('ana', anaKey, payTo: 'u1ana');
    final ben = signing('ben', benKey, payTo: 'u1ben');

    final log = BillLog(ana);
    log.add([
      await signed(
          createBill(
              host: ana, name: 'Dinner', currency: 'EUR', creatorKey: anaKey),
          anaKey),
    ]);
    ana.tick();
    log.add([
      await signed(joinBill(host: ana, name: 'Ana', payTo: 'u1ana'), anaKey),
    ]);
    ben.tick();
    ben.tick();
    log.add([
      await signed(
          joinBill(host: ben, name: 'Ben', payTo: 'u1ben', identityKey: benKey),
          benKey),
    ]);

    // Ben's own key is bound, nothing is contested.
    final clean = log.fold();
    expect(clean.identities.contested, isEmpty);
    expect(clean.identities.bound['ben'], benKey);

    // An impostor mints a rival self-claim for ben's id, with their own
    // payout address. Both claims verify against the key each carries, so
    // §10.7 binds neither.
    final impostor = signing('ben', impostorKey, payTo: 'u1impostor');
    impostor.tick();
    impostor.tick();
    impostor.tick();
    log.add([
      await signed(
          joinBill(
              host: impostor,
              name: 'Ben',
              payTo: 'u1impostor',
              identityKey: impostorKey),
          impostorKey),
    ]);

    final contested = log.fold();
    expect(contested.identities.contested, contains('ben'));
    expect(contested.identities.bound.containsKey('ben'), isFalse);
  });

  test('a contested payee is not settled to silently', () async {
    final anaKey = fakeKey('ana');
    final benKey = fakeKey('ben');
    final impostorKey = fakeKey('zzz');

    final ana = signing('ana', anaKey, payTo: 'u1ana');
    final ben = signing('ben', benKey, payTo: 'u1ben');
    final impostor = signing('ben', impostorKey, payTo: 'u1impostor');

    final log = BillLog(ana);
    log.add([
      await signed(
          createBill(
              host: ana, name: 'Dinner', currency: 'EUR', creatorKey: anaKey),
          anaKey),
    ]);
    ana.tick();
    log.add([
      await signed(joinBill(host: ana, name: 'Ana', payTo: 'u1ana'), anaKey),
    ]);
    ben.tick();
    ben.tick();
    log.add([
      await signed(
          joinBill(host: ben, name: 'Ben', payTo: 'u1ben', identityKey: benKey),
          benKey),
    ]);
    // Ben paid, so ana owes ben.
    ben.tick();
    log.add([
      await signed(
          addExpense(
            host: ben,
            expenseId: 'x1',
            paidBy: 'ben',
            amount: 9000,
            split: const {
              'type': 'equal',
              'among': ['ana', 'ben'],
            },
          ),
          benKey),
    ]);
    ana.tick();
    log.add([
      await signed(
          setRate(host: ana, currency: 'EUR', minorUnitsPerZec: 51234), anaKey),
    ]);

    // Before the contest, ana pays ben at ben's own address.
    final before = obligationFor(ana, log.fold())!;
    expect(before.uri, startsWith('zcash:u1ben'));

    impostor.tick();
    impostor.tick();
    impostor.tick();
    impostor.tick();
    impostor.tick();
    log.add([
      await signed(
          joinBill(
              host: impostor,
              name: 'Ben',
              payTo: 'u1impostor',
              identityKey: impostorKey),
          impostorKey),
    ]);

    final folded = log.fold();
    expect(folded.identities.contested, contains('ben'));

    final after = obligationFor(ana, folded)!;
    expect(after.uri, isNull,
        reason: 'section 10.7: a wallet MUST NOT settle to a contested '
            "participant's address without putting it in front of the payer");
    expect(after.contested.single.to, 'ben');
    expect(after.contested.single.amount, 4500);
    expect(after.contested.single.address, 'u1impostor',
        reason: 'the payer is shown the address they would have paid');
    expect(after.settlements, isEmpty);
  });

  test('a payer who has been shown the contest can still pay', () async {
    // A contest is also a denial of payment: anyone may mint a rival claim for
    // an id. §10.7 asks that the payer be shown it, not that paying be
    // impossible, so a refusal with no way through would hand an attacker a
    // way to stop a bill being settled at all.
    final anaKey = fakeKey('ana');
    final benKey = fakeKey('ben');
    final impostorKey = fakeKey('zzz');

    final ana = signing('ana', anaKey, payTo: 'u1ana');
    final ben = signing('ben', benKey, payTo: 'u1ben');
    final impostor = signing('ben', impostorKey, payTo: 'u1impostor');

    final log = BillLog(ana);
    log.add([
      await signed(
          createBill(
              host: ana, name: 'Dinner', currency: 'EUR', creatorKey: anaKey),
          anaKey),
    ]);
    ana.tick();
    log.add([
      await signed(joinBill(host: ana, name: 'Ana', payTo: 'u1ana'), anaKey),
    ]);
    ben.tick();
    ben.tick();
    log.add([
      await signed(
          joinBill(host: ben, name: 'Ben', payTo: 'u1ben', identityKey: benKey),
          benKey),
    ]);
    ben.tick();
    log.add([
      await signed(
          addExpense(
            host: ben,
            expenseId: 'x1',
            paidBy: 'ben',
            amount: 9000,
            split: const {
              'type': 'equal',
              'among': ['ana', 'ben'],
            },
          ),
          benKey),
    ]);
    ana.tick();
    log.add([
      await signed(
          setRate(host: ana, currency: 'EUR', minorUnitsPerZec: 51234), anaKey),
    ]);
    for (var i = 0; i < 5; i++) {
      impostor.tick();
    }
    log.add([
      await signed(
          joinBill(
              host: impostor,
              name: 'Ben',
              payTo: 'u1impostor',
              identityKey: impostorKey),
          impostorKey),
    ]);

    final folded = log.fold();
    final shown = obligationFor(ana, folded)!;
    expect(shown.uri, isNull);
    expect(shown.contested.single.address, 'u1impostor');

    // Having seen which address it is, the payer decides.
    final accepted = obligationFor(ana, folded, payAnyway: const {'ben'})!;
    expect(accepted.contested, isEmpty);
    expect(accepted.settlements.single.to, 'ben');
    expect(accepted.uri, startsWith('zcash:u1impostor'),
        reason: 'the payer accepted this address, having been shown it');
  });
}
