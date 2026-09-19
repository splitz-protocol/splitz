/// Scanned payloads and sealed frames (SPEC.md §11.2 and §11.3).
library;

import 'dart:convert';

import 'canonical_json.dart';
import 'errors.dart';
import 'invite.dart';

/// What a version-40 QR code holds in byte mode at error-correction level M.
///
/// Enforced on decode as well as encode: a cap applied only when writing
/// bounds what an implementation emits rather than what it accepts, which is
/// the wrong direction for a trust boundary.
const int payloadCap = 2331;

/// The payload format version this library writes and the highest it reads.
const int payloadVersion = 1;

const String _tabPrefix = 'splitz1:';
const String _deltaPrefix = 'splitzd1:';

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
    return base64Url.decode(text.padRight((text.length + 3) & ~3, '='));
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
  if (prefix != _tabPrefix && prefix != _deltaPrefix) {
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
  if (s.startsWith(_tabPrefix)) {
    prefix = _tabPrefix;
  } else if (s.startsWith(_deltaPrefix)) {
    prefix = _deltaPrefix;
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
    invite: prefix == _tabPrefix && map['invite'] is Map
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
