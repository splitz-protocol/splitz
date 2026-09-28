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

/// The bill every entry in this file is signed and verified on (§10.6).
const String _bill = 'identity-test-bill';

bool verifyByName(Map<String, dynamic> entry, String key,
        {String bill = _bill}) =>
    entry['sig'] ==
    'sig-by-$key:'
        '${splitz.sha256Hex(utf8.encode(splitz.signingMessage(entry, bill)))}';

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
    signEntry(
        host: signing(entry['author'] as String, key),
        entry: entry,
        billId: _bill);

void main() {
  // §10.7. A participant who publishes a key is named by the id that key
  // derives, so no second key can claim them.
  test('a rival key cannot claim a bound participant', () async {
    final anaKey = fakeKey('ana');
    final benKey = fakeKey('ben');
    final impostorKey = fakeKey('zzz');
    final benId = splitz.participantId(benKey)!;

    final ana = signing('ana', anaKey, payTo: 'u1ana');
    final ben = signing(benId, benKey, payTo: 'u1ben');

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

    final clean = log.fold();
    expect(clean.identities.bound[benId], benKey);

    // An impostor writes a self-claim for ben's id under their own key, with
    // their own payout address. Their key does not derive that id.
    final impostor = signing(benId, impostorKey, payTo: 'u1impostor');
    for (var i = 0; i < 3; i++) {
      impostor.tick();
    }
    final rival = await signed(
        joinBill(
            host: impostor,
            name: 'Ben',
            payTo: 'u1impostor',
            identityKey: impostorKey),
        impostorKey);
    log.add([rival]);

    final after = log.fold();
    expect(after.identities.bound[benId], benKey,
        reason: "the rival claim leaves ben's binding as it was");
    // Written as ben and signed with another key, it is refused before its
    // record is read: an entry by a bound participant verifies against their
    // key (§10.3).
    expect(after.setAside.map((a) => (a.id, a.code)),
        contains((rival['id'], splitz.SplitCode.unauthorizedEntry)));
    expect(after.bill.participant(benId)!.payTo, 'u1ben');
  });

  test('a rival claim does not change who a payer pays', () async {
    final anaKey = fakeKey('ana');
    final benKey = fakeKey('ben');
    final impostorKey = fakeKey('zzz');
    final benId = splitz.participantId(benKey)!;

    final ana = signing('ana', anaKey, payTo: 'u1ana');
    final ben = signing(benId, benKey, payTo: 'u1ben');
    final impostor = signing(benId, impostorKey, payTo: 'u1impostor');

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
            paidBy: benId,
            amount: 9000,
            split: {
              'type': 'equal',
              'among': ['ana', benId],
            },
          ),
          benKey),
    ]);
    ana.tick();
    log.add([
      await signed(
          setRate(host: ana, currency: 'EUR', minorUnitsPerZec: 51234), anaKey),
    ]);

    final before = obligationFor(ana, log.fold())!;
    expect(before.uri, startsWith('zcash:u1ben'));

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

    final after = obligationFor(ana, log.fold())!;
    expect(after.uri, before.uri,
        reason: 'the request pays the address ben published, not the rival');
    expect(after.settlements.single.to, benId);
    expect(after.settlements.single.amount, 4500);
  });

  test('a signature made on one bill does not verify on another', () async {
    // §10.6: the message names the bill. A participant's id and key are the
    // same on every bill, so a confirmation copied from one bill into another
    // would otherwise verify there and settle a debt nobody paid there.
    final key = fakeKey('ana');
    final confirmation = await signed(
        confirmPayment(
            host: signing('ana', key),
            paymentId: 'P',
            method: 'recipientConfirmed',
            record: 'r'),
        key);
    expect(verifyByName(confirmation, key), isTrue);
    expect(verifyByName(confirmation, key, bill: 'another-bill'), isFalse);
  });
}
