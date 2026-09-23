/// Scanned payloads and sealed frames (SPEC.md §11.2 and §11.3).
library;

import 'dart:convert';

import 'canonical_json.dart';
import 'errors.dart';
import 'invite.dart';
import 'log.dart';
import 'sha256.dart';

/// What a version-40 QR code holds in byte mode at error-correction level M.
///
/// Enforced on decode as well as encode: a cap applied only when writing
/// bounds what an implementation emits rather than what it accepts, which is
/// the wrong direction for a trust boundary.
const int payloadCap = 2331;

/// The payload format version this library writes and the highest it reads.
const int payloadVersion = 1;

/// The prefix a whole bill's payload carries (§11.2).
const String billPrefix = 'splitz1:';

/// The prefix a delta carries. A delta never carries an invite.
const String deltaPrefix = 'splitzd1:';

/// The sealed frame version.
const int sealedVersion = 1;

/// XChaCha20-Poly1305 nonce length, in bytes.
const int nonceBytes = 24;

/// Poly1305 tag length, in bytes.
const int tagBytes = 16;

const String _b64urlAlphabet =
    'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_';

String _b64(List<int> raw) => base64UrlEncode(raw).replaceAll('=', '');

List<int>? _unb64(String text) {
  for (final unit in text.codeUnits) {
    if (!_b64urlAlphabet.contains(String.fromCharCode(unit))) return null;
  }
  try {
    final raw = base64Url.decode(text.padRight((text.length + 3) & ~3, '='));
    // §9.4: a text that is not its bytes' canonical encoding is refused as one
    // that does not decode.
    return _b64(raw) == text ? raw : null;
  } on FormatException {
    return null;
  }
}

/// A decoded payload.
class ScannedPayload {
  const ScannedPayload({
    required this.prefix,
    required this.version,
    required this.log,
    this.invite,
  });

  final String prefix;
  final int version;
  final List<Object?> log;
  final Map<String, dynamic>? invite;
}

/// Encodes [body] under [prefix].
String encodePayload(String prefix, Map<String, dynamic> body) {
  if (prefix != billPrefix && prefix != deltaPrefix) {
    raise(SplitCode.payloadNotAPayload, 'No such payload prefix: "$prefix"');
  }
  final encoded = _b64(utf8.encode(canonicalJson(body)));
  if (encoded.length > payloadCap) {
    raise(SplitCode.payloadTooLarge,
        'A payload of ${encoded.length} characters exceeds $payloadCap');
  }
  return '$prefix$encoded';
}

/// Decodes a scanned payload.
ScannedPayload decodePayload(String text) {
  // Padding is stripped before the prefix is matched and before the size is
  // measured, so padding does not count toward the cap.
  final s = stripScanPadding(text);
  final String prefix;
  if (s.startsWith(billPrefix)) {
    prefix = billPrefix;
  } else if (s.startsWith(deltaPrefix)) {
    prefix = deltaPrefix;
  } else {
    raise(SplitCode.payloadNotAPayload, 'Not a payload: "$text"');
  }

  final encoded = s.substring(prefix.length);
  if (encoded.length > payloadCap) {
    raise(SplitCode.payloadTooLarge,
        'A payload of ${encoded.length} characters exceeds $payloadCap');
  }

  final raw = _unb64(encoded);
  if (raw == null) {
    raise(SplitCode.payloadDamaged, 'The body is not base64url');
  }
  Object? body;
  try {
    body = jsonDecode(utf8.decode(raw));
  } on Object {
    raise(SplitCode.payloadDamaged, 'The body is not JSON');
  }
  if (body is! Map) {
    raise(SplitCode.payloadDamaged, 'A payload body is an object');
  }
  if (!withinDepth(body, maxDocumentDepth)) {
    raise(SplitCode.payloadDamaged,
        'A payload body nests deeper than $maxDocumentDepth');
  }
  final map = body.cast<String, dynamic>();

  final version = map['v'];
  if (version is! int || version < 1) {
    raise(SplitCode.payloadDamaged, 'A payload states its version');
  }
  if (version > payloadVersion) {
    raise(
        SplitCode.payloadFutureVersion,
        'This payload is version $version; this reader implements '
        '$payloadVersion');
  }

  final log = map['log'];
  if (log is! List) {
    raise(SplitCode.payloadMissingBody, 'A payload carries a log');
  }

  return ScannedPayload(
    prefix: prefix,
    version: version,
    log: log,
    // §11.2. Only the bill prefix carries an invite, and only an object is
    // one: a delta's reader already holds a key, and a second one arriving
    // from a peer names a bill and a key that reader never chose.
    invite: prefix == billPrefix && map['invite'] is Map
        ? (map['invite'] as Map).cast<String, dynamic>()
        : null,
  );
}

/// What a sealed frame states before the cipher runs.
class SealedFrame {
  const SealedFrame({
    required this.version,
    required this.nonce,
    required this.bodyBytes,
  });

  final int version;

  /// Derived from the plaintext, so one entry always seals to one blob.
  final String nonce;
  final int bodyBytes;
}

/// Parses a sealed frame: a version byte, a 24-byte nonce, then the cipher's
/// output.
///
/// Opening it is the host's; what this checks is the frame two wallets must
/// agree on before the cipher runs.
SealedFrame parseSealedFrame(String text) {
  final s = stripScanPadding(text);
  if (s.isEmpty) {
    raise(SplitCode.sealedMalformed, 'An empty frame');
  }
  final raw = _unb64(s);
  if (raw == null) {
    raise(SplitCode.sealedMalformed, 'A frame is base64url');
  }
  if (raw.length < 1 + nonceBytes + tagBytes) {
    raise(SplitCode.sealedMalformed,
        'A frame of ${raw.length} bytes cannot hold a nonce and a tag');
  }
  final version = raw[0];
  // A version of zero is a malformed frame and not a future format: telling
  // somebody their app is too old sends them to an update that will not help.
  if (version < 1) {
    raise(SplitCode.sealedMalformed, 'A frame states a version of at least 1');
  }
  if (version > sealedVersion) {
    raise(
        SplitCode.sealedFutureVersion,
        'This frame is version $version; this reader implements '
        '$sealedVersion');
  }
  return SealedFrame(
    version: version,
    nonce: _b64(raw.sublist(1, 1 + nonceBytes)),
    bodyBytes: raw.length - 1 - nonceBytes,
  );
}

/// What a peer has not seen, and whether it fits one square (§14.5).
///
/// Three answers, not two. A peer holding everything and a peer holding none
/// of a log too long to encode are opposite states, and one value for both
/// tells somebody their bill is up to date while entries on it have never
/// reached them.
sealed class Delta {
  const Delta(this.entryCount);

  /// How many entries the peer is missing. Zero only for [NothingMissing].
  final int entryCount;
}

/// The peer holds every entry this device does.
class NothingMissing extends Delta {
  const NothingMissing() : super(0);
}

/// The entries the peer has not seen, as one square.
class DeltaSquare extends Delta {
  const DeltaSquare(this.uri, super.entryCount);

  final String uri;
}

/// The peer is behind by more than one square can carry. What this needs is a
/// relay, not a smaller camera.
class TooBigForOneSquare extends Delta {
  const TooBigForOneSquare(super.entryCount, this.code);

  /// The refusal §11.2 gave, `payload_too_large` when it is the cap.
  final String code;
}

/// Computes what [theyHave] is missing from [entries] (§14.5).
///
/// A delta carries no invite: its reader already holds the key (§11.2).
Delta deltaFor(List<Map<String, dynamic>> entries, Set<String> theyHave) {
  final missing = [
    for (final e in orderEntries(entries))
      if (!theyHave.contains(e['id'])) e,
  ];
  if (missing.isEmpty) return const NothingMissing();
  try {
    final uri =
        encodePayload(deltaPrefix, <String, dynamic>{'v': 1, 'log': missing});
    return DeltaSquare(uri, missing.length);
  } on SplitError catch (e) {
    return TooBigForOneSquare(missing.length, e.code);
  }
}

// --- §11.3, the producing half ---------------------------------------------
//
// The cipher itself is the host's: this library depends on nothing that could
// hold a key. What it owns is every value §11.3 fixes — the plaintext, the
// nonce derived from it, the frame around the cipher's output, and the
// channel. Those are the parts two devices must agree on byte for byte, and a
// wallet that derives them a second time is a second place for them to drift.

/// The bytes an entry is sealed as (§11.3).
///
/// Canonical JSON (§9.3), UTF-8. The canonical form is what makes the seal
/// idempotent: two devices holding one entry produce one plaintext, therefore
/// one nonce, therefore one blob.
List<int> sealedPlaintext(Map<String, dynamic> entry) =>
    utf8.encode(canonicalJson(entry));

/// The nonce [plaintext] seals under: `SHA-256(plaintext)` truncated to
/// [nonceBytes] (§11.3).
///
/// Derived rather than random, so the same entry always seals to the same
/// blob and a relay stores it once however many times it is pushed. Two
/// different plaintexts never share a nonce, which is the one condition the
/// cipher requires.
///
/// **Never derive this from the entry id.** Two payloads can carry one id, and
/// a stream cipher under a repeated (key, nonce) hands a relay the xor of two
/// plaintexts it holds no key for.
List<int> sealedNonce(List<int> plaintext) =>
    sha256(plaintext).sublist(0, nonceBytes);

/// Frames a sealed [body] for a transport (§11.3).
///
/// [body] is the cipher's own output — ciphertext followed by its tag — and
/// this library never produces it. The frame is one version byte, the
/// [nonceBytes]-byte [nonce], then [body], the whole unpadded base64url. A
/// reader knowing the fixed nonce and tag lengths splits it apart with no
/// length fields.
String frameSealed(List<int> nonce, List<int> body) {
  if (nonce.length != nonceBytes) {
    raise(SplitCode.sealedMalformed,
        'A nonce is $nonceBytes bytes, got ${nonce.length}');
  }
  if (body.length < tagBytes) {
    raise(SplitCode.sealedMalformed,
        'A body carries at least a $tagBytes-byte tag, got ${body.length}');
  }
  return _b64(<int>[sealedVersion, ...nonce, ...body]);
}

/// The channel a bill's blobs are pushed to and pulled from (§11.3).
///
/// The bill id's SHA-256, lower-case hex. A digest rather than the id itself,
/// because the id is a live address printed in every invite: every
/// participant knows it and computes the same channel, and a relay that only
/// ever sees traffic cannot run it backwards.
String channelFor(String billId) => sha256Hex(utf8.encode(billId));
