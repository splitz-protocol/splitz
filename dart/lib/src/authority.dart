/// What a signature covers, and who a participant is (SPEC.md §10.6, §10.7).
library;

import 'dart:convert';

import 'canonical_json.dart';
import 'ordering.dart';
import 'sha256.dart';

/// The domain separator an entry's signature covers.
const String entrySigningDomain = 'splitz-entry-v2';

/// The bytes an entry's signature covers on the bill [billId] (§10.6).
///
/// The bill is part of the message because an entry does not name it: a
/// participant's id and key are the same on every bill, so without it a
/// signature made on one bill verifies on any other.
///
/// `sig` is excluded because it is the output, and `v` because an entry does
/// not carry its own format version through an implementation's object model:
/// a reader re-encodes with the version it writes, so a signature covering `v`
/// would stop verifying for every existing entry the day it changed.
///
/// Returned as text. A test asserting that a signature verified would pass in
/// two implementations that disagree about the bytes, each checking its own.
String signingMessage(Map<String, dynamic> entry, String billId) {
  final body = <String, dynamic>{
    for (final e in entry.entries)
      if (e.key != 'sig' && e.key != 'v') e.key: e.value,
  };
  return entrySigningDomain +
      canonicalJson(<String, dynamic>{'bill': billId, 'entry': body});
}

/// Whether the host takes an entry's signature to verify against a key.
///
/// The curve operation is the host's (§13). This library fixes the message and
/// everything around the answer, never the answer itself.
typedef VerifySignature = bool Function(Map<String, dynamic> entry, String key);

/// The domain separator a participant id's digest covers (§10.7).
const String participantIdDomain = 'splitz-participant-v1';

/// The bytes [value] encodes when it is the canonical unpadded base64url of
/// exactly [length] bytes, or null.
List<int>? canonicalBytes(Object? value, int length) {
  if (value is! String || value.isEmpty) return null;
  const alphabet =
      'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_';
  for (final unit in value.codeUnits) {
    if (!alphabet.contains(String.fromCharCode(unit))) return null;
  }
  try {
    final raw = base64Url.decode(value.padRight((value.length + 3) & ~3, '='));
    // Canonical, so re-encoding reproduces it exactly: a last character with
    // an unused bit set decodes to the same bytes as another spelling.
    if (raw.length != length ||
        base64UrlEncode(raw).replaceAll('=', '') != value) {
      return null;
    }
    return raw;
  } on FormatException {
    return null;
  }
}

/// The participant id a key speaks as (§10.7), or null for a text that is
/// not a canonical 32-byte key:
/// `base64url( SHA-256( "splitz-participant-v1" || key bytes )[0..16] )`.
///
/// Two keys cannot derive one id, so no second key can claim a participant
/// this binds.
String? participantId(String key) {
  final raw = canonicalBytes(key, 32);
  if (raw == null) return null;
  final digest = sha256([...utf8.encode(participantIdDomain), ...raw]);
  return base64UrlEncode(digest.sublist(0, 16)).replaceAll('=', '');
}

/// Which key, if any, speaks for each participant (§10.7).
class Identities {
  const Identities(this.bound);

  /// Participant id to the key bound to it.
  final Map<String, String> bound;
}

/// Resolves identities from the entry set alone.
///
/// Never from arrival order, never from anything a device has seen before: two
/// devices resolving this differently fold a different bill from one log.
Identities resolveIdentities(
  List<Map<String, dynamic>> entries,
  Map<String, dynamic> create,
  VerifySignature verify,
) {
  final creator = create['author'] as String;
  final creatorKey = create['creatorKey'] as String?;

  // The creator is bound by the invite, not by a join: the bill's id is the
  // digest of the entry that states their key, so it needs no prior
  // acquaintance. The signature requirement is not ornamental — absent it,
  // `creatorKey` is a number the author typed.
  final bound = <String, String>{};
  if (creatorKey != null && verify(create, creatorKey)) {
    bound[creator] = creatorKey;
  }

  // A key is bound by a self-claim: a join whose author is the participant it
  // carries, whose id is the one that participant's key derives, and whose
  // signature verifies against that key. An entry naming somebody else proves
  // nothing about them, and a key cannot claim an id it does not derive, so
  // no second key can claim a bound participant.
  for (final e in entries) {
    if (e['kind'] != 'joinBill') continue;
    final p = e['participant'];
    final id = p is Map ? p['id'] : null;
    final key = p is Map ? p['identityKey'] : null;
    if (id is! String || e['author'] != id || id == creator) continue;
    if (key is! String || participantId(key) != id || !verify(e, key)) {
      continue;
    }
    bound[id] = key;
  }

  return Identities({for (final id in sortedUtf8(bound.keys)) id: bound[id]!});
}
