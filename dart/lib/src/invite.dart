/// Invite URIs (SPEC.md §11.1).
///
/// The grammar is exact and is never delegated to a general URI library. A
/// general parser brings its own answers to questions this format has to fix
/// itself, and two libraries answer them differently.
library;

import 'dart:convert';

import 'errors.dart';

const String _prefix = 'splitz://join';

/// The invite format version this library writes and the highest it reads.
const int inviteVersion = 1;

/// A bill id in an invite holds at most this many base64url characters. A
/// derived id is 22; the cap is generous rather than tight.
const int maxInviteBillId = 128;

/// Exactly the code points §11.1 calls scan padding.
///
/// Not a general trim: Dart's `String.trim()` strips every Unicode
/// `White_Space` character *and* U+FEFF, Rust's `str::trim()` strips that
/// whitespace and leaves U+FEFF. One QR code with a leading byte order mark
/// would then be an invite to one reader and not the other.
const List<int> scanPadding = [0x09, 0x0A, 0x0D, 0x20, 0xFEFF];

const String _b64urlAlphabet =
    'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_';

/// What an invite carries.
class Invite {
  const Invite({
    required this.billId,
    required this.key,
    this.name = '',
    this.expiry,
  });

  final String billId;

  /// The symmetric key the bill's contents are encrypted under. This protocol
  /// carries the key; it does not encrypt.
  final String key;

  final String name;
  final int? expiry;
}

/// Strips scan padding from both ends of [text], and nothing else.
///
/// The same characters anywhere inside are content.
String stripScanPadding(String text) {
  var start = 0;
  var end = text.length;
  while (start < end && scanPadding.contains(text.codeUnitAt(start))) {
    start++;
  }
  while (end > start && scanPadding.contains(text.codeUnitAt(end - 1))) {
    end--;
  }
  return text.substring(start, end);
}

/// Percent-decodes one parameter value.
///
/// `+` is a literal plus, never a space: that convention belongs to HTML form
/// encoding, and a `+` inside a bill id or a key must survive the round trip. A
/// malformed escape is left literal.
String _percentDecode(String value) {
  final out = <int>[];
  final raw = utf8.encode(value);
  var i = 0;
  while (i < raw.length) {
    if (raw[i] == 0x25 && i + 2 < raw.length) {
      final hex = String.fromCharCodes(raw.sublist(i + 1, i + 3));
      final byte = int.tryParse(hex, radix: 16);
      if (byte != null) {
        out.add(byte);
        i += 3;
        continue;
      }
    }
    out.add(raw[i]);
    i++;
  }
  return utf8.decode(out, allowMalformed: true);
}

bool _isB64Url(String value) {
  for (final unit in value.codeUnits) {
    if (!_b64urlAlphabet.contains(String.fromCharCode(unit))) return false;
  }
  return true;
}

/// §11.1. [value] decodes as unpadded base64url: the alphabet, a length that
/// is not one more than a multiple of four, and no unused bit set in its last
/// character, so it is the canonical encoding of its bytes.
bool _decodesAsB64Url(String value) {
  if (value.isEmpty || !_isB64Url(value) || value.length % 4 == 1) {
    return false;
  }
  try {
    final raw = base64Url.decode(value.padRight((value.length + 3) & ~3, '='));
    return base64UrlEncode(raw).replaceAll('=', '') == value;
  } on FormatException {
    // Dart's decoder refuses an unused bit set, where others decode it.
    return false;
  }
}

/// Parses an invite.
Invite parseInvite(String text) {
  final s = stripScanPadding(text);

  // The scheme and host are matched case-sensitively.
  if (!s.startsWith(_prefix)) {
    raise(SplitCode.inviteNotAnInvite, 'Not an invite: "$text"');
  }
  final rest = s.substring(_prefix.length);
  // Nothing may follow `join` but an optional query.
  if (rest.isNotEmpty && !rest.startsWith('?')) {
    raise(SplitCode.inviteNotAnInvite, 'Not an invite: "$text"');
  }
  final query = rest.startsWith('?') ? rest.substring(1) : '';

  // Where a parameter appears more than once, the first occurrence wins: the
  // alternative lets one code present one version to a reader that scans
  // forwards and another to a reader that does not.
  final fields = <String, String>{};
  if (query.isNotEmpty) {
    for (final pair in query.split('&')) {
      final eq = pair.indexOf('=');
      final key = eq < 0 ? pair : pair.substring(0, eq);
      final value = eq < 0 ? '' : pair.substring(eq + 1);
      if (key.isNotEmpty) fields.putIfAbsent(key, () => _percentDecode(value));
    }
  }

  final rawVersion = fields['v'];
  if (rawVersion == null || !RegExp(r'^[0-9]+$').hasMatch(rawVersion)) {
    raise(SplitCode.inviteMissingVersion, 'An invite states its version');
  }
  // Bounded before it is compared (§11.1): a numeral wider than 64 bits takes
  // `int.parse` outside SplitError, and a scanned code must not reach that.
  final version = int.tryParse(rawVersion);
  if (version == null) {
    raise(SplitCode.inviteMissingVersion,
        'A version fits in a signed 64-bit integer, got "$rawVersion"');
  }
  // No sign, no padding, no whitespace.
  if (rawVersion != version.toString() || version < 1) {
    raise(SplitCode.inviteMissingVersion,
        'A version is a bare decimal integer, got "$rawVersion"');
  }
  if (version > inviteVersion) {
    raise(
        SplitCode.inviteFutureVersion,
        'This invite is version $version; this reader implements '
        '$inviteVersion');
  }

  final billId = fields['b'] ?? '';
  if (billId.isEmpty) {
    raise(SplitCode.inviteMissingBillId, 'An invite names a bill');
  }
  // A derived id is base64url, so nothing outside that alphabet came from a
  // derivation. This refuses a literal `+`, which percent-decoding leaves
  // intact.
  if (billId.length > maxInviteBillId || !_isB64Url(billId)) {
    raise(SplitCode.inviteBadBillId, 'Not a bill id: "$billId"');
  }

  final key = fields['k'] ?? '';
  if (key.isEmpty || !_decodesAsB64Url(key)) {
    raise(SplitCode.inviteMissingKey, 'An invite carries a base64url key');
  }

  int? expiry;
  if (fields.containsKey('x')) {
    final raw = fields['x']!;
    if (!RegExp(r'^[0-9]+$').hasMatch(raw)) {
      raise(SplitCode.inviteBadExpiry, 'Not an expiry: "$raw"');
    }
    expiry = int.tryParse(raw);
    // A bare decimal integer, as `v` is: no padding, and within range.
    // `int.tryParse` returns null above the range rather than saturating.
    if (expiry == null || raw != expiry.toString()) {
      raise(SplitCode.inviteBadExpiry,
          'An expiry is a bare decimal integer within 64 bits, got "$raw"');
    }
  }

  return Invite(
      billId: billId, key: key, name: fields['n'] ?? '', expiry: expiry);
}

const String _unreserved =
    'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~';
const String _inviteExtra = "!*'()";

String _escape(String text) {
  final out = StringBuffer();
  for (final byte in utf8.encode(text)) {
    final ch = String.fromCharCode(byte);
    if (_unreserved.contains(ch) || _inviteExtra.contains(ch)) {
      out.write(ch);
    } else {
      out.write('%${byte.toRadixString(16).toUpperCase().padLeft(2, '0')}');
    }
  }
  return out.toString();
}

/// Renders an invite.
///
/// An encoder refuses what [parseInvite] refuses: a bound enforced only on
/// decode lets a caller build a URI no reader accepts, and the caller learns
/// of it from somebody else's scanner.
String renderInvite(Invite invite) {
  final expiry = invite.expiry;
  if (expiry != null && expiry < 0) {
    raise(SplitCode.inviteBadExpiry, 'An expiry is not negative, got $expiry');
  }
  if (invite.billId.isEmpty ||
      invite.billId.length > maxInviteBillId ||
      !_isB64Url(invite.billId)) {
    raise(SplitCode.inviteBadBillId, 'Not a bill id: "${invite.billId}"');
  }
  if (invite.key.isEmpty || !_decodesAsB64Url(invite.key)) {
    raise(SplitCode.inviteMissingKey, 'An invite carries a base64url key');
  }
  final parts = <String>[
    'v=$inviteVersion',
    'b=${_escape(invite.billId)}',
    'k=${_escape(invite.key)}',
    if (invite.name.isNotEmpty) 'n=${_escape(invite.name)}',
    if (invite.expiry != null) 'x=${invite.expiry}',
  ];
  return '$_prefix?${parts.join('&')}';
}
