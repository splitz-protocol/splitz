/// Where a bill's key and this account's signing identity live.
library;

import 'dart:convert';
import 'dart:math';

import 'package:crypto/crypto.dart' as hashing;

import 'signing.dart';
import 'wallet.dart';

/// Holds the symmetric key each bill's contents are sealed under, and the
/// Ed25519 seed this account signs with.
///
/// Both are secrets and both live in the wallet's [SecretStore]. Neither is
/// derived from anything an invite hands out: a bill key derived from the bill
/// id would be reproducible by everyone who ever saw a QR code, which is the
/// one thing an invite gives away freely.
class SplitsKeys {
  SplitsKeys({required SecretStore store, Random? random})
    : _store = store,
      _random = random ?? Random.secure();

  final SecretStore _store;
  final Random _random;

  static const String _billPrefix = 'splitz_bill_key_';
  static const String _identityPrefix = 'splitz_identity_seed_';

  /// Key length in bytes. 32 suits XChaCha20 and AES-256 alike, so the choice
  /// of cipher stays open, and it is also Ed25519's seed length.
  static const int keyLengthBytes = 32;

  /// Domain separator for the derived identity seed.
  ///
  /// Names the protocol, not a wallet. A wallet-specific domain would give one
  /// person two identities across two wallets, and a bill would stop
  /// recognising them the day they switched — which is the opposite of what a
  /// protocol several wallets implement is for.
  ///
  /// Changing it changes every identity derived afterwards, so it is versioned
  /// rather than edited.
  static const String identityDomain = 'splitz.identity.v1';

  String _billKeyName(String billId) => '$_billPrefix$billId';

  String _identityName(String accountId) => '$_identityPrefix$accountId';

  /// The bill's key, creating and storing one on first use.
  Future<String> ensureBillKey(String billId) async {
    final existing = await readBillKey(billId);
    if (existing != null && existing.isNotEmpty) return existing;
    final key = generateKey();
    await _store.write(_billKeyName(billId), key);
    return key;
  }

  /// The stored key, or null when this device holds none for that bill.
  Future<String?> readBillKey(String billId) =>
      _store.read(_billKeyName(billId));

  /// Stores a key that arrived with an invite, so a joiner can open the bill.
  ///
  /// Refuses a key the cipher cannot use. §11.1 checks only that an invite's
  /// `k` is non-empty base64url — not that it is the right length — so without
  /// this a malformed key is stored, and the failure then surfaces from inside
  /// whatever loop next tries to decrypt, far from the scan that caused it.
  Future<void> storeBillKey(String billId, String key) async {
    if (!isWellFormedKey(key)) {
      throw ArgumentError.value(
        key,
        'key',
        'a bill key is $keyLengthBytes bytes of base64url',
      );
    }
    await _store.write(_billKeyName(billId), key);
  }

  /// Forgets a bill's key, so the keychain does not accumulate secrets for
  /// bills that no longer exist.
  Future<void> forgetBill(String billId) => _store.delete(_billKeyName(billId));

  /// Whether [key] is one the cipher can actually use: base64url, padded or
  /// not, decoding to exactly [keyLengthBytes] bytes.
  static bool isWellFormedKey(String key) {
    if (key.isEmpty) return false;
    try {
      return SplitsSigner.decode(key).length == keyLengthBytes;
    } on FormatException {
      return false;
    }
  }

  /// A fresh key, base64url without padding, as an invite carries one.
  String generateKey() => SplitsSigner.encode(
    List<int>.generate(keyLengthBytes, (_) => _random.nextInt(256)),
  );

  /// The 32-byte Ed25519 seed that is this account's signing identity,
  /// creating and storing one on first use.
  ///
  /// One identity per wallet account, not one per bill: the same key signs this
  /// account's entries on every bill, and its public half — published in that
  /// account's own join — is what other participants pin under §10.7.
  ///
  /// [WalletAccount.viewingKey] is what makes it survive a reinstall. A viewing
  /// key is derived from the wallet seed, so the same mnemonic yields the same
  /// identity on a new device; the account identifier it is filed under does
  /// not, because the wallet's own database assigns that at import. Keyed only
  /// by the identifier, a restored participant would be a stranger to every
  /// bill naming them.
  ///
  /// An account with no viewing key gets a random seed. It signs correctly and
  /// simply cannot be recovered, which is a state to report rather than one to
  /// paper over.
  Future<List<int>> ensureIdentitySeed(WalletAccount account) async {
    final stored = await _store.read(_identityName(account.id));
    if (stored != null && stored.isNotEmpty) {
      return SplitsSigner.decode(stored);
    }
    final viewingKey = account.viewingKey;
    final seed = viewingKey == null || viewingKey.isEmpty
        ? List<int>.generate(keyLengthBytes, (_) => _random.nextInt(256))
        : hashing.sha256
              .convert(utf8.encode('$identityDomain:$viewingKey'))
              .bytes;
    await _store.write(_identityName(account.id), SplitsSigner.encode(seed));
    return seed;
  }

  /// Whether this account's identity would survive a restore from its mnemonic.
  ///
  /// False when the seed was drawn at random for want of a viewing key. The
  /// difference is invisible in every signature it makes and decisive the day
  /// the device is replaced, so it is reported rather than inferred.
  static bool identityIsRecoverable(WalletAccount account) =>
      account.viewingKey != null && account.viewingKey!.isNotEmpty;
}
