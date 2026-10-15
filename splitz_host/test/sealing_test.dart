import 'dart:convert';
import 'dart:math';

import 'package:cryptography/cryptography.dart';
import 'package:splitz_core/splitz_core.dart' as protocol;

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

  group('a blob is padded to a bucket (§11.3)', () {
    /// A frame of [version] sealing [clear] as it is, unpadded or not.
    Future<String> sealRaw(int version, List<int> clear, String key) async {
      final cipher = Xchacha20.poly1305Aead();
      final nonce = protocol.sealedNonce(clear);
      final box = await cipher.encrypt(
        clear,
        secretKey: await cipher.newSecretKeyFromBytes(SplitsSigner.decode(key)),
        nonce: nonce,
      );
      return SplitsSigner.encode([
        version,
        ...nonce,
        ...box.cipherText,
        ...box.mac.bytes,
      ]);
    }

    int sealedBytes(String blob) => SplitsSigner.decode(blob).length - 41;

    test('a blob is as long as its bucket, not its entry', () async {
      final key = keys.generateKey();
      expect(sealedBytes(await sealing.seal(anEntry(), key)), 1024);
      expect(
        sealedBytes(await sealing.seal(anEntry(name: 'A' * 600), key)),
        1024,
      );
      expect(
        sealedBytes(await sealing.seal(anEntry(name: 'A' * 1200), key)),
        2048,
      );
      expect(SplitsSigner.decode(await sealing.seal(anEntry(), key))[0], 2);
    });

    test('a version 1 blob, sealed unpadded, still opens', () async {
      final key = keys.generateKey();
      final entry = anEntry();
      final v1 = await sealRaw(1, protocol.sealedPlaintext(entry), key);
      expect(await sealing.open(v1, key), entry);
    });

    test('a version 2 blob padded any other way is refused', () async {
      final key = keys.generateKey();
      final plain = protocol.sealedPlaintext(anEntry());
      for (final clear in [
        plain,
        [...plain, 0x80, ...List.filled(1022 - plain.length, 0)],
        [...plain, ...List.filled(1024 - plain.length, 0)],
        // One plaintext has one padding: not a bucket past the smallest.
        [...plain, 0x80, ...List.filled(2047 - plain.length, 0)],
      ]) {
        await expectLater(
          () async => sealing.open(await sealRaw(2, clear, key), key),
          throwsA(isA<SealingException>()),
          reason: '${clear.length} bytes',
        );
      }
      final good = protocol.padSealed(plain);
      expect(
        await sealing.open(await sealRaw(2, good, key), key),
        jsonDecode(utf8.decode(plain)),
      );
    });

    test('a version of zero is refused', () async {
      final key = keys.generateKey();
      final v0 = await sealRaw(0, protocol.sealedPlaintext(anEntry()), key);
      await expectLater(
        () => sealing.open(v0, key),
        throwsA(isA<SealingException>()),
      );
    });

    test(
      'an entry no bucket holds is refused, one byte under is not',
      () async {
        final key = keys.generateKey();
        // `{"n":"…"}` is 8 bytes around the string; the marker is one more.
        Map<String, dynamic> sized(int bytes) => {'n': 'x' * (bytes - 8)};
        expect(protocol.sealedSizeRefusal(sized(32767)), isNull);
        expect(sealedBytes(await sealing.seal(sized(32767), key)), 32768);
        expect(
          protocol.sealedSizeRefusal(sized(32768)),
          protocol.SplitCode.sealedTooLarge,
        );
        await expectLater(
          () => sealing.seal(sized(32768), key),
          throwsA(
            isA<protocol.SplitError>().having(
              (e) => e.code,
              'code',
              protocol.SplitCode.sealedTooLarge,
            ),
          ),
        );
      },
    );
  });
}
