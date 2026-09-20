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

  /// Whether [entry]'s signature verifies against [publicKey].
  ///
  /// False when the entry is unsigned, when either input is malformed, and when
  /// the signature simply does not match — all of which mean the same thing to
  /// §10.7: this entry was not written by the holder of that key.
  Future<bool> verifyEntry(Map<String, dynamic> entry, String publicKey) async {
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
      message = utf8.encode(protocol.signingMessage(entry));
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

  /// Answers every signature question a fold of [entries] can ask, in advance.
  ///
  /// `foldLog` takes a **synchronous** verifier, and this curve operation is
  /// asynchronous. Rather than block, every pair the fold can ask about is
  /// verified first and the fold is then handed a pure lookup — which is also
  /// what §10.7 requires of it, since a verifier that answered differently on
  /// two devices would fold two different bills from one log.
  ///
  /// The pairs are enumerable without restating §10.7: the protocol only ever
  /// asks whether an entry verifies against a key *that same entry states* —
  /// `creatorKey` on a create, `participant.identityKey` on a join. Any other
  /// pair is a question this build did not expect, and [VerifiedLog.unanswered]
  /// records it rather than letting a `false` pass for an answer.
  Future<VerifiedLog> prepare(Iterable<Map<String, dynamic>> entries) async {
    final answers = <String, bool>{};
    for (final entry in entries) {
      for (final key in _keysStatedBy(entry)) {
        answers[_pair(entry, key)] = await verifyEntry(entry, key);
      }
    }
    return VerifiedLog._(answers);
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
  static String _pair(Map<String, dynamic> entry, String key) =>
      '${entry['id']}\u0000${entry['sig']}\u0000$key';

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
