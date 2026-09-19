/// What a signature covers, and who a participant is (SPEC.md §10.6, §10.7).
library;

import 'canonical_json.dart';
import 'ordering.dart';

/// The domain separator an entry's signature covers.
const String entrySigningDomain = 'splitz-entry-v1';

/// The bytes an entry's signature covers (§10.6).
///
/// `sig` is excluded because it is the output, and `v` because an entry does
/// not carry its own format version through an implementation's object model:
/// a reader re-encodes with the version it writes, so a signature covering `v`
/// would stop verifying for every existing entry the day it changed.
///
/// Returned as text. A test asserting that a signature verified would pass in
/// two implementations that disagree about the bytes, each checking its own.
String signingMessage(Map<String, dynamic> entry) {
  final body = <String, dynamic>{
    for (final e in entry.entries)
      if (e.key != 'sig' && e.key != 'v') e.key: e.value,
  };
  return entrySigningDomain + canonicalJson(body);
}

/// Whether the host takes an entry's signature to verify against a key.
///
/// The curve operation is the host's (§13). This library fixes the message and
/// everything around the answer, never the answer itself.
typedef VerifySignature = bool Function(Map<String, dynamic> entry, String key);

/// Which key, if any, speaks for each participant (§10.7).
class Identities {
  const Identities(this.bound, this.contested);

  /// Participant id to the key bound to it.
  final Map<String, String> bound;

  /// Ids two keys each claim. Neither is bound: nothing inside the log says
  /// which is the person, and `at` is whatever its author wrote, so resolving
  /// by time hands the identity to whoever backdates furthest.
  final Set<String> contested;
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
  // carries, stating a key, whose signature verifies against that key. An
  // entry naming somebody else proves nothing about them, whoever signed it.
  final claims = <String, Set<String>>{};
  for (final e in entries) {
    if (e['kind'] != 'joinBill') continue;
    final p = (e['participant'] as Map?)?.cast<String, dynamic>() ?? {};
    final id = p['id'];
    final key = p['identityKey'];
    if (id is! String || key is! String || e['author'] != id) continue;
    if (!verify(e, key)) continue;
    claims.putIfAbsent(id, () => <String>{}).add(key);
  }

  final contested = <String>{};
  for (final id in sortedUtf8(claims.keys)) {
    // A join claiming the creator's id is not a rival claim; §10.7 refuses it
    // rather than contesting the one identity the invite proves.
    if (id == creator) continue;
    if (claims[id]!.length > 1) {
      contested.add(id);
    } else {
      bound[id] = claims[id]!.first;
    }
  }

  return Identities(bound, contested);
}
