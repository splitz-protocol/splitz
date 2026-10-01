/// Ed25519 over the message SPEC.md §10.6 fixes.
library;

import 'dart:convert';

import 'package:cryptography/cryptography.dart';
import 'package:splitz_core/splitz_core.dart' as protocol;

/// Signs a bill's entries, and answers whether somebody else's verify.
///
/// The protocol fixes what a signature covers and everything around the answer;
/// the curve operation is the host's (§13). This is that operation.
///
/// Why it matters that entries are signed at all: a bill's contents are sealed
/// under a key every participant holds, so the seal proves only that a blob
/// came from *someone* who saw the invite. A signature proves *which*
/// participant wrote an entry, which is what stops a peer redirecting somebody
/// else's payout or writing an expense in their name.
class SplitsSigner {
  SplitsSigner({Ed25519? algorithm}) : _algorithm = algorithm ?? Ed25519();

  final Ed25519 _algorithm;

  /// An Ed25519 seed is 32 bytes, and so is the public key it yields — which
  /// is also the length §9.4 requires of a `creatorKey`.
  static const int seedBytes = 32;

  /// The base64url public key for [seed], in the encoding §9.4 wants.
  Future<String> publicKeyFromSeed(List<int> seed) async {
    final pair = await _algorithm.newKeyPairFromSeed(seed);
    final key = await pair.extractPublicKey();
    return encode(key.bytes);
  }

  /// The participant id a device signing with [seed] speaks as (§10.7): the
  /// one its public key derives.
  Future<String> participantIdFromSeed(List<int> seed) async =>
      protocol.participantId(await publicKeyFromSeed(seed))!;

  /// The host's signer for [seed], in the shape `splitz` asks for.
  ///
  /// Ed25519 is deterministic, so re-signing an unchanged entry yields the same
  /// bytes. That is what keeps a sealed blob idempotent: the same entry pushed
  /// twice is one blob, not two.
  Future<String> Function(List<int>) signerFor(List<int> seed) =>
      (List<int> message) async {
        final pair = await _algorithm.newKeyPairFromSeed(seed);
        final signature = await _algorithm.sign(message, keyPair: pair);
        return encode(signature.bytes);
      };

  /// Whether [entry]'s signature, made on the bill [billId], verifies against
  /// [publicKey]. A signature made on any other bill does not (§10.6).
  ///
  /// False when the entry is unsigned, when either input is malformed, and when
  /// the signature simply does not match — all of which mean the same thing to
  /// §10.7: this entry was not written by the holder of that key.
  Future<bool> verifyEntry(
    Map<String, dynamic> entry,
    String publicKey, {
    required String billId,
  }) async {
    final signature = entry['sig'];
    if (signature is! String) return false;

    final List<int> signatureBytes;
    final List<int> keyBytes;
    try {
      signatureBytes = decode(signature);
      keyBytes = decode(publicKey);
    } on FormatException {
      return false;
    }
    if (keyBytes.length != seedBytes) return false;

    // §10.6's message, from the protocol rather than from this encoder's key
    // order, so a signature made here verifies in another implementation and
    // one made there verifies here.
    final List<int> message;
    try {
      message = utf8.encode(protocol.signingMessage(entry, billId));
    } on protocol.SplitError {
      return false;
    }

    try {
      return await _algorithm.verify(
        message,
        signature: Signature(
          signatureBytes,
          publicKey: SimplePublicKey(keyBytes, type: KeyPairType.ed25519),
        ),
      );
    } on Object {
      return false;
    }
  }

  /// Answers every signature question a fold of [entries] on the bill
  /// [billId] can ask, in advance.
  ///
  /// `foldLog` takes a **synchronous** verifier, and this curve operation is
  /// asynchronous. Rather than block, every pair the fold can ask about is
  /// verified first and the fold is then handed a pure lookup — which is also
  /// what §10.7 requires of it, since a verifier that answered differently on
  /// two devices would fold two different bills from one log.
  ///
  /// The pairs are enumerable without restating §10.7. The protocol asks
  /// whether an entry verifies against a key that same entry states —
  /// `creatorKey` on a create, `participant.identityKey` on a join — and, once
  /// its author is bound, against the author's key (§10.3), which is one of
  /// the keys the author states in a create or a join of their own. Any other
  /// pair is a question this build did not expect, and [VerifiedLog.unanswered]
  /// records it rather than letting a `false` pass for an answer.
  ///
  /// An answer depends only on the entry and the key, so answers are kept
  /// across calls: a bill folded again after one new entry verifies one entry,
  /// not every entry it holds.
  Future<VerifiedLog> prepare(
    Iterable<Map<String, dynamic>> entries, {
    required String billId,
  }) async {
    final all = entries.toList();
    final keysOf = <String, Set<String>>{};
    for (final entry in all) {
      final author = entry['author'];
      if (author is! String) continue;
      if (entry['kind'] == 'createBill') {
        final key = entry['creatorKey'];
        if (key is String) keysOf.putIfAbsent(author, () => {}).add(key);
      } else if (entry['kind'] == 'joinBill') {
        final participant = entry['participant'];
        if (participant is Map &&
            participant['id'] == author &&
            participant['identityKey'] is String) {
          keysOf
              .putIfAbsent(author, () => {})
              .add(participant['identityKey'] as String);
        }
      }
    }
    final answers = <String, bool>{};
    for (final entry in all) {
      final keys = {..._keysStatedBy(entry), ...?keysOf[entry['author']]};
      for (final key in keys) {
        final known = _knownKey(entry, key, billId);
        answers[_pair(entry, key)] = known == null
            ? await verifyEntry(entry, key, billId: billId)
            : _known[known] ??= await verifyEntry(entry, key, billId: billId);
      }
    }
    return VerifiedLog._(answers);
  }

  /// Answers already computed, keyed by [_knownKey].
  final Map<String, bool> _known = {};

  /// What an answer depends on: the exact message signed, the signature and
  /// the key. Not the id — an entry carrying a copied id and signature over
  /// other content would otherwise file its `false` under the genuine
  /// entry's name. Null when the entry has no §10.6 message, and then nothing
  /// is kept.
  static String? _knownKey(
    Map<String, dynamic> entry,
    String key,
    String billId,
  ) {
    try {
      return '${protocol.signingMessage(entry, billId)}\u0000${entry['sig']}\u0000$key';
    } on protocol.SplitError {
      return null;
    }
  }

  /// The keys an entry states about itself.
  static Iterable<String> _keysStatedBy(Map<String, dynamic> entry) sync* {
    final creatorKey = entry['creatorKey'];
    if (creatorKey is String) yield creatorKey;
    final participant = entry['participant'];
    if (participant is Map) {
      final identityKey = participant['identityKey'];
      if (identityKey is String) yield identityKey;
    }
  }

  /// Identifies one (entry, key) question.
  ///
  /// `sig` is part of it: §9.5's digest excludes the signature, so a signed
  /// entry and its unsigned twin share an id while giving opposite answers.
  /// One (copy, key) question. The copy is its whole canonical encoding: two
  /// copies may share an id and a signature over different content, and an
  /// answer filed under the id alone lets the copy that fails overwrite the
  /// one that verifies. An entry §9.3 cannot encode is filed under its plain
  /// encoding; ingress refuses it before any fold applies it.
  static String _pair(Map<String, dynamic> entry, String key) {
    String copy;
    try {
      copy = protocol.canonicalJson(entry);
    } on protocol.SplitError {
      copy = jsonEncode(entry);
    }
    return '$copy\u0000$key';
  }

  /// Unpadded base64url, as §9.4 writes a key.
  static String encode(List<int> bytes) =>
      base64Url.encode(bytes).replaceAll('=', '');

  /// The alphabet §9.4, §10.6 and §11.1 write, and no other.
  static const String _alphabet =
      'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_';

  /// Reads unpadded base64url, and tolerates trailing `=` padding.
  ///
  /// The alphabet is checked before decoding. `base64Url.decode` also accepts
  /// standard base64's `+` and `/` and silently re-encodes them as `-` and
  /// `_`, so without this a key in the wrong alphabet decodes here, is stored
  /// as it arrived, and then matches nothing the protocol produced.
  static List<int> decode(String value) {
    var body = value;
    while (body.endsWith('=')) {
      body = body.substring(0, body.length - 1);
    }
    for (final unit in body.codeUnits) {
      if (!_alphabet.contains(String.fromCharCode(unit))) {
        throw FormatException('not base64url', value);
      }
    }
    return base64Url.decode(body.padRight((body.length + 3) & ~3, '='));
  }
}

/// Signature answers for one log, ready for a synchronous fold.
class VerifiedLog {
  VerifiedLog._(this._answers);

  final Map<String, bool> _answers;
  final List<String> _unanswered = [];

  /// The verifier to hand `foldLog`.
  protocol.VerifySignature get verify =>
      (Map<String, dynamic> entry, String key) {
        final answer = _answers[SplitsSigner._pair(entry, key)];
        if (answer == null) {
          // Not a pair this build anticipated. Recorded rather than silently
          // answered: `false` here reads as "that signature is invalid", which
          // is a different and much quieter claim than "nobody asked".
          _unanswered.add(SplitsSigner._pair(entry, key));
          return false;
        }
        return answer;
      };

  /// Pairs the fold asked about that were never verified.
  ///
  /// Empty on every fold this build understands. Anything here means the
  /// protocol's §10.7 now asks a question [SplitsSigner.prepare] does not
  /// anticipate, and the identities it reported are not to be trusted.
  List<String> get unanswered => List.unmodifiable(_unanswered);
}
