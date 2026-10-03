import 'dart:convert';
import 'dart:math';

import 'package:test/test.dart';
import 'package:splitz_host/splitz_host.dart';

void main() {
  late InMemorySecretStore store;
  late SplitsKeys keys;

  setUp(() {
    store = InMemorySecretStore();
    keys = SplitsKeys(store: store, random: Random(1));
  });

  test('a bill key is 32 bytes and is kept, not re-derived', () async {
    final first = await keys.ensureBillKey('b1');
    expect(SplitsKeys.isWellFormedKey(first), isTrue);
    expect(await keys.ensureBillKey('b1'), first);
    expect(await keys.readBillKey('b1'), first);
  });

  test('two bills do not share a key', () async {
    expect(
      await keys.ensureBillKey('b1'),
      isNot(await keys.ensureBillKey('b2')),
    );
  });

  test('a key is not derived from the bill id', () async {
    // The id is what every invite and every QR code hands out. A key derived
    // from it would be reproducible by anyone who ever saw one.
    final a = SplitsKeys(store: InMemorySecretStore(), random: Random(1));
    final b = SplitsKeys(store: InMemorySecretStore(), random: Random(2));
    expect(await a.ensureBillKey('b1'), isNot(await b.ensureBillKey('b1')));
  });

  test(
    'a key of the wrong length is refused at the scan, not at the cipher',
    () async {
      // §11.1 checks only that an invite's `k` is non-empty base64url. A key that
      // is the right alphabet and the wrong length therefore reaches this
      // untouched, and storing it would move the failure into whatever loop next
      // tries to decrypt.
      for (final bad in ['AAAA', '', 'A' * 42, 'A' * 44]) {
        expect(
          () => keys.storeBillKey('b1', bad),
          throwsA(isA<ArgumentError>()),
          reason: 'key of length ${bad.length}',
        );
      }
      expect(await keys.readBillKey('b1'), isNull);
    },
  );

  test("a key in standard base64's alphabet is not a key", () async {
    // `base64Url.decode` accepts `+` and `/` and re-encodes them as `-` and
    // `_`. Left unchecked, such a key decodes to 32 bytes, is stored as it
    // arrived, and then matches nothing §11.1 will ever hand over.
    const wrongAlphabet = '+/AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA';
    expect(wrongAlphabet.length, 43);
    expect(SplitsKeys.isWellFormedKey(wrongAlphabet), isFalse);
    expect(
      () => keys.storeBillKey('b1', wrongAlphabet),
      throwsA(isA<ArgumentError>()),
    );
    // The same 32 bytes in the alphabet §9.4 writes are a key.
    expect(SplitsKeys.isWellFormedKey('-_${'A' * 41}'), isTrue);
  });

  test('a well-formed key from an invite is stored as it arrived', () async {
    final key = keys.generateKey();
    await keys.storeBillKey('b1', key);
    expect(await keys.readBillKey('b1'), key);
  });

  test('forgetting a bill forgets its key', () async {
    await keys.ensureBillKey('b1');
    await keys.forgetBill('b1');
    expect(await keys.readBillKey('b1'), isNull);
  });

  test('an identity derived from a secret survives a reinstall', () async {
    const account = WalletAccount(id: 'acct-1', identitySecret: [1, 2, 3]);

    final first = await keys.ensureIdentitySeed(account);

    // A new device: a fresh keychain, and a wallet database that assigns a
    // different account identifier from the same mnemonic.
    final reinstalled = SplitsKeys(store: InMemorySecretStore());
    const afterRestore = WalletAccount(id: 'acct-9', identitySecret: [1, 2, 3]);
    expect(await reinstalled.ensureIdentitySeed(afterRestore), first);
    expect(SplitsKeys.identityIsRecoverable(account), isTrue);
  });

  test('two accounts do not share an identity', () async {
    const a = WalletAccount(id: 'acct-1', identitySecret: [1, 2, 3]);
    const b = WalletAccount(id: 'acct-2', identitySecret: [4, 5, 6]);
    expect(
      await keys.ensureIdentitySeed(a),
      isNot(await keys.ensureIdentitySeed(b)),
    );
  });

  test(
    'an account with no secret still signs, and says it cannot recover',
    () async {
      const account = WalletAccount(id: 'acct-1');
      final seed = await keys.ensureIdentitySeed(account);
      expect(seed.length, SplitsKeys.keyLengthBytes);
      expect(
        await keys.ensureIdentitySeed(account),
        seed,
        reason: 'it is stored, so it is stable on this device',
      );
      expect(SplitsKeys.identityIsRecoverable(account), isFalse);

      // And it is a real signing key: §10.6 is satisfied by it.
      final signer = SplitsSigner();
      final key = await signer.publicKeyFromSeed(seed);
      expect(SplitsSigner.decode(key).length, 32);
    },
  );

  test(
    'a stored identity is returned unchanged, whatever the account now says',
    () async {
      const before = WalletAccount(id: 'acct-1');
      final seed = await keys.ensureIdentitySeed(before);
      // A secret arriving later must not silently rotate an identity that
      // other participants have already pinned.
      const after = WalletAccount(id: 'acct-1', identitySecret: [1, 2, 3]);
      expect(await keys.ensureIdentitySeed(after), seed);
    },
  );

  test('the seed is SHA-256 of the domain, a zero byte and the secret', () {
    // Pinned against a digest computed outside this package:
    //   printf 'splitz.identity.v2\x00\x01\x02\x03' | shasum -a 256
    expect(
      SplitsSigner.encode(identitySeedFrom([1, 2, 3])),
      SplitsSigner.encode(
        _hex(
          '30da771c99ad554a64185a76297a22e32b18a03a8a8bd49fe2ea50c39aaf3a9b',
        ),
      ),
    );
  });

  test(
    'an identity stored under the viewing-key derivation is not read',
    () async {
      final store = InMemorySecretStore();
      await store.write(
        'splitz_identity_seed_acct-1',
        SplitsSigner.encode(List<int>.filled(32, 7)),
      );
      final fresh = SplitsKeys(store: store);
      const account = WalletAccount(id: 'acct-1', identitySecret: [1, 2, 3]);
      expect(
        await fresh.ensureIdentitySeed(account),
        identitySeedFrom([1, 2, 3]),
      );
    },
  );

  group('a mnemonic derives the same identity in every wallet (§15.1)', () {
    const mnemonic =
        'abandon abandon abandon abandon abandon abandon abandon abandon '
        'abandon abandon abandon about';
    String seedOf(String passphrase, int account) => base64UrlEncode(
      identitySeedFrom(
        identitySecretFromMnemonic(
          mnemonic: mnemonic,
          passphrase: passphrase,
          accountIndex: account,
        ),
      ),
    ).replaceAll('=', '');

    test('pinned against seeds computed outside this package', () {
      expect(seedOf('', 0), 'Bsu7QAZyG9usbFpPUrQUo5ni3MtX7JeU-rUHoWKUPKY');
      expect(
        seedOf('TREZOR', 0),
        'NoXrjBFhw-SnV_XZyW7Pe9JyZ5s3G17lAWIl76cxbVU',
      );
      expect(seedOf('', 1), 'jWQijb3QECEvHgOUb4_W7UPiUujN7D-7Nq0jYg5mrKo');
      expect(
        seedOf('', maxAccountIndex),
        'W0O2YQssPQfLgQT0BSukqsP17xenAYEbLGWwi6Ln7q4',
      );
      expect(
        seedOf('pässwörd', 5),
        'mQ-gPUEwIqxWeL6LWOJNH3azGQVVfUOET30BZPpzRa0',
      );
    });

    test('account 0 carries no index, account 1 does', () {
      expect(identitySecretFromMnemonic(mnemonic: 'm', passphrase: 'p'), [
        0x6d,
        0,
        0x70,
      ]);
      expect(
        identitySecretFromMnemonic(
          mnemonic: 'm',
          passphrase: 'p',
          accountIndex: 1,
        ),
        [0x6d, 0, 0x70, 0, 0, 0, 0, 1],
      );
    });

    test('an empty mnemonic and an index past ZIP 32 are refused', () {
      expect(
        () => identitySecretFromMnemonic(mnemonic: '', passphrase: 'p'),
        throwsArgumentError,
      );
      expect(
        () => identitySecretFromMnemonic(
          mnemonic: 'm',
          passphrase: 'p',
          accountIndex: 0x80000000,
        ),
        throwsRangeError,
      );
      expect(
        () => identitySecretFromMnemonic(
          mnemonic: 'm',
          passphrase: 'p',
          accountIndex: -1,
        ),
        throwsRangeError,
      );
      // A zero byte is the separator, so one inside either text would let two
      // inputs join to one secret: account 1 of (m, p) and account 0 of
      // (m, "p\0\0\0\0\x01") are the same bytes.
      for (final (mnemonic, passphrase) in [
        ('m', 'p\u0000\u0000\u0000\u0000\u0001'),
        ('a\u0000b', ''),
        ('a', 'b\u0000'),
      ]) {
        expect(
          () => identitySecretFromMnemonic(
            mnemonic: mnemonic,
            passphrase: passphrase,
          ),
          throwsArgumentError,
        );
      }
      expect(identitySecretFromMnemonic(mnemonic: 'm', passphrase: ''), [
        0x6d,
        0,
      ]);
    });
  });

  group('two accounts on one store', () {
    const key = 'Ag0fRP6k8Q3m4m0yS5Yq0R1Ff7lM0nq8yLw4X0nNw5c';
    test('unscoped, one forgetting a bill deletes the other\'s key', () async {
      final shared = InMemorySecretStore();
      final a = SplitsKeys(store: shared, random: Random(1));
      final b = SplitsKeys(store: shared, random: Random(2));
      await a.storeBillKey('bill', key);
      await b.storeBillKey('bill', key);
      await a.forgetBill('bill');
      expect(await b.readBillKey('bill'), isNull);
    });

    test('scoped, each keeps its own', () async {
      final shared = InMemorySecretStore();
      final a = SplitsKeys(
        store: AccountSecretStore(shared, account: 'acct-a'),
        random: Random(1),
      );
      final b = SplitsKeys(
        store: AccountSecretStore(shared, account: 'acct-b'),
        random: Random(2),
      );
      await a.storeBillKey('bill', key);
      await b.storeBillKey('bill', key);
      await a.forgetBill('bill');
      expect(await a.readBillKey('bill'), isNull);
      expect(await b.readBillKey('bill'), key);
      expect(
        () => AccountSecretStore(shared, account: ''),
        throwsArgumentError,
      );
    });
  });
}

List<int> _hex(String hex) => [
  for (var i = 0; i < hex.length; i += 2)
    int.parse(hex.substring(i, i + 2), radix: 16),
];
