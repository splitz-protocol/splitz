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
  'setRate',
};

/// The payload each kind carries, and no other.
const Map<String, String> payloadForKind = {
  'joinBill': 'participant',
  'addExpense': 'expense',
  'recordPayment': 'payment',
  'confirmPayment': 'confirmation',
  'setRate': 'rate',
};

const List<String> _payloadNames = [
  'rate',
  'expense',
  'payment',
  'confirmation',
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

/// §10.4: the member naming what each kind of entry is about, which an
/// amendment may not change.
const Map<String, (String, String)> _amendedSubject = {
  'joinBill': ('participant', 'id'),
  'addExpense': ('expense', 'id'),
  'recordPayment': ('payment', 'id'),
  'confirmPayment': ('confirmation', 'paymentId'),
};

const Map<String, ConfirmationRule> confirmationMethods = {
  'recipientConfirmed': ConfirmationRule('to', false, true),
  'walletReceived': ConfirmationRule('to', false, true),
  // The recipient saying they opened the transaction and saw it land. A
  // shielded payment is visible to nobody else, so nobody else can say it.
  'onChain': ConfirmationRule('to', true, true),
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

/// §9.4: canonical, so re-encoding reproduces it exactly.
bool _isB64UrlOfLength(Object? value, int bytes) =>
    canonicalBytes(value, bytes) != null;

/// §2.2 and §9.3. Every number in an entry is an integer a signed 64-bit
/// value holds.
///
/// A JSON reader holds a large integer as a double, so `9223372036854775808`
/// and `9.223372036854775808e18` arrive here as one value: one outside the
/// 64-bit range is `amount_overflow` however it was written, and the code
/// follows from the value alone. Any other non-integer is
/// `canonical_json_float`, which is what §9.3's encoding refuses.
void _checkNumbers(Object? value) {
  if (value is double) {
    if (!value.isFinite || value.abs() >= 9223372036854775808.0) {
      raise(SplitCode.amountOverflow, 'A number past 64 bits: $value');
    }
    raise(SplitCode.canonicalJsonFloat, 'A number that is not an integer');
  } else if (value is Map) {
    for (final v in value.values) {
      _checkNumbers(v);
    }
  } else if (value is List) {
    for (final v in value) {
      _checkNumbers(v);
    }
  }
}

/// The members of a payload that name a participant or an entry (§10.1).
const Map<String, List<String>> _idMembersOf = {
  'participant': ['id'],
  'expense': ['id', 'paidBy'],
  'payment': ['id', 'from', 'to'],
  'confirmation': ['paymentId'],
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

/// The domain separator a payment digest covers (§10.5).
const String paymentDigestDomain = 'splitz-payment-v1';

/// What a confirmation binds (§10.5): the digest of the payment payload a
/// record carries, as written, less the `id` the confirmation names beside
/// it.
///
/// A confirmation carries it as `record`, and applies only while the record
/// the bill holds under that id still says this. One withdrawn and written
/// again, or amended since, is a payment nobody confirmed.
String paymentDigest(Map<String, dynamic> payment) =>
    _deriveId(paymentDigestDomain, payment);

/// `value` as a map, or an empty one.
///
/// For the passes that read inside a payload before it has been decoded: §10.1
/// types the payload itself, not its members, so anything under it is whatever
/// a peer wrote and a cast there is a language-level error waiting for one
/// entry to arrive.
Map<String, dynamic> _mapOf(Object? value) =>
    value is Map ? value.cast<String, dynamic>() : <String, dynamic>{};

/// §10.3 step 5. Whether [id] is one its [author] minted: the author's
/// participant id, `:`, then anything.
///
/// Only the author writes entries under such an id, so a copy of it in
/// anybody else's entry is set aside whatever `at` either states. §10.2's
/// order is each author's to write, and deciding whose id it is by that order
/// hands it to whoever backdates furthest.
///
/// An author whose id holds `:` mints nothing: `<payer>:<txid>` as a
/// participant id would otherwise own `<payer>:<txid>:<recipient>`, the id the
/// payer's own send record carries.
bool ownsId(String author, String id) =>
    !author.contains(':') && id.startsWith('$author:');

/// `value` as a list, or an empty one. See [_mapOf].
List<Object?> _listOf(Object? value) => value is List ? value : const [];

/// Checks an entry before it reaches a log (§10.1).
///
/// An entry carrying more than one payload is refused because the currency
/// fallback and the fold would otherwise read different ones. One carrying
/// none is refused because removing a member makes an entry's canonical
/// encoding sort higher than the same entry with it, so the merge would keep
/// the stripped copy.
Map<String, dynamic> checkEntry(Object? raw) {
  // §10.1's checks run in the order it states, so an entry wrong in two ways
  // is refused for the same one by every reader.
  if (raw is! Map) {
    raise(SplitCode.billTypeError, 'An entry is an object, got $raw');
  }
  final entry = raw.cast<String, dynamic>();

  // §10.1. An entry arriving over a relay (§11.3) never passes §11.2's cap,
  // and every pass below walks it — deriving its id encodes it. A depth
  // nobody bounded is a stack the peer chose.
  if (!withinDepth(entry, maxEntryDepth)) {
    raise(SplitCode.billTypeError, 'An entry nests deeper than $maxEntryDepth');
  }

  final kind = entry['kind'];
  if (kind is! String || !entryKinds.contains(kind)) {
    raise(SplitCode.billUnknownEntryKind, 'No such entry kind: $kind');
  }

  // §10.1. A signature is a string or absent. Anything else is a third state
  // §10.2 would have to rank, and `null` sorts above every string, so it would
  // win every merge it entered.
  if (entry.containsKey('sig') && entry['sig'] is! String) {
    raise(SplitCode.billTypeError, 'A signature is a string');
  }

  // §10.1. `v` sits outside the id (§9.5), so a copy with any value keeps the
  // honest id; one that is not an integer would reach the canonical encoding
  // the merge and the order compare, and stop there. A Dart `int` holds at
  // most 2^63-1, and a larger one arrives as a double.
  if (entry.containsKey('v')) {
    final v = entry['v'];
    if (v is! int || v < 1) {
      raise(SplitCode.billTypeError,
          'An entry version is an integer from 1 to 2^63-1');
    }
  }

  // §2.3, at entry ingress: a lone surrogate anywhere in the entry breaks the
  // §10.2 order the merge and the fold both depend on.
  checkScalarValues(entry);
  // §2.2 and §9.3, at entry ingress, before any member is read.
  _checkNumbers(entry);

  final carried = [
    for (final p in _payloadNames)
      if (entry.containsKey(p)) p
  ];
  if (carried.length > 1) {
    raise(SplitCode.billAmbiguousEntry,
        'An entry carries ${carried.join(" and ")}');
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
  if (entry.containsKey('targetId') && entry['targetId'] is! String) {
    raise(SplitCode.billTypeError, 'A targetId is a string');
  }

  final wanted = payloadForKind[kind];
  if (wanted != null && !entry.containsKey(wanted)) {
    raise(SplitCode.billMissingEntryPayload, 'A $kind carries a $wanted');
  }
  if (wanted != null) {
    _checkPayloadIds((entry[wanted] as Map).cast<String, dynamic>(), wanted);
  }
  if ((kind == 'voidEntry' || kind == 'amendEntry') &&
      (entry['targetId'] == null || entry['targetId'] == '')) {
    raise(SplitCode.billMissingEntryPayload, 'A $kind names a target');
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
    // §9.4: the digest of the bill key it was made with, when it states one.
    if (entry.containsKey('keyDigest') &&
        !_isB64UrlOfLength(entry['keyDigest'], 32)) {
      raise(SplitCode.billTypeError, 'A key digest is 32 bytes');
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

/// The total order (§10.2): the instant `at` names, then `at` as written,
/// then `author`, then `id`, all ascending.
///
/// By the instant rather than the text: §9.3 admits `t` and `z` and any
/// number of fractional digits, and the text of an earlier instant can sort
/// after a later one. The text then breaks ties between spellings of one
/// instant. It never depends on arrival order or on local state.
List<Map<String, dynamic>> orderEntries(List<Map<String, dynamic>> entries) {
  final keyed = [
    for (final e in entries) (_instantKey(e['at'] as String), e),
  ];
  keyed.sort((x, y) {
    final (ka, a) = x;
    final (kb, b) = y;
    final byInstant = compareUtf8(ka, kb);
    if (byInstant != 0) return byInstant;
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
  return [for (final (_, e) in keyed) e];
}

/// The canonical instant [at] names, or [at] itself when it names none: an
/// entry that reached here unchecked still sorts, and every checked one sorts
/// by its instant.
String _instantKey(String at) {
  try {
    return canonicalInstant(at);
  } on SplitError {
    return at;
  }
}

/// Merges logs by set union, keyed by entry id and signature (§10.2).
///
/// §10.1 is applied at ingress: an entry that does not carry the payload its
/// kind uses never enters the union.
///
/// A signed copy beats an unsigned one. Two signed copies with different
/// signatures are both kept: §9.5's digest does not cover `sig`, so nothing in
/// the pair says which is the author's, and resolving it by order would hand
/// the id to whichever signature sorts higher. The fold decides with the key
/// (§10.3). Copies sharing a signature, or all unsigned, resolve to the one
/// whose canonical encoding sorts higher. Every rule is a function of the
/// copies alone, which is what makes union commutative.
MergeResult mergeLogs(List<List<Object?>> logs) {
  // Id, then signature (null when unsigned), to the copy held.
  final copies = <String, Map<String?, Map<String, dynamic>>>{};
  final refused = <SetAside>[];
  for (final log in logs) {
    for (final raw in log) {
      // A log decoded from a peer is a list of whatever the peer sent (§11),
      // and an element that is not an object is refused like any other
      // malformed entry rather than failing the merge.
      if (raw is! Map) {
        refused.add(const SetAside('', SplitCode.billTypeError));
        continue;
      }
      final entry = raw.cast<String, dynamic>();
      // §10.1 at ingress. Removing a payload member makes an entry sort
      // higher under §9.3, so without this the stripped copy wins rule 3 and
      // displaces the genuine entry on every device.
      try {
        checkEntry(entry);
      } on SplitError catch (e) {
        // Coerced rather than cast: the refusal path must be total over
        // every value checkEntry refuses, including a non-string id.
        final reported = entry['id'];
        refused.add(SetAside(reported is String ? reported : '', e.code));
        continue;
      }
      final held = copies.putIfAbsent(entry['id'] as String, () => {});
      final sig = entry['sig'] as String?;
      final other = held[sig];
      if (other == null ||
          compareUtf8(canonicalJson(entry), canonicalJson(other)) > 0) {
        held[sig] = entry;
      }
    }
  }
  final out = <Map<String, dynamic>>[];
  for (final held in copies.values) {
    final signed = [
      for (final e in held.entries)
        if (e.key != null) e.value
    ];
    out.addAll(signed.isNotEmpty ? signed : held.values);
  }
  refused.sort((a, b) {
    final byId = compareUtf8(a.id, b.id);
    return byId != 0 ? byId : compareUtf8(a.code, b.code);
  });
  return MergeResult(orderEntries(out), refused);
}

/// A merged log and the entries §10.1 refused at ingress.
class MergeResult {
  const MergeResult(this.merged, this.refused);
  final List<Map<String, dynamic>> merged;
  final List<SetAside> refused;
}

/// An entry the fold could not apply, and why.
class SetAside {
  const SetAside(this.id, this.code);
  final String id;

  /// The §12 code, and nothing else.
  ///
  /// The code is what a wallet turns into a sentence for its user (§1), so a
  /// sentence written here would be a second source for that text, in English
  /// only, that no wallet should render.
  final String code;
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
    required this.paymentAuthors,
    required this.paymentDigests,
    required this.expenseEntries,
    required this.expenseAuthors,
    required this.paymentEntries,
    required this.rateEntry,
    required this.rateAuthor,
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

  /// Which key speaks for each participant (§10.7).
  ///
  /// Empty when [foldLog] is given no verifier: §13 makes the curve operation
  /// the host's, so a fold that cannot check a signature reports no binding
  /// rather than claiming there is none.
  final Identities identities;

  /// Who wrote each payment record on the bill, by the payment's id.
  ///
  /// Not part of the bill document, which restates neither author nor
  /// instant (§10.5). §14.4 withholds only for a record the payer wrote.
  final Map<String, String> paymentAuthors;

  /// What each payment record on the bill says, by the payment's id: the
  /// [paymentDigest] a confirmation of it carries as `record` (§10.5).
  final Map<String, String> paymentDigests;

  /// The entry that introduced each expense on the bill, by the expense's own
  /// id: what an amendment or a withdrawal of it targets (§10.3).
  ///
  /// Taken from the fold rather than from the log, which holds entries the
  /// fold set aside under the same expense id.
  final Map<String, String> expenseEntries;

  /// Who wrote each expense on the bill, by the expense's own id.
  final Map<String, String> expenseAuthors;

  /// The entry that recorded each payment on the bill, by the payment's id.
  final Map<String, String> paymentEntries;

  /// The `setRate` entry whose rate the bill carries, or null with no rate.
  final String? rateEntry;

  /// Who wrote that `setRate` (§14.2: a payer is shown who set the rate).
  final String? rateAuthor;
}

/// Where a participant is paid (§10.3 step 4): the address of their first
/// payout when they declare any, their payTo otherwise, or null. A value the
/// decoder never read is taken as none, not cast.
String? _destination(Map<String, dynamic> participant) {
  final payouts = participant['payouts'];
  final Object? address = payouts is List && payouts.isNotEmpty
      ? _mapOf(payouts.first)['address']
      : participant['payTo'];
  return address is String ? address : null;
}

/// The copy whose canonical encoding sorts highest (§10.2 rule 3).
Map<String, dynamic> _highest(List<Map<String, dynamic>> copies) =>
    copies.reduce(
        (a, b) => compareUtf8(canonicalJson(a), canonicalJson(b)) >= 0 ? a : b);

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
      refusedAtIngress.add(SetAside(id is String ? id : '', e.code));
    }
  }
  // §10.3. Every copy the merge kept, several under one id when their
  // signatures differ.
  final copies = mergeLogs([admitted]).merged;

  var creates = [
    for (final e in copies)
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
    // that needs no prior acquaintance, because §9.4 binds it to the id. A
    // create is refused only when no copy of it verifies.
    final kept = [
      for (final e in creates)
        if (verify(e, e['creatorKey'] as String? ?? '')) e
    ];
    final keptIds = {for (final e in kept) e['id'] as String};
    for (final id in sortedUtf8({
      for (final e in creates)
        if (!keptIds.contains(e['id'])) e['id'] as String
    })) {
      refusedAtIngress.add(SetAside(id, SplitCode.unauthorizedEntry));
    }
    creates = kept;
  }
  final createIds = {for (final e in creates) e['id'] as String};
  if (createIds.isEmpty) {
    raise(SplitCode.logNoCreate, 'A log holding no create entry opens no bill');
  }
  if (createIds.length > 1) {
    // Anyone holding the invite can push in a create entry of their own, which
    // §9.4 admits because it is valid for a different bill.
    raise(SplitCode.ambiguousCreate,
        'A log holds ${createIds.length} create entries and names no bill');
  }
  final create = _highest(creates);
  final creator = create['author'] as String;

  // §10.7, over every copy: a withdrawal does not undo a claim, and a copy
  // nobody applies is still evidence that was made. Without a verifier
  // nothing can be decided, and nothing is claimed.
  final identities = verify == null
      ? const Identities({})
      : resolveIdentities(copies, create, verify);

  // §10.3. One id names one entry: a re-sent entry would otherwise be applied
  // twice, and one expense sent twice doubles what everybody owes. An author
  // with a key is spoken for only by a copy that verifies against it.
  final groups = <String, List<Map<String, dynamic>>>{};
  for (final e in copies) {
    groups.putIfAbsent(e['id'] as String, () => []).add(e);
  }
  final chosen = <Map<String, dynamic>>[];
  for (final MapEntry(key: id, value: group) in groups.entries) {
    final first = group.first;
    final String? key = verify == null
        ? null
        : first['kind'] == 'createBill'
            ? first['creatorKey'] as String?
            : identities.bound[first['author']];
    var candidates = group;
    if (key != null) {
      candidates = [
        for (final e in group)
          if (verify!(e, key)) e
      ];
      if (candidates.isEmpty) {
        refusedAtIngress.add(SetAside(id, SplitCode.unauthorizedEntry));
        continue;
      }
    }
    chosen.add(_highest(candidates));
  }
  final entries = orderEntries(chosen);

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

  void aside(Map<String, dynamic> e, String code) =>
      setAside.add(SetAside(e['id'] as String, code));

  // Amendments: authored by the author of their target, carrying a payload of
  // the target's kind. An amendment replaces its target wholesale, so one
  // carrying no payload silently deletes what it claims to correct.
  for (final e in entries) {
    if (e['kind'] != 'amendEntry') continue;
    final target = byId[e['targetId']];
    if (target == null) {
      aside(e, SplitCode.unknownEntry);
      continue;
    }
    if (e['author'] != target['author']) {
      aside(e, SplitCode.unauthorizedEntry);
      continue;
    }
    final wanted = payloadForKind[target['kind']];
    if (wanted != null && !e.containsKey(wanted)) {
      aside(e, SplitCode.amendKindMismatch);
      continue;
    }
    // §10.4. The id the target is about stays: renaming it makes a different
    // entry the §10.8 checks never read.
    final subject = _amendedSubject[target['kind']];
    if (subject != null &&
        (e[subject.$1] as Map)[subject.$2] !=
            (target[subject.$1] as Map)[subject.$2]) {
      aside(e, SplitCode.amendKindMismatch);
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
      aside(e, SplitCode.unknownEntry);
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
      case 'setRate':
        // A rate prices every request on the bill, and the latest by `at`
        // decides, so one dated far ahead outranks every later correction its
        // author does not withdraw.
        allowed = {target['author'] as String, creator};
      default:
        allowed = {target['author'] as String};
    }
    if (!allowed.contains(e['author'])) {
      aside(e, SplitCode.unauthorizedEntry);
      authorised[e['id'] as String] = false;
      continue;
    }
    authorised[e['id'] as String] = true;
  }

  // A withdrawal is in force unless an authorised withdrawal naming it is
  // itself in force. `at` plays no part: an id is the digest of its entry, so
  // a withdrawal can only name one that existed when it was written, and the
  // chains are acyclic. Resolved by what names what, from the entries nothing
  // names inwards.
  final naming = <String, List<Map<String, dynamic>>>{};
  for (final e in voids) {
    naming.putIfAbsent(e['targetId'] as String, () => []).add(e);
  }
  final inForce = <String, bool>{};
  for (final start in voids) {
    final stack = <(Map<String, dynamic>, bool)>[(start, false)];
    while (stack.isNotEmpty) {
      final (current, expanded) = stack.removeLast();
      final id = current['id'] as String;
      if (inForce.containsKey(id)) continue;
      if (!(authorised[id] ?? false)) {
        inForce[id] = false;
        continue;
      }
      final namers = naming[id] ?? const [];
      final pending = [
        for (final w in namers)
          if (!inForce.containsKey(w['id'])) w
      ];
      if (pending.isNotEmpty && !expanded) {
        stack.add((current, true));
        for (final w in pending) {
          stack.add((w, false));
        }
        continue;
      }
      inForce[id] = !namers.any((w) => inForce[w['id']]!);
    }
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
  // Every removal is decided, and each refused one reported: two removals of
  // one participant are two acts, and deciding them one at a time would let
  // the first refusal hide the second.
  final removals = [
    for (final e in entries)
      if (e['kind'] == 'voidEntry' &&
          voided.contains(e['targetId']) &&
          byId[e['targetId']]!['kind'] == 'joinBill')
        e
  ];
  for (final e in removals) {
    final target = byId[e['targetId']]!;
    final gone = _mapOf(target['participant'])['id'];
    var named = false;
    for (final other in entries) {
      if (voided.contains(other['id']) || other['kind'] == 'voidEntry') {
        continue;
      }
      // The amendment and the entry it corrects are both read: the amendment
      // may yet be set aside when it is applied, and the entry then applies
      // as written.
      for (final eff in [effective(other), other]) {
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
        } else if (other['kind'] == 'confirmPayment' &&
            other['author'] == gone) {
          named = true;
        }
      }
      if (named) break;
    }
    if (named) {
      // The fold cannot apply an entry naming somebody who is not on the bill,
      // so without this, removing the person who spent the most silently drops
      // every expense they paid for.
      voided.remove(e['targetId']);
      aside(e, SplitCode.participantStillNamed);
    }
  }

  final live = [
    for (final e in entries)
      if (!voided.contains(e['id']) && e['kind'] != 'voidEntry') e,
  ];

  /// §10.4. [attempt] applied to [e] as amended, then as written. An
  /// amendment that cannot be applied is set aside and the entry it corrects
  /// applies as written: correcting an entry into one that cannot be applied
  /// does not take the entry off the bill.
  ///
  /// Returns the result and the version applied, or null when neither
  /// applies.
  (T, Map<String, dynamic>)? applied<T>(
    Map<String, dynamic> e,
    T Function(Map<String, dynamic> version) attempt,
  ) {
    final amendment = amendments[e['id']];
    if (amendment != null) {
      try {
        return (attempt(amendment), amendment);
      } on SplitError catch (err) {
        aside(amendment, err.code);
      }
    }
    try {
      return (attempt(e), e);
    } on SplitError catch (err) {
      aside(e, err.code);
      return null;
    }
  }

  // Participants in a pass of their own, before anything that references them.
  final participants = <String, Map<String, dynamic>>{};
  final replaced = <ReplacedAddress>[];
  for (final e in live) {
    if (e['kind'] != 'joinBill') continue;
    final result = applied(e, (version) {
      final p = _mapOf(version['participant']);
      final id = p['id'];
      // §9.1. An empty id is not a name anyone can be settled to: two readers
      // disagreeing about it fold different bills from one log.
      if (id is! String || id.isEmpty) {
        raise(SplitCode.billMissingEntryPayload, 'A join names no participant');
      }
      if (identities.bound.containsKey(id) && e['author'] != id) {
        // §10.7. A bound participant's record is theirs to create as well as
        // to change, so a payout cannot be redirected to somebody who never
        // joined by an entry of their own.
        raise(SplitCode.unauthorizedEntry, 'Writes a bound participant');
      }
      if (participants.containsKey(id) && e['author'] != id) {
        // Without this, one join naming another participant's id and carrying
        // your own address redirects every later settlement to that person.
        raise(SplitCode.unauthorizedEntry, 'Changes a record it does not own');
      }
      // The decoder decides what a participant is, here rather than once the
      // document is assembled: a member it would refuse sets this entry aside
      // (§10.3) instead of making the whole bill undecodable.
      final decoded = decodeParticipant(p);
      // §10.7. A record stating a key names the participant that key derives.
      // Any other id would let a second key speak for somebody a first one
      // already binds.
      final key = decoded.identityKey;
      if (key != null && id != creator && participantId(key) != id) {
        raise(SplitCode.participantIdNotDerived,
            'A key speaks for the id it derives, not $id');
      }
      return p;
    });
    if (result == null) continue;
    final (p, _) = result;
    final id = p['id'] as String;
    // §10.3 step 4. The destination this record replaces: the one held for
    // the participant, or, for the first record, the one the join was written
    // with before an amendment changed it.
    final before = participants.containsKey(id)
        ? _destination(participants[id]!)
        : _destination(_mapOf(e['participant']));
    final after = _destination(p);
    if (before != after) {
      replaced.add(ReplacedAddress(id, before, after));
    }
    participants[id] = p;
  }

  // §10.1. The latest live setRate by a participant decides, by §10.2's
  // order, so the answer is a function of the log and not of which device last
  // spoke. Decided after the participants, because only they may set it.
  Map<String, dynamic>? rate;
  String? rateEntry;
  String? rateAuthor;
  for (final e in live) {
    if (e['kind'] != 'setRate') continue;
    final result = applied(e, (version) {
      if (!participants.containsKey(e['author'])) {
        raise(
            SplitCode.unknownParticipant, 'Sets a rate on a bill it is not on');
      }
      // §10.7. A rate prices every request on the bill, so a fold that
      // verifies takes it only from a participant whose key it has bound: an
      // unsigned join is enough to put anybody holding the invite on the bill.
      if (verify != null && !identities.bound.containsKey(e['author'])) {
        raise(SplitCode.unauthorizedEntry, 'Sets a rate with no bound key');
      }
      final payload = version['rate'];
      decodeRate(payload);
      return (payload as Map).cast<String, dynamic>();
    });
    if (result == null) continue;
    // §10.1. The creator's latest rate stands over anybody else's, which
    // decides only while the creator has set none: the latest by `at` is
    // whoever dates furthest ahead, and the creator is the one participant
    // every reader can verify (§10.7).
    if (rateAuthor == creator && e['author'] != creator) continue;
    rate = result.$1;
    rateEntry = e['id'] as String;
    rateAuthor = e['author'] as String;
  }

  final expenses = <Map<String, dynamic>>[];
  final payments = <Map<String, dynamic>>[];
  final paymentAuthors = <String, String>{};
  final paymentDigests = <String, String>{};
  // The entry that introduced each applied expense and payment, and who wrote
  // it: what an amendment or a withdrawal targets, and whose entry it is.
  final expenseEntries = <String, String>{};
  final expenseAuthors = <String, String>{};
  final paymentEntries = <String, String>{};
  // §5.1's balances, formed as this pass applies each entry and in the order
  // §5.1 forms them, so a bill this fold returns always has balances §2.2 can
  // hold. An entry whose effect would carry one out of range is set aside,
  // deterministically and in log order, rather than left to make §5 refuse
  // the whole bill.
  var running = {for (final id in participants.keys) id: 0};
  // Keyed by the pair itself: ids may hold any character, so no separator
  // joins two of them into one string without collisions.
  // What each author has recorded one participant paying another. §14.4 sums
  // a payer's own records, so the bound is per author: a record the other
  // party wrote cannot carry the payer's out of range.
  final pairTotal = <(String, String, String), int>{};
  // §10.3 step 5: the ids some entry's own author minted (see [ownsId]).
  final ownedExpenseIds = <String>{};
  final ownedPaymentIds = <String>{};
  for (final e in live) {
    final member = switch (e['kind']) {
      'addExpense' => 'expense',
      'recordPayment' => 'payment',
      _ => null,
    };
    if (member == null) continue;
    final owned = member == 'expense' ? ownedExpenseIds : ownedPaymentIds;
    for (final version in [
      e,
      if (amendments[e['id']] != null) amendments[e['id']]!
    ]) {
      final id = _mapOf(version[member])['id'];
      if (id is String && ownsId(e['author'] as String, id)) owned.add(id);
    }
  }
  for (final e in live) {
    if (e['kind'] == 'addExpense') {
      final result = applied(e, (version) {
        final ex = {..._mapOf(version['expense'])};
        // An amount that states no currency is denominated by the fold. One
        // that states another is set aside, never restamped: that would keep
        // the count and change the unit. §9.1 falls back only when the member
        // is absent, so a present value that is not a currency is an entry
        // that cannot be applied, and §10.3 sets those aside rather than
        // raising.
        if (!ex.containsKey('currency')) {
          ex['currency'] = billCurrency;
        } else if (!isCurrency(ex['currency'])) {
          raise(SplitCode.billBadCurrency, 'States no currency');
        } else if (ex['currency'] != billCurrency) {
          raise(SplitCode.currencyMismatch, 'States another currency');
        }
        if (!participants.containsKey(ex['paidBy'])) {
          raise(
              SplitCode.unknownParticipant, 'Paid by somebody not on the bill');
        }
        final decoded =
            decodeExpense(ex, billCurrency, participants.keys.toSet());
        // One id names one expense. An amendment or a withdrawal is written
        // against the expense a reader shows, and two under one id leave it to
        // guess which. An id its author minted is theirs whatever the order;
        // otherwise the first stands.
        if (ownedExpenseIds.contains(decoded.id) &&
            !ownsId(e['author'] as String, decoded.id)) {
          raise(SplitCode.duplicateExpense,
              'Copies ${decoded.id}, which its author minted');
        }
        if (expenseEntries.containsKey(decoded.id)) {
          raise(SplitCode.duplicateExpense, 'Two expenses share ${decoded.id}');
        }
        // §4 is what turns an expense into what each person owes, and §5 runs
        // it downstream of this fold. An expense whose split §4 refuses cannot
        // be applied, so it is set aside here rather than raising out of
        // `netBalances` once the bill is already built.
        final shares = splitExpense(decoded.amount, decoded.split);
        final moved = {...running};
        moved[decoded.paidBy] =
            checkedBalance(checkedAdd(moved[decoded.paidBy]!, decoded.amount));
        shares.forEach((id, owed) {
          moved[id] = checkedBalance(checkedSubtract(moved[id]!, owed));
        });
        return (ex, moved);
      });
      if (result == null) continue;
      final ((ex, moved), _) = result;
      running = moved;
      expenses.add(ex);
      expenseEntries[ex['id'] as String] = e['id'] as String;
      expenseAuthors[ex['id'] as String] = e['author'] as String;
    } else if (e['kind'] == 'recordPayment') {
      final result = applied(e, (version) {
        final pay = {..._mapOf(version['payment'])};
        // A payment moves both parties' balances, so without this any holder
        // of the invite could clear a debt neither of them had settled.
        if (e['author'] != pay['from'] && e['author'] != pay['to']) {
          raise(SplitCode.unauthorizedPayment, 'Written by neither party');
        }
        if (!participants.containsKey(pay['from']) ||
            !participants.containsKey(pay['to'])) {
          raise(SplitCode.unknownParticipant, 'Names somebody not on the bill');
        }
        if (pay['from'] == pay['to']) {
          raise(SplitCode.selfPayment, 'Pays its own author');
        }
        if (!pay.containsKey('currency')) {
          pay['currency'] = billCurrency;
        } else if (!isCurrency(pay['currency'])) {
          raise(SplitCode.billBadCurrency, 'States no currency');
        } else if (pay['currency'] != billCurrency) {
          raise(SplitCode.currencyMismatch, 'States another currency');
        }
        decodePayment(pay, billCurrency, participants.keys.toSet());
        // §10.5: a confirmation names one record, and a method that speaks for
        // the payment's `to` is checked against that record's `to`. Two
        // records under one id name a payee ambiguously, so one recipient's
        // confirmation would settle a debt another never vouched for. A record
        // under an id its author minted stands; otherwise the first does. One
        // transaction paying several people carries the transaction in
        // `reference`, not in the id.
        if (ownedPaymentIds.contains(pay['id']) &&
            !ownsId(e['author'] as String, pay['id'] as String)) {
          raise(SplitCode.duplicatePayment,
              'Copies ${pay['id']}, which its author minted');
        }
        if (payments.any((p) => p['id'] == pay['id'])) {
          raise(SplitCode.duplicatePayment, 'Two payments share an id');
        }
        // What one participant has recorded paying another, confirmed or not,
        // stays in range: §14.4 sums the unconfirmed part of it.
        final pair =
            (pay['from'] as String, pay['to'] as String, e['author'] as String);
        final total = checkedAdd(pairTotal[pair] ?? 0, pay['amount'] as int);
        return (pay, pair, total);
      });
      if (result == null) continue;
      final ((pay, pair, total), version) = result;
      pairTotal[pair] = total;
      payments.add(pay);
      paymentAuthors[pay['id'] as String] = e['author'] as String;
      paymentEntries[pay['id'] as String] = e['id'] as String;
      paymentDigests[pay['id'] as String] =
          paymentDigest(_mapOf(version['payment']));
    }
  }

  // Confirmations in a pass of their own, once every payment is on the bill: a
  // confirmation may arrive before the payment it vouches for, and a single
  // pass would set aside one that is merely early.
  final known = {for (final p in payments) p['id'] as String};
  final confirmed = <String>{};
  final confirmedBy = <String, List<Map<String, dynamic>>>{};
  for (final e in live) {
    if (e['kind'] != 'confirmPayment') continue;
    final result = applied(e, (version) {
      final c = _mapOf(version['confirmation']);
      final rule = confirmationMethods[c['method']];
      if (rule == null) {
        raise(SplitCode.billUnknownConfirmationMethod, 'No such method');
      }
      if (!known.contains(c['paymentId'])) {
        raise(SplitCode.unknownPayment, 'Vouches for no payment on the bill');
      }
      // §10.5. A confirmation binds what the record said when it was given.
      if (c['record'] != paymentDigests[c['paymentId']]) {
        raise(SplitCode.unknownPayment, 'Vouches for a record since changed');
      }
      if (!participants.containsKey(e['author'])) {
        raise(SplitCode.unknownParticipant, 'Written by somebody not on it');
      }
      final pay = payments.firstWhere((p) => p['id'] == c['paymentId']);
      if (rule.speaksFor != null && e['author'] != pay[rule.speaksFor]) {
        // A confirmation's whole weight is in who gave it, so a method anyone
        // may claim is a method that says nothing.
        raise(SplitCode.unauthorizedConfirmation, 'Speaks for somebody else');
      }
      final reference = c['reference'];
      if (rule.needsReference &&
          !(reference is String && reference.isNotEmpty)) {
        // One that says a payment is on a chain without saying where contains
        // no chain. A number or a list is not a transaction id either, and
        // reading "present" three different ways settles a debt on one device
        // and leaves it open on another.
        raise(SplitCode.confirmationMissingReference, 'Names no transaction');
      }
      return (c['paymentId'] as String, rule.settles);
    });
    if (result == null) continue;
    final ((paid, settles), version) = result;
    if (settles) {
      confirmed.add(paid);
      confirmedBy.putIfAbsent(paid, () => []).add(version);
    }
  }

  // Confirmed payments move balances in the order the bill lists them (§5.1).
  // One that would carry a balance out of range stays unconfirmed, and every
  // confirmation that settled it is set aside.
  for (final pay in payments) {
    if (!confirmed.contains(pay['id'])) continue;
    final amount = pay['amount'] as int;
    try {
      final from = checkedBalance(checkedAdd(running[pay['from']]!, amount));
      final to = checkedBalance(checkedSubtract(running[pay['to']]!, amount));
      running[pay['from'] as String] = from;
      running[pay['to'] as String] = to;
    } on SplitError catch (err) {
      confirmed.remove(pay['id']);
      for (final e in confirmedBy[pay['id']]!) {
        aside(e, err.code);
      }
    }
  }

  setAside.sort((a, b) {
    final byId = compareUtf8(a.id, b.id);
    return byId != 0 ? byId : compareUtf8(a.code, b.code);
  });

  Map<String, String> sorted(Map<String, String> m) =>
      {for (final id in sortedUtf8(m.keys)) id: m[id]!};

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
    paymentAuthors: sorted(paymentAuthors),
    paymentDigests: sorted(paymentDigests),
    expenseEntries: sorted(expenseEntries),
    expenseAuthors: sorted(expenseAuthors),
    paymentEntries: sorted(paymentEntries),
    rateEntry: rateEntry,
    rateAuthor: rateAuthor,
  );
}
