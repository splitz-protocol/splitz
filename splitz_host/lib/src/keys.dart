/// Where a bill's key and this account's signing identity live.
library;

import 'dart:async';
import 'dart:convert';
import 'dart:math';

import 'package:crypto/crypto.dart' as hashing;
import 'package:unorm_dart/unorm_dart.dart' as unorm;

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
  // v2: an identity stored under the v1 name was derived from a viewing key,
  // which a wallet hands out, so it is never read again.
  static const String _identityPrefix = 'splitz_identity_seed_v2_';

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
  static const String identityDomain = 'splitz.identity.v2';

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
  ///
  /// Refuses, with [BillKeyConflict], a different key for a bill this device
  /// already holds a key for. Replacing it would seal everything this device
  /// writes under a key the others do not hold, and open nothing they write —
  /// and an invite link is text anyone can send. The same key again is a
  /// no-op.
  Future<void> storeBillKey(String billId, String key) async {
    if (!isWellFormedKey(key)) {
      throw ArgumentError.value(
        key,
        'key',
        'a bill key is $keyLengthBytes bytes of base64url',
      );
    }
    await _inTurn(billId, () async {
      final held = await readBillKey(billId);
      if (held != null && held.isNotEmpty) {
        if (held == key) return;
        throw BillKeyConflict(billId);
      }
      await _store.write(_billKeyName(billId), key);
    });
  }

  /// The turn each bill's key is changed in, per store and for the whole
  /// process: a read of the key and the write or delete that depends on it
  /// are one step, so a key replaced between them is never overwritten or
  /// deleted by a decision made about the one before it.
  static final Expando<Map<String, Future<void>>> _turnsOf = Expando();

  Future<T> _inTurn<T>(String billId, Future<T> Function() step) async {
    final turns = _turnsOf[_store] ??= {};
    final before = turns[billId];
    final done = Completer<void>();
    turns[billId] = done.future;
    try {
      if (before != null) await before;
      return await step();
    } finally {
      done.complete();
      if (identical(turns[billId], done.future)) turns.remove(billId);
    }
  }

  /// Forgets [billId]'s key only while it is still [key], and says whether it
  /// did. The read and the delete are one turn: a key replaced since [key]
  /// was read stays.
  Future<bool> forgetBillIfStill(String billId, String key) =>
      _inTurn(billId, () async {
        String? held;
        try {
          held = await readBillKey(billId);
        } on StateError {
          return false;
        }
        if (held != key) return false;
        await _store.delete(_billKeyName(billId));
        return true;
      });

  /// Replaces the key held for [billId] with [key], whatever was held.
  ///
  /// For a person who has decided which of two invites is genuine — the one
  /// held may itself have come from a forged link. Never called on the
  /// strength of an invite alone: [storeBillKey] refuses that.
  Future<void> replaceBillKey(String billId, String key) async {
    if (!isWellFormedKey(key)) {
      throw ArgumentError.value(
        key,
        'key',
        'a bill key is $keyLengthBytes bytes of base64url',
      );
    }
    await _inTurn(billId, () => _store.write(_billKeyName(billId), key));
  }

  /// Forgets a bill's key, so the keychain does not accumulate secrets for
  /// bills that no longer exist.
  Future<void> forgetBill(String billId) =>
      _inTurn(billId, () => _store.delete(_billKeyName(billId)));

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
  /// [WalletAccount.identitySecret] is what makes it survive a reinstall: the
  /// seed is SHA-256 of [identityDomain], a zero byte and the secret, so the
  /// same mnemonic yields the same identity on a new device. The account
  /// identifier it is filed under does not, because the wallet's own database
  /// assigns that at import.
  ///
  /// An account with no secret gets a random seed. It signs correctly and
  /// simply cannot be recovered, which is a state to report rather than one to
  /// paper over.
  Future<List<int>> ensureIdentitySeed(WalletAccount account) async {
    final stored = await _store.read(_identityName(account.id));
    if (stored != null && stored.isNotEmpty) {
      return SplitsSigner.decode(stored);
    }
    final secret = account.identitySecret;
    final seed = secret == null || secret.isEmpty
        ? List<int>.generate(keyLengthBytes, (_) => _random.nextInt(256))
        : identitySeedFrom(secret);
    await _store.write(_identityName(account.id), SplitsSigner.encode(seed));
    return seed;
  }

  /// Whether this account's identity would survive a restore from its mnemonic.
  ///
  /// False when the seed was drawn at random for want of a secret. The
  /// difference is invisible in every signature it makes and decisive the day
  /// the device is replaced, so it is reported rather than inferred.
  static bool identityIsRecoverable(WalletAccount account) =>
      account.identitySecret != null && account.identitySecret!.isNotEmpty;
}

/// The identity seed [secret] derives: SHA-256 of [SplitsKeys.identityDomain],
/// a zero byte, and the secret.
List<int> identitySeedFrom(List<int> secret) => hashing.sha256.convert([
  ...utf8.encode(SplitsKeys.identityDomain),
  0,
  ...secret,
]).bytes;

/// The highest ZIP 32 account index: account indices are below 2^31.
const maxAccountIndex = 0x7fffffff;

/// Unicode's White_Space property, the set Rust's `split_whitespace` splits
/// on. Dart's `\s` also matches U+FEFF, which is not white space, and two
/// sets would derive two identities from one mnemonic.
final _whiteSpace = RegExp(
  '[\t\n\v\f\r \u0085\u00A0\u1680\u2000-\u200A\u2028\u2029\u202F'
  '\u205F\u3000]+',
);

/// The identity secret of a BIP39 wallet account (§15.1): [mnemonic] and
/// [passphrase], UTF-8, joined by a zero byte, and for any ZIP 32 account but
/// account 0, a zero byte and [accountIndex] as four big-endian bytes.
///
/// Both texts are read in Unicode NFKC, and the mnemonic's words are joined
/// by one space whatever separated them. BIP39 hashes NFKD, and two texts
/// share an NFKC form exactly when they share an NFKD one, so every spelling
/// of one wallet's words and passphrase — and so one wallet's funds — is one
/// participant. NFKC rather than NFKD leaves text as a keyboard types it
/// unchanged: plain ASCII, and composed letters such as "ä".
///
/// The passphrase is part of it because it selects another wallet from one
/// mnemonic; the account index because two accounts of one mnemonic are two
/// people to a bill. Neither text may hold a zero byte, so no two inputs join
/// to the same bytes. Every wallet derives the same bytes, so one person
/// is one participant whichever wallet they restore into.
///
/// Throws [ArgumentError] for an empty mnemonic, whose identity anyone could
/// derive, and for either text holding a zero byte; [RangeError] for an
/// account index outside 0..[maxAccountIndex].
List<int> identitySecretFromMnemonic({
  required String mnemonic,
  required String passphrase,
  int accountIndex = 0,
}) {
  if (mnemonic.isEmpty) {
    throw ArgumentError.value(
      mnemonic,
      'mnemonic',
      'an empty mnemonic derives an identity anyone can compute',
    );
  }
  // The zero byte is the separator: one inside either text would let two
  // different inputs join to one secret, and so one identity.
  if (mnemonic.contains('\u0000') || passphrase.contains('\u0000')) {
    throw ArgumentError(
      'a mnemonic or passphrase holding a zero byte is not one a wallet derives',
    );
  }
  RangeError.checkValueInInterval(
    accountIndex,
    0,
    maxAccountIndex,
    'accountIndex',
  );
  final words = unorm
      .nfkc(mnemonic)
      .split(_whiteSpace)
      .where((w) => w.isNotEmpty)
      .join(' ');
  if (words.isEmpty) {
    throw ArgumentError.value(
      mnemonic,
      'mnemonic',
      'a mnemonic of white space derives an identity anyone can compute',
    );
  }
  return [
    ...utf8.encode(words),
    0,
    ...utf8.encode(unorm.nfkc(passphrase)),
    if (accountIndex != 0) ...[
      0,
      (accountIndex >> 24) & 0xff,
      (accountIndex >> 16) & 0xff,
      (accountIndex >> 8) & 0xff,
      accountIndex & 0xff,
    ],
  ];
}

/// Raised when an invite carries a different key for a bill this device
/// already holds.
class BillKeyConflict implements Exception {
  const BillKeyConflict(this.billId);

  final String billId;

  @override
  String toString() =>
      'BillKeyConflict: this device already holds a different key for $billId';
}
