/// Sealing what leaves the device, so a relay only ever holds ciphertext.
library;

import 'dart:convert';
import 'dart:typed_data';

import 'package:cryptography/cryptography.dart';
import 'package:splitz_core/splitz_core.dart' as protocol;

import 'keys.dart';
import 'signing.dart';

/// Seals a bill's entries under its key.
///
/// An entry crossing a wire is the graph of who ate with whom and who owes
/// whom — the one thing no server is meant to see. So an entry is sealed under
/// the bill's symmetric key before it is handed to any transport. A relay holds
/// the blob, its size, and when it changed; it cannot read a name, an amount,
/// or an address.
///
/// XChaCha20-Poly1305: its 192-bit nonce is long enough to be chosen freely for
/// every blob without the birthday-bound concern a 96-bit nonce carries, so
/// there is no per-key nonce counter to persist and keep consistent across
/// devices that never coordinate.
class SplitsSealing {
  SplitsSealing({Cipher? cipher})
    : _cipher = cipher ?? Xchacha20.poly1305Aead();

  final Cipher _cipher;

  /// Blob layout version, the first byte of every blob, so a later change to
  /// the framing or the cipher is recognised rather than fed to the wrong
  /// decoder. Bumped only when an older reader would misread a newer blob.
  static const int blobVersion = 1;

  /// Seals one entry into a base64url string safe for a URL, a QR payload or a
  /// JSON field.
  ///
  /// The nonce is derived from the sealed bytes, so the same entry always seals
  /// to the same blob while two different entries never share a nonce — the one
  /// condition the cipher requires. Idempotence is what keeps a channel finite:
  /// a relay keyed by blob content stores an entry once however often it is
  /// pushed. A random nonce would avoid reuse and make every re-push a new
  /// blob; deriving from the entry's *id* would reuse a nonce whenever two
  /// payloads carried one id, and a stream cipher under a repeated (key, nonce)
  /// hands a relay that holds no key the xor of the two plaintexts.
  Future<String> seal(Map<String, dynamic> entry, String billKey) async {
    final key = await _secretKey(billKey);
    // Canonical, not this encoder's key order. The nonce comes from these
    // bytes, so a device ordering keys differently would seal one entry into a
    // second blob that every device opens perfectly and none can recognise as
    // the same entry.
    final clear = utf8.encode(protocol.canonicalJson(entry));
    final box = await _cipher.encrypt(
      clear,
      secretKey: key,
      nonce: _nonceFor(clear),
    );
    // version ++ nonce ++ ciphertext ++ mac. The cipher's nonce and mac
    // lengths are fixed, so a reader splits it apart with no length fields.
    final concatenation = box.concatenation();
    final framed = Uint8List(1 + concatenation.length)
      ..[0] = blobVersion
      ..setRange(1, 1 + concatenation.length, concatenation);
    return SplitsSigner.encode(framed);
  }

  /// Opens a blob back into an entry.
  ///
  /// A blob sealed under another key, or altered in transit, fails the Poly1305
  /// check and raises rather than returning a wrong plaintext.
  Future<Map<String, dynamic>> open(String blob, String billKey) async {
    final bytes = _decode(blob, 'blob');
    if (bytes.isEmpty) throw const SealingException('Empty blob');

    final version = bytes[0];
    if (version != blobVersion) {
      throw SealingException(
        'Blob is format v$version; this build reads v$blobVersion',
      );
    }

    final nonceLength = _cipher.nonceLength;
    final macLength = _cipher.macAlgorithm.macLength;
    final body = bytes.sublist(1);
    if (body.length < nonceLength + macLength) {
      throw const SealingException('Blob is too short to be a sealed entry');
    }

    final List<int> clear;
    try {
      clear = await _cipher.decrypt(
        SecretBox.fromConcatenation(
          body,
          nonceLength: nonceLength,
          macLength: macLength,
        ),
        secretKey: await _secretKey(billKey),
      );
    } on SecretBoxAuthenticationError {
      // Wrong key or tampered bytes — indistinguishable, and one thing to a
      // caller: this blob is not for this bill.
      throw const SealingException(
        'Could not open: wrong key, or the blob was altered',
      );
    }

    // A blob that authenticates but does not hold an entry — genuinely corrupt,
    // or crafted by a key-holder to break decoding — is reported the same way,
    // so a sync skips it like any other unopenable blob instead of the
    // exception aborting the whole pull.
    final Object? decoded;
    try {
      decoded = jsonDecode(utf8.decode(clear));
    } on FormatException catch (e) {
      throw SealingException('Opened blob is not JSON: ${e.message}');
    }
    if (decoded is! Map<String, dynamic>) {
      throw const SealingException('Opened blob is not an entry');
    }
    return decoded;
  }

  /// A nonce bound to [plaintext], the cipher's nonce length long.
  ///
  /// §11.3 fixes this value, so it is taken from the protocol rather than
  /// derived here: two devices that disagree about it produce blobs neither
  /// can open, and a second implementation is a second place to disagree.
  List<int> _nonceFor(List<int> plaintext) {
    final nonce = protocol.sealedNonce(plaintext);
    if (nonce.length != _cipher.nonceLength) {
      throw SealingException(
        'This cipher takes a ${_cipher.nonceLength}-byte nonce; '
        'section 11.3 fixes ${nonce.length}',
      );
    }
    return nonce;
  }

  Future<SecretKey> _secretKey(String billKey) async {
    final bytes = _decode(billKey, 'key');
    if (bytes.length != SplitsKeys.keyLengthBytes) {
      throw SealingException(
        'Key is ${bytes.length} bytes; expected ${SplitsKeys.keyLengthBytes}',
      );
    }
    return _cipher.newSecretKeyFromBytes(bytes);
  }

  static Uint8List _decode(String value, String what) {
    try {
      return Uint8List.fromList(SplitsSigner.decode(value));
    } on FormatException catch (e) {
      throw SealingException('Malformed base64url $what: ${e.message}');
    }
  }
}

/// Raised when a blob cannot be sealed or opened.
class SealingException implements Exception {
  const SealingException(this.message);

  final String message;

  @override
  String toString() => 'SealingException: $message';
}
