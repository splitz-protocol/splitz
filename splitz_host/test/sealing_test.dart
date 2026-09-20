import 'dart:math';

import 'package:test/test.dart';
import 'package:splitz_core/host.dart' as splitz;
import 'package:splitz_host/splitz_host.dart';

import 'support/fake_wallet.dart';

void main() {
  final sealing = SplitsSealing();
  final keys = SplitsKeys(store: InMemorySecretStore(), random: Random(7));

  Map<String, dynamic> anEntry({String name = 'Ana'}) {
    final host = WalletBillHost(FakeWallet());
    return splitz.joinBill(host: host, name: name, payTo: 'u1ana');
  }

  test('a sealed entry opens back into the same entry', () async {
    final key = keys.generateKey();
    final entry = anEntry();
    final opened = await sealing.open(await sealing.seal(entry, key), key);
    expect(opened, entry);
  });

  test('the same entry always seals to the same blob', () async {
    // What keeps a channel finite: a relay keyed by blob content stores an
    // entry once however often it is pushed.
    final key = keys.generateKey();
    final entry = anEntry();
    expect(await sealing.seal(entry, key), await sealing.seal(entry, key));
  });

  test('two different entries never seal to the same blob', () async {
    final key = keys.generateKey();
    expect(
      await sealing.seal(anEntry(name: 'Ana'), key),
      isNot(await sealing.seal(anEntry(name: 'Ben'), key)),
    );
  });

  test('key order in the caller\'s map does not change the blob', () async {
    // The nonce comes from the sealed bytes, and those are canonical. A device
    // building the same entry with its members in another order must reach the
    // same blob, or a relay holds two copies that every device opens perfectly
    // and none recognises as one entry.
    final key = keys.generateKey();
    final entry = anEntry();
    final reordered = <String, dynamic>{
      for (final k in entry.keys.toList().reversed) k: entry[k],
    };
    expect(entry.keys.toList(), isNot(reordered.keys.toList()));
    expect(await sealing.seal(entry, key), await sealing.seal(reordered, key));
  });

  test('a blob sealed under another key does not open', () async {
    final mine = keys.generateKey();
    final theirs = keys.generateKey();
    final blob = await sealing.seal(anEntry(), mine);
    await expectLater(
      () => sealing.open(blob, theirs),
      throwsA(isA<SealingException>()),
    );
  });

  test('an altered blob does not open', () async {
    final key = keys.generateKey();
    final blob = await sealing.seal(anEntry(), key);
    // Flip one character of the ciphertext, well past the version byte.
    final bytes = SplitsSigner.decode(blob).toList();
    bytes[bytes.length - 3] ^= 0x01;
    await expectLater(
      () => sealing.open(SplitsSigner.encode(bytes), key),
      throwsA(isA<SealingException>()),
    );
  });

  test('a blob from a later format is refused, not misread', () async {
    final key = keys.generateKey();
    final bytes = SplitsSigner.decode(
      await sealing.seal(anEntry(), key),
    ).toList()..[0] = SplitsSealing.blobVersion + 1;
    await expectLater(
      () => sealing.open(SplitsSigner.encode(bytes), key),
      throwsA(isA<SealingException>()),
    );
  });

  test('a truncated or empty blob is refused', () async {
    final key = keys.generateKey();
    for (final blob in [
      '',
      SplitsSigner.encode([SplitsSealing.blobVersion]),
    ]) {
      await expectLater(
        () => sealing.open(blob, key),
        throwsA(isA<SealingException>()),
        reason: 'blob "$blob"',
      );
    }
  });

  test(
    'a key of the wrong length is refused before the cipher sees it',
    () async {
      await expectLater(
        () => sealing.seal(anEntry(), 'AAAA'),
        throwsA(isA<SealingException>()),
      );
    },
  );

  test('anything a key-holder seals comes back, entry-shaped or not', () async {
    // Sealing authenticates; it does not judge. A map that is not a valid
    // entry opens perfectly and is refused one layer up, by the protocol's own
    // ingress check, which is the thing that knows what an entry is.
    final key = keys.generateKey();
    final notAnEntry = <String, dynamic>{'v': 1};
    expect(
      await sealing.open(await sealing.seal(notAnEntry, key), key),
      notAnEntry,
    );
  });
}
