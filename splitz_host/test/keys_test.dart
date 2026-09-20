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

  test('an identity derived from a viewing key survives a reinstall', () async {
    const account = WalletAccount(id: 'acct-1', viewingKey: 'uview1abc');

    final first = await keys.ensureIdentitySeed(account);

    // A new device: a fresh keychain, and a wallet database that assigns a
    // different account identifier from the same mnemonic.
    final reinstalled = SplitsKeys(store: InMemorySecretStore());
    const afterRestore = WalletAccount(id: 'acct-9', viewingKey: 'uview1abc');
    expect(await reinstalled.ensureIdentitySeed(afterRestore), first);
    expect(SplitsKeys.identityIsRecoverable(account), isTrue);
  });

  test('two accounts do not share an identity', () async {
    const a = WalletAccount(id: 'acct-1', viewingKey: 'uview1abc');
    const b = WalletAccount(id: 'acct-2', viewingKey: 'uview1def');
    expect(
      await keys.ensureIdentitySeed(a),
      isNot(await keys.ensureIdentitySeed(b)),
    );
  });

  test(
    'an account with no viewing key still signs, and says it cannot recover',
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
      // The viewing key arriving later must not silently rotate an identity that
      // other participants have already pinned.
      const after = WalletAccount(id: 'acct-1', viewingKey: 'uview1abc');
      expect(await keys.ensureIdentitySeed(after), seed);
    },
  );
}
