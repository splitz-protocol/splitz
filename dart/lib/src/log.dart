/// The log (SPEC.md §10).
///
/// A bill is materialised from an append-only log. Entries are never modified:
/// correcting an expense appends an amendment naming the one it replaces, and
/// removing it appends a withdrawal. There is no state to conflict, only facts
/// to union.
library;

import 'dart:convert';

import 'authority.dart';
import 'canonical_json.dart';
import 'errors.dart';
import 'instant.dart';
import 'money.dart';
import 'ordering.dart';
import 'serialization.dart';
import 'sha256.dart';
import 'split.dart';

const Set<String> entryKinds = {
  'createBill',
  'joinBill',
  'addExpense',
  'amendEntry',
  'voidEntry',
  'recordPayment',
  'confirmPayment',
  'vouchIdentity',
  'setRate',
};

/// The payload each kind carries, and no other.
const Map<String, String> payloadForKind = {
  'joinBill': 'participant',
  'addExpense': 'expense',
  'recordPayment': 'payment',
  'confirmPayment': 'confirmation',
  'vouchIdentity': 'vouch',
  'setRate': 'rate',
};

const List<String> _payloadNames = [
  'rate',
  'expense',
  'payment',
  'confirmation',
  'vouch',
];

/// Every member that is a payload, including the one no kind lists as
/// ambiguous (§10.1).
///
/// `_payloadNames` drives the ambiguity check and omits `participant`; the
/// type check below must not, because an `amendEntry` may carry any of them
/// and every later pass indexes what it finds.
const List<String> _payloadMembers = [..._payloadNames, 'participant'];

/// The domain separator the bill id digest covers.
const String billIdDomain = 'splitz-bill-id-v1';

/// Who a confirmation method speaks for, whether it needs a reference, and
/// whether it settles a debt (§10.5).
class ConfirmationRule {
  const ConfirmationRule(this.speaksFor, this.needsReference, this.settles);

  /// `from`, `to`, or null when any participant may author it.
  final String? speaksFor;
  final bool needsReference;
  final bool settles;
}

const Map<String, ConfirmationRule> confirmationMethods = {
  'recipientConfirmed': ConfirmationRule('to', false, true),
  'walletReceived': ConfirmationRule('to', false, true),
  // Names a public transaction any participant can check, so it speaks for
  // nobody in particular.
  'onChain': ConfirmationRule(null, true, true),
  // A payer saying they paid is the claim of the record, not evidence for it.
  'payerAttested': ConfirmationRule('from', false, false),
};

/// The bill id derived from the entry that opens a bill (§9.4).
///
/// `id`, `sig` and `v` are excluded: the first is the output, the second
/// covers the first, and the third is restated by whichever reader re-encodes
/// the entry.
String deriveBillId(Map<String, dynamic> entry) =>
    _deriveId(billIdDomain, entry);

String _deriveId(String domain, Map<String, dynamic> entry) {
  final body = <String, dynamic>{
    for (final e in entry.entries)
      if (e.key != 'id' && e.key != 'sig' && e.key != 'v') e.key: e.value,
  };
  final digest = sha256(utf8.encode(domain + canonicalJson(body)));
  return base64UrlEncode(digest.sublist(0, 16)).replaceAll('=', '');
}

bool _isB64UrlOfLength(Object? value, int bytes) {
  if (value is! String || value.isEmpty) return false;
  const alphabet =
      'ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_';
  for (final unit in value.codeUnits) {
    if (!alphabet.contains(String.fromCharCode(unit))) return false;
  }
  try {
    return base64Url
            .decode(value.padRight((value.length + 3) & ~3, '='))
            .length ==
        bytes;
  } on FormatException {
    return false;
  }
}

/// The members of a payload that name a participant or an entry (§10.1).
const Map<String, List<String>> _idMembersOf = {
  'participant': ['id'],
  'expense': ['id', 'paidBy'],
  'payment': ['id', 'from', 'to'],
  'confirmation': ['paymentId'],
  'vouch': ['subject'],
};

void _checkPayloadIds(Map<String, dynamic> payload, String wanted) {
  for (final member in _idMembersOf[wanted] ?? const <String>[]) {
    final value = payload[member];
    if (value != null && value is! String) {
      raise(SplitCode.billTypeError, 'A $wanted states $member as a string');
    }
  }
  // §4's id lists name participants. A member that cannot be read is not
  // dropped: dropping reassigns that participant's share to the others.
  final split = payload['split'];
  if (split is Map) checkIdLists(split.cast<String, dynamic>());
}

/// The domain separator an entry id's digest covers (§9.5).
///
/// Different from §9.4's so that a `createBill` id and any other entry's id
/// are drawn from different spaces and neither can be presented as the other.
const String entryIdDomain = 'splitz-entry-id-v1';

/// The id of [entry] (§9.5): the digest of the entry with `id`, `sig` and `v`
/// removed.
String deriveEntryId(Map<String, dynamic> entry) =>
    _deriveId(entryIdDomain, entry);

/// Checks an entry before it reaches a log (§10.1).
///
/// An entry carrying more than one payload is refused because the currency
/// fallback and the fold would otherwise read different ones. One carrying
/// none is refused because removing a member makes an entry's canonical
/// encoding sort higher than the same entry with it, so the merge would keep
/// the stripped copy.
/// `value` as a map, or an empty one.
///
/// For the passes that read inside a payload before it has been decoded: §10.1
/// types the payload itself, not its members, so anything under it is whatever
/// a peer wrote and a cast there is a language-level error waiting for one
/// entry to arrive.
Map<String, dynamic> _mapOf(Object? value) =>
    value is Map ? value.cast<String, dynamic>() : <String, dynamic>{};

/// `value` as a list, or an empty one. See [_mapOf].
List<Object?> _listOf(Object? value) => value is List ? value : const [];

Map<String, dynamic> checkEntry(Object? raw) {
  if (raw is! Map) {
    raise(SplitCode.billTypeError, 'An entry is an object, got $raw');
  }
  final entry = raw.cast<String, dynamic>();

  final kind = entry['kind'];
  if (kind is! String || !entryKinds.contains(kind)) {
    raise(SplitCode.billUnknownEntryKind, 'No such entry kind: $kind');
  }

  final carried = [
    for (final p in _payloadNames)
      if (entry.containsKey(p)) p
  ];
  if (carried.length > 1) {
    raise(SplitCode.billAmbiguousEntry,
        'An entry carries ${carried.join(" and ")}');
  }

  // §2.3, at entry ingress: a lone surrogate anywhere in the entry breaks the
  // §10.2 order the merge and the fold both depend on.
  checkScalarValues(entry);

  final wanted = payloadForKind[kind];
  if (wanted != null && !entry.containsKey(wanted)) {
    raise(SplitCode.billMissingEntryPayload, 'A $kind carries a $wanted');
  }
  if ((kind == 'voidEntry' || kind == 'amendEntry') &&
      (entry['targetId'] == null || entry['targetId'] == '')) {
    raise(SplitCode.billMissingEntryPayload, 'A $kind names a target');
  }
  if (entry.containsKey('targetId') && entry['targetId'] is! String) {
    raise(SplitCode.billTypeError, 'A targetId is a string');
  }

  // §10.1. Every pass after this one indexes the payload without re-checking
  // it, and the fold's authorisation pass reads the target's payload to decide
  // who may withdraw an entry — so a scalar payload admitted here makes the
  // entry unwithdrawable and the bill unopenable.
  // Every payload an entry carries is an object, whatever the kind names. An
  // amendEntry carries the payload it replaces and no kind declares it, so
  // without this a scalar reaches the fold and is indexed there.
  for (final name in _payloadMembers) {
    if (entry.containsKey(name) && entry[name] is! Map) {
      raise(
          SplitCode.billTypeError, 'A $name is an object, got ${entry[name]}');
    }
  }
  if (wanted != null) {
    _checkPayloadIds((entry[wanted] as Map).cast<String, dynamic>(), wanted);
  }

  canonicalInstant(entry['at']);
  if (entry['id'] is! String) {
    raise(SplitCode.billTypeError, 'An entry states its id');
  }
  if (entry['author'] is! String) {
    raise(SplitCode.billTypeError, 'An entry states its author');
  }

  if (kind == 'createBill') {
    // The fold copies these into the bill document without re-reading them,
    // so they are decided here rather than at decode, where the whole bill
    // would be unopenable instead of this entry refused.
    if (entry.containsKey('name') && entry['name'] is! String) {
      raise(SplitCode.billTypeError, 'A bill states its name as a string');
    }
    checkCurrency(entry['currency']);
    // §9.1. An optional scalar does not read `null` as absent.
    final createMode =
        entry.containsKey('splitMode') ? entry['splitMode'] : 'equal';
    if (createMode is! String) {
      raise(SplitCode.billTypeError, 'A split mode is a string');
    }
    if (!billSplitModes.contains(createMode)) {
      raise(SplitCode.billUnknownSplitMode, 'No such split mode: $createMode');
    }
    // Both fields, or the entry is unbound and anybody could claim its id.
    if (!_isB64UrlOfLength(entry['creatorKey'], 32) ||
        !_isB64UrlOfLength(entry['nonce'], 16)) {
      raise(SplitCode.createUnbound,
          'A create entry carries a 32-byte key and a 16-byte nonce');
    }
    if (entry['id'] != deriveBillId(entry)) {
      raise(SplitCode.createIdNotDerived,
          'A create entry\'s id is the digest of the entry');
    }
  } else if (entry['id'] != deriveEntryId(entry)) {
    // §9.5. An id anyone may choose is an id anyone may take: without this,
    // re-pushing a copy of an entry with one member changed displaces the
    // genuine one under §10.2 rule 2.
    raise(SplitCode.entryIdNotDerived,
        'An entry\'s id is the digest of the entry');
  }
  return entry;
}

/// The total order: `at`, then `author`, then `id`, all ascending.
///
/// It never depends on arrival order or on local state.
List<Map<String, dynamic>> orderEntries(List<Map<String, dynamic>> entries) {
  final sorted = [...entries];
  sorted.sort((a, b) {
    final byAt = compareUtf8(a['at'] as String, b['at'] as String);
    if (byAt != 0) return byAt;
    final byAuthor = compareUtf8(a['author'] as String, b['author'] as String);
    if (byAuthor != 0) return byAuthor;
    final byId = compareUtf8(a['id'] as String, b['id'] as String);
    if (byId != 0) return byId;
    // §10.2. The order is total: a comparator that calls two unequal rows
    // equal leaves them to the host's sort, and Dart's is not stable.
    return compareUtf8(canonicalJson(a), canonicalJson(b));
  });
  return sorted;
}

/// Merges logs by set union, keyed by entry id (§10.2).
///
/// §10.1 is applied at ingress: an entry that does not carry the payload its
/// kind uses never enters the union.
///
/// A copy carrying a signature beats one that does not; otherwise the entry
/// whose canonical encoding sorts higher wins. Both parts are functions of the
/// two entries alone, which is what makes union commutative: resolving by
/// arrival makes the merged bill depend on the order two devices synced in.
MergeResult mergeLogs(List<List<Map<String, dynamic>>> logs) {
  final byId = <String, Map<String, dynamic>>{};
  final refused = <SetAside>[];
  for (final log in logs) {
    for (final entry in log) {
      // §10.1 at ingress. Removing a payload member makes an entry sort
      // higher under §9.3, so without this the stripped copy wins rule 2 and
      // displaces the genuine entry on every device.
      try {
        checkEntry(entry);
      } on SplitError catch (e) {
        // Coerced rather than cast: the refusal path must be total over
        // every value checkEntry refuses, including a non-string id.
        final reported = entry['id'];
        refused.add(SetAside(
            reported is String ? reported : '', e.code, 'refused at ingress'));
        continue;
      }
      final id = entry['id'] as String;
      final held = byId[id];
      if (held == null) {
        byId[id] = entry;
        continue;
      }
      final heldSigned = held.containsKey('sig');
      final entrySigned = entry.containsKey('sig');
      if (heldSigned != entrySigned) {
        byId[id] = heldSigned ? held : entry;
      } else {
        byId[id] = compareUtf8(canonicalJson(held), canonicalJson(entry)) >= 0
            ? held
            : entry;
      }
    }
  }
  refused.sort((a, b) {
    final byId = compareUtf8(a.id, b.id);
    return byId != 0 ? byId : compareUtf8(a.code, b.code);
  });
  return MergeResult(orderEntries(byId.values.toList()), refused);
}

/// A merged log and the entries §10.1 refused at ingress.
class MergeResult {
  const MergeResult(this.merged, this.refused);
  final List<Map<String, dynamic>> merged;
  final List<SetAside> refused;
}

/// An entry the fold could not apply, and why.
class SetAside {
  const SetAside(this.id, this.code, this.reason);
  final String id;
  final String code;
  final String reason;
}

/// An address a rejoin replaced.
///
/// A wallet must put one of these in front of the payer before settling to it.
class ReplacedAddress {
  const ReplacedAddress(this.id, this.from, this.to);
  final String id;
  final String? from;
  final String? to;
}

/// Everything folding a log reaches.
class FoldResult {
  const FoldResult({
    required this.bill,
    required this.creator,
    required this.replacedAddresses,
    required this.withdrawn,
    required this.setAside,
    required this.identities,
  });

  /// The materialised bill, as a wire-form map.
  final Map<String, dynamic> bill;

  /// Named by the withdrawal rules of §10.8.
  final String creator;

  final List<ReplacedAddress> replacedAddresses;

  /// A withdrawal is absent from the fold by design and is otherwise
  /// indistinguishable from an entry that was never written.
  final List<String> withdrawn;

  final List<SetAside> setAside;

  /// Which key speaks for each participant, and which ids two keys claim
  /// (§10.7).
  ///
  /// Empty when [foldLog] is given no verifier: §13 makes the curve operation
  /// the host's, so a fold that cannot check a signature reports no binding
  /// and no contest rather than claiming there are none.
  final Identities identities;
}

/// Materialises a bill from [rawEntries] (§10.3).
///
/// An entry that cannot be applied is set aside and reported, never raised as
/// a failure of the whole fold: the log merges by union, so one malformed
/// entry propagates to every device, and aborting on it would leave the bill
/// permanently unopenable — including unopenable to append the withdrawal that
/// would remove it.
FoldResult foldLog(List<Object?> rawEntries,
    {String? billId, VerifySignature? verify}) {
  if (rawEntries.isEmpty) {
    raise(SplitCode.logEmpty, 'A log with no entries opens no bill');
  }
  // §10.3. An entry that cannot be applied is set aside, never raised as a
  // failure of the whole fold: the log merges by union, so one malformed entry
  // reaches every device, and aborting leaves the bill unopenable — including
  // unopenable to append the withdrawal that would remove it.
  final admitted = <Map<String, dynamic>>[];
  final refusedAtIngress = <SetAside>[];
  for (final raw in rawEntries) {
    try {
      admitted.add(checkEntry(raw));
    } on SplitError catch (e) {
      final id = raw is Map ? raw['id'] : null;
      refusedAtIngress
          .add(SetAside(id is String ? id : '', e.code, 'refused at ingress'));
    }
  }
  // §10.3. One id names one entry in the fold as in the merge: a re-sent
  // entry would otherwise be applied twice, and one expense sent twice
  // doubles what everybody owes.
  final entries = orderEntries(mergeLogs([admitted]).merged);

  var creates = [
    for (final e in entries)
      if (e['kind'] == 'createBill') e
  ];
  if (billId != null) {
    creates = [
      for (final e in creates)
        if (e['id'] == billId) e
    ];
  }
  if (verify != null) {
    // §10.1. A host that verifies MUST check a create entry's signature
    // against the creatorKey that same entry states — the one key on a bill
    // that needs no prior acquaintance, because §9.4 binds it to the id.
    final unverified = [
      for (final e in creates)
        if (!verify(e, e['creatorKey'] as String? ?? '')) e
    ];
    for (final e in unverified) {
      refusedAtIngress.add(SetAside(e['id'] as String,
          SplitCode.unauthorizedEntry, 'a create entry whose signature fails'));
    }
    creates = [
      for (final e in creates)
        if (!unverified.contains(e)) e
    ];
  }
  if (creates.isEmpty) {
    raise(SplitCode.logNoCreate, 'A log holding no create entry opens no bill');
  }
  if (creates.length > 1) {
    // Anyone holding the invite can push in a create entry of their own, which
    // §9.4 admits because it is valid for a different bill.
    raise(SplitCode.ambiguousCreate,
        'A log holds ${creates.length} create entries and names no bill');
  }
  final create = creates.first;
  final creator = create['author'] as String;

  final currency = create['currency'];
  checkCurrency(currency);
  final billCurrency = currency as String;

  final mode = create.containsKey('splitMode') ? create['splitMode'] : 'equal';
  if (mode is! String || !{'equal', 'percentage'}.contains(mode)) {
    raise(SplitCode.billUnknownSplitMode, 'No such split mode: $mode');
  }

  final byId = {for (final e in entries) e['id'] as String: e};
  final setAside = <SetAside>[...refusedAtIngress];
  final amendments = <String, Map<String, dynamic>>{};
  final voided = <String>{};

  void aside(Map<String, dynamic> e, String code, String reason) =>
      setAside.add(SetAside(e['id'] as String, code, reason));

  // Amendments: authored by the author of their target, carrying a payload of
  // the target's kind. An amendment replaces its target wholesale, so one
  // carrying no payload silently deletes what it claims to correct.
  for (final e in entries) {
    if (e['kind'] != 'amendEntry') continue;
    final target = byId[e['targetId']];
    if (target == null) {
      aside(e, SplitCode.unknownEntry, 'amends an entry the log does not hold');
      continue;
    }
    if (e['author'] != target['author']) {
      aside(e, SplitCode.unauthorizedEntry, 'amends an entry it did not write');
      continue;
    }
    final wanted = payloadForKind[target['kind']];
    if (wanted != null && !e.containsKey(wanted)) {
      aside(e, SplitCode.amendKindMismatch,
          'carries no payload of its target\'s kind');
      continue;
    }
    amendments[e['targetId'] as String] = e;
  }

  Map<String, dynamic> effective(Map<String, dynamic> e) =>
      amendments[e['id']] ?? e;

  // Withdrawals, §10.8, in two stages.
  //
  // Authorisation first, because only a withdrawal that is allowed to stand
  // may take another one back. Resolving in force over every withdrawal lets
  // a stranger cancel a legitimate one: the fold would report their entry
  // refused and honour it in the same breath.
  final voids = [
    for (final e in entries)
      if (e['kind'] == 'voidEntry') e
  ];
  final authorised = <String, bool>{};
  for (final e in voids) {
    final target = byId[e['targetId']];
    if (target == null) {
      aside(e, SplitCode.unknownEntry,
          'withdraws an entry the log does not hold');
      authorised[e['id'] as String] = false;
      continue;
    }
    final kind = target['kind'];
    final Set<String?> allowed;
    switch (kind) {
      case 'addExpense':
        // An expense is entered by hand and duplicated by accident, and the
        // person who entered it may be asleep.
        allowed = {target['author'] as String, creator};
      case 'recordPayment':
        // Withdrawing a payment reopens a debt somebody believed settled, so
        // the creator is not given this.
        final pay = (target['payment'] as Map?)?.cast<String, dynamic>() ?? {};
        allowed = {
          target['author'] as String,
          pay['from'] as String?,
          pay['to'] as String?,
        };
      case 'joinBill':
        final p =
            (target['participant'] as Map?)?.cast<String, dynamic>() ?? {};
        allowed = {creator, p['id'] as String?};
      default:
        allowed = {target['author'] as String};
    }
    if (!allowed.contains(e['author'])) {
      aside(e, SplitCode.unauthorizedEntry, 'may not withdraw a $kind');
      authorised[e['id'] as String] = false;
      continue;
    }
    authorised[e['id'] as String] = true;
  }

  // A withdrawal is in force unless a later authorised withdrawal, itself in
  // force, names it. Resolved from the latest backwards, so by the time one is
  // considered every withdrawal that could name it has been decided.
  final inForce = <String, bool>{};
  for (final e in voids.reversed) {
    if (!(authorised[e['id']] ?? false)) {
      inForce[e['id'] as String] = false;
      continue;
    }
    inForce[e['id'] as String] = !voids.any((other) =>
        other['targetId'] == e['id'] && (inForce[other['id']] ?? false));
  }

  for (final e in voids) {
    if (inForce[e['id']] ?? false) voided.add(e['targetId'] as String);
  }

  // An amendment whose own entry was withdrawn is discarded with it, so the
  // entry it corrected reads as it was written. Collecting amendments before
  // withdrawals are resolved and applying them afterwards would leave a
  // retracted correction standing: the figure a person took back would be the
  // figure the bill shows.
  amendments.removeWhere((_, e) => voided.contains(e['id']));

  // Taking somebody off the bill. This runs after every other withdrawal is
  // resolved and before the joins are applied: a check made once the person is
  // gone is a check made too late.
  for (final e in entries) {
    if (e['kind'] != 'voidEntry' || !voided.contains(e['targetId'])) continue;
    final target = byId[e['targetId']]!;
    if (target['kind'] != 'joinBill') continue;
    final gone = _mapOf(target['participant'])['id'];
    var named = false;
    for (final other in entries) {
      if (voided.contains(other['id']) || other['kind'] == 'voidEntry') {
        continue;
      }
      final eff = effective(other);
      if (other['kind'] == 'addExpense') {
        // Total accessors, not casts: this pass runs before the expense is
        // decoded, so `split` and everything under it is whatever a peer
        // wrote. §10.1 types the payload itself; it does not type inside it.
        final ex = _mapOf(eff['expense']);
        final split = _mapOf(ex['split']);
        final pool = <Object?>{
          ..._listOf(split['among']),
          ..._mapOf(split['amounts']).keys,
          ..._mapOf(split['basisPoints']).keys,
          ..._mapOf(split['shareCounts']).keys,
          for (final item in _listOf(split['items']))
            ..._listOf(_mapOf(item)['sharedBy']),
        };
        if (ex['paidBy'] == gone || pool.contains(gone)) named = true;
      } else if (other['kind'] == 'recordPayment') {
        final pay = _mapOf(eff['payment']);
        if (pay['from'] == gone || pay['to'] == gone) named = true;
      } else if (other['kind'] == 'confirmPayment' && other['author'] == gone) {
        named = true;
      }
      if (named) break;
    }
    if (named) {
      // The fold cannot apply an entry naming somebody who is not on the bill,
      // so without this, removing the person who spent the most silently drops
      // every expense they paid for.
      voided.remove(e['targetId']);
      aside(e, SplitCode.participantStillNamed,
          'a surviving entry still names that participant');
    }
  }

  final live = [
    for (final e in entries)
      if (!voided.contains(e['id']) && e['kind'] != 'voidEntry') e,
  ];

  // §10.1. The latest live setRate decides, by §10.2's order, so the answer
  // is a function of the log and not of which device last spoke.
  Map<String, dynamic>? rate;
  for (final e in live) {
    if (e['kind'] != 'setRate') continue;
    final payload = effective(e)['rate'];
    try {
      decodeRate(payload);
    } on SplitError catch (err) {
      aside(e, err.code, 'carries a rate this reader cannot decode');
      continue;
    }
    rate = (payload as Map).cast<String, dynamic>();
  }

  // Participants in a pass of their own, before anything that references them.
  final participants = <String, Map<String, dynamic>>{};
  final replaced = <ReplacedAddress>[];
  for (final e in live) {
    if (e['kind'] != 'joinBill') continue;
    final p =
        (effective(e)['participant'] as Map?)?.cast<String, dynamic>() ?? {};
    final id = p['id'];
    // §9.1. An empty id is not a name anyone can be settled to: two readers
    // disagreeing about it fold different bills from one log.
    if (id is! String || id.isEmpty) {
      aside(e, SplitCode.billMissingEntryPayload, 'names no participant');
      continue;
    }
    // The decoder decides what a participant is, here rather than once the
    // document is assembled: a member it would refuse sets this entry aside
    // (§10.3) instead of making the whole bill undecodable.
    try {
      decodeParticipant(p);
    } on SplitError catch (err) {
      aside(e, err.code, 'carries a participant this reader cannot decode');
      continue;
    }
    if (participants.containsKey(id) && e['author'] != id) {
      // Without this, one join naming another participant's id and carrying
      // your own address redirects every later settlement to that person.
      aside(e, SplitCode.unauthorizedEntry, 'changes a record it does not own');
      continue;
    }
    if (participants.containsKey(id) &&
        participants[id]!['payTo'] != p['payTo']) {
      replaced.add(ReplacedAddress(
          id, participants[id]!['payTo'] as String?, p['payTo'] as String?));
    }
    participants[id] = p;
  }

  final expenses = <Map<String, dynamic>>[];
  final payments = <Map<String, dynamic>>[];
  for (final e in live) {
    final eff = effective(e);
    if (e['kind'] == 'addExpense') {
      final ex = {...(eff['expense'] as Map).cast<String, dynamic>()};
      // An amount that states no currency is denominated by the fold. One
      // that states another is set aside, never restamped: that would keep
      // the count and change the unit. §9.1 falls back only when the member
      // is absent, so a present value that is not a currency is an entry that
      // cannot be applied, and §10.3 sets those aside rather than raising.
      if (!ex.containsKey('currency')) {
        ex['currency'] = billCurrency;
      } else if (!isCurrency(ex['currency'])) {
        aside(e, SplitCode.billBadCurrency,
            'states a value that is not a currency');
        continue;
      } else if (ex['currency'] != billCurrency) {
        aside(e, SplitCode.currencyMismatch,
            'states a currency the bill does not use');
        continue;
      }
      if (!participants.containsKey(ex['paidBy'])) {
        aside(e, SplitCode.unknownParticipant,
            'paid by somebody not on the bill');
        continue;
      }
      try {
        final decoded =
            decodeExpense(ex, billCurrency, participants.keys.toSet());
        // §4 is what turns an expense into what each person owes, and §5 runs
        // it downstream of this fold. An expense whose split §4 refuses cannot
        // be applied, so it is set aside here rather than raising out of
        // `netBalances` once the bill is already built.
        splitExpense(decoded.amount, decoded.split);
      } on SplitError catch (err) {
        aside(e, err.code, 'carries an expense this reader cannot apply');
        continue;
      }
      expenses.add(ex);
    } else if (e['kind'] == 'recordPayment') {
      final pay = {...(eff['payment'] as Map).cast<String, dynamic>()};
      // A payment moves both parties' balances, so without this any holder of
      // the invite could clear a debt neither of them had settled.
      if (e['author'] != pay['from'] && e['author'] != pay['to']) {
        aside(e, SplitCode.unauthorizedPayment, 'written by neither party');
        continue;
      }
      if (!participants.containsKey(pay['from']) ||
          !participants.containsKey(pay['to'])) {
        aside(
            e, SplitCode.unknownParticipant, 'names somebody not on the bill');
        continue;
      }
      if (pay['from'] == pay['to']) {
        aside(e, SplitCode.selfPayment, 'pays its own author');
        continue;
      }
      if (!pay.containsKey('currency')) {
        pay['currency'] = billCurrency;
      } else if (!isCurrency(pay['currency'])) {
        aside(e, SplitCode.billBadCurrency,
            'states a value that is not a currency');
        continue;
      } else if (pay['currency'] != billCurrency) {
        aside(e, SplitCode.currencyMismatch,
            'states a currency the bill does not use');
        continue;
      }
      try {
        decodePayment(pay, billCurrency, participants.keys.toSet());
      } on SplitError catch (err) {
        aside(e, err.code, 'carries a payment this reader cannot decode');
        continue;
      }
      payments.add(pay);
    }
  }

  // Confirmations in a pass of their own, once every payment is on the bill: a
  // confirmation may arrive before the payment it vouches for, and a single
  // pass would set aside one that is merely early.
  final known = {for (final p in payments) p['id'] as String};
  final confirmed = <String>{};
  for (final e in live) {
    if (e['kind'] != 'confirmPayment') continue;
    final c =
        (effective(e)['confirmation'] as Map?)?.cast<String, dynamic>() ?? {};
    final rule = confirmationMethods[c['method']];
    if (rule == null) {
      aside(
          e, SplitCode.billUnknownConfirmationMethod, 'method ${c['method']}');
      continue;
    }
    if (!known.contains(c['paymentId'])) {
      aside(e, SplitCode.unknownPayment,
          'vouches for a payment the bill does not hold');
      continue;
    }
    if (!participants.containsKey(e['author'])) {
      aside(e, SplitCode.unknownParticipant,
          'written by somebody not on the bill');
      continue;
    }
    final pay = payments.firstWhere((p) => p['id'] == c['paymentId']);
    if (rule.speaksFor != null && e['author'] != pay[rule.speaksFor]) {
      // A confirmation's whole weight is in who gave it, so a method anyone
      // may claim is a method that says nothing.
      aside(e, SplitCode.unauthorizedConfirmation,
          '${c['method']} speaks for the payment\'s ${rule.speaksFor}');
      continue;
    }
    final reference = c['reference'];
    if (rule.needsReference && !(reference is String && reference.isNotEmpty)) {
      // One that says a payment is on a chain without saying where contains no
      // chain. A number or a list is not a transaction id either, and reading
      // "present" three different ways settles a debt on one device and
      // leaves it open on another.
      aside(e, SplitCode.confirmationMissingReference,
          '${c['method']} names no transaction');
      continue;
    }
    if (rule.settles) confirmed.add(c['paymentId'] as String);
  }

  setAside.sort((a, b) {
    final byId = compareUtf8(a.id, b.id);
    return byId != 0 ? byId : compareUtf8(a.code, b.code);
  });

  // §10.7, over the same entry set the bill was materialised from. Without a
  // verifier nothing can be decided, and nothing is claimed.
  final identities = verify == null
      ? const Identities({}, {})
      : resolveIdentities(entries, create, verify);

  return FoldResult(
    bill: {
      'v': billVersion,
      'id': create['id'],
      'name': create['name'] ?? '',
      'currency': billCurrency,
      'splitMode': mode,
      'participants': [
        for (final id in sortedUtf8(participants.keys)) participants[id],
      ],
      'expenses': expenses,
      'payments': payments,
      'confirmedPayments': sortedUtf8(confirmed),
      if (rate != null) 'rate': rate,
    },
    creator: creator,
    replacedAddresses: replaced,
    withdrawn: sortedUtf8(voided),
    setAside: setAside,
    identities: identities,
  );
}
