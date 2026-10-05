/// What taking somebody off a bill needs first, and what of it one device can
/// do (§10.3, §10.8).
///
/// A `voidEntry` of somebody's `joinBill` is refused with
/// `participant_still_named` while any surviving entry names them, and a
/// refused withdrawal is still written and synced. [planRemoval] answers the
/// question before anything is written: which expenses this device can take
/// them out of, and what else still names them.
library;

import 'dart:convert';

import 'package:splitz_core/host.dart' as splitz;
import 'package:splitz_core/splitz_core.dart' as protocol;

/// One expense to write again without the person, withdrawing [entryId].
class RemovalEdit {
  const RemovalEdit({
    required this.entryId,
    required this.seen,
    required this.author,
    required this.split,
    this.basis,
    this.paidBy,
  });

  /// The `addExpense` entry the restated expense replaces.
  final String entryId;

  /// The amendment applied to [entryId] when the plan read it, or null when
  /// none was. The restatement names it, and the fold sets the restatement
  /// aside when the expense has been corrected since (§10.8).
  final String? basis;

  /// The expense as the plan read it. What is written again is this, under
  /// [split]: payer, amount and description come from the same reading.
  final protocol.Expense seen;

  /// Who wrote the expense being withdrawn. The expense written in its place
  /// is the restating device's, and only its author may correct an expense
  /// (§10.4).
  final String? author;

  /// [seen]'s split without the person, the others sharing what was theirs;
  /// for a merge, with the person's part moved to whom they are merged into.
  final Map<String, dynamic> split;

  /// The payer written in place of [seen]'s, or null to keep it. Only a merge
  /// changes the payer (see [planMerge]).
  final String? paidBy;
}

/// Why an entry still names somebody once a plan's edits are written.
enum RemovalBlock {
  /// An expense naming them that the fold does not apply. Its description is
  /// the one written in the entry.
  unapplied,

  /// They paid for the expense: nobody else can be its payer.
  paidFor,

  /// The expense was written by somebody other than this device, and this
  /// device did not open the bill (§10.8). [RemovalBlocker.author] can.
  addedByAnother,

  /// Taking them out of the split needs a choice only a person can make.
  splitByHand,

  /// A payment from or to them is on the bill.
  payment,

  /// They confirmed a payment.
  confirmation,
}

/// One entry that still names somebody once a plan's edits are written.
class RemovalBlocker {
  const RemovalBlocker(
    this.block, {
    required this.entryId,
    this.description = '',
    this.author,
    this.fromThem = false,
  });

  final RemovalBlock block;

  /// The entry that names them.
  final String entryId;

  /// The expense's description, empty when it has none, and empty for a
  /// payment or a confirmation.
  final String description;

  /// Who wrote the expense, for [RemovalBlock.addedByAnother]; null when the
  /// fold names no author.
  final String? author;

  /// For [RemovalBlock.payment]: whether they are its payer. False when they
  /// are only its payee.
  final bool fromThem;
}

/// What taking somebody off a bill needs, as one device sees it.
class RemovalPlan {
  const RemovalPlan({
    required this.edits,
    required this.blockers,
    this.joins = const [],
    this.mayWithdrawJoins = true,
  });

  /// Expenses this device can take them out of.
  final List<RemovalEdit> edits;

  /// What still names them once [edits] are written.
  final List<RemovalBlocker> blockers;

  /// Every `joinBill` still stating them, in log order: the entries a
  /// `voidEntry` must withdraw, all of them, to take them off. Each change to
  /// how somebody is paid restates their record in another join, and one left
  /// standing keeps them on the bill.
  final List<String> joins;

  /// Whether the device planning may withdraw [joins]: only the bill's
  /// creator or the person themselves may (§10.8), and a withdrawal by anybody
  /// else is set aside with `unauthorized_entry`.
  final bool mayWithdrawJoins;

  /// Whether any entry still in force names them.
  bool get namesThem => edits.isNotEmpty || blockers.isNotEmpty;

  /// Whether writing [edits] and withdrawing [joins] takes them off the bill:
  /// nothing else names them, and this device may withdraw their joins. A
  /// host offers the [edits] only when this holds (§10.8): written alone they
  /// leave the person on the bill, owed what they paid and sharing in nothing
  /// else.
  bool get complete => blockers.isEmpty && mayWithdrawJoins;

  /// How much more each participant owes once [edits] are written, in the
  /// bill's minor units: positive for the others taking on a share, and
  /// minus their share for the person taken out. Every expense is split as
  /// §4 splits it before and after, so the figures sum to zero. Participants
  /// whose share does not change are left out.
  ///
  /// Throws `amount_overflow` when a running total leaves the range §2.2
  /// allows.
  Map<String, int> get shareChanges {
    final change = <String, int>{};
    for (final e in edits) {
      final before = protocol.splitExpense(e.seen.amount, e.seen.split);
      final after = protocol.splitExpense(e.seen.amount, e.split);
      for (final id in {...before.keys, ...after.keys}) {
        final delta = protocol.checkedSubtract(after[id] ?? 0, before[id] ?? 0);
        change[id] = protocol.checkedAdd(change[id] ?? 0, delta);
      }
    }
    return {
      for (final id in change.keys.toList()..sort())
        if (change[id] != 0) id: change[id]!,
    };
  }

  /// Whether [other] writes exactly what this does and is held back by the
  /// same things: what a person confirmed is still what would be written.
  bool sameAs(RemovalPlan other) => _fingerprint(this) == _fingerprint(other);

  static String _fingerprint(RemovalPlan plan) => jsonEncode({
    'edits': [
      for (final e in plan.edits)
        [
          e.entryId,
          e.basis,
          e.seen.id,
          e.seen.paidBy,
          e.seen.amount,
          e.seen.description,
          e.seen.split,
          e.split,
          e.paidBy,
        ],
    ],
    'blockers': [
      for (final b in plan.blockers)
        [b.block.name, b.entryId, b.description, b.author, b.fromThem],
    ],
    'joins': plan.joins,
    'mayWithdrawJoins': plan.mayWithdrawJoins,
  });
}

/// [split] without [id] in it, the others sharing what was theirs, or null
/// when that leaves nobody or needs a choice only a person can make: exact
/// amounts and percentages must still add up, an item they alone had
/// belongs to nobody else, and shares that leave nobody a share divide
/// nothing (§4.4).
///
/// A member the split's `type` does not read still names them under §10.8's
/// check, which reads every member whatever the type, so it loses them too.
Map<String, dynamic>? splitWithout(Map<String, dynamic> split, String id) {
  final typed = _typedWithout(split, id);
  return typed == null ? null : _withoutAnywhere(typed, id);
}

/// [split] with [from]'s part moved to [into], or null when that needs a
/// choice only a person can make.
///
/// A list (`among`, an item's `sharedBy`) names [into] in [from]'s place; one
/// already naming both is null, because one place for two names changes
/// everybody else's share. A figure (`amounts`, `basisPoints`,
/// `shareCounts`) is added to [into]'s, so every other figure, and the total,
/// stay as they are. Throws `amount_overflow` when that sum leaves §2.2's
/// range.
Map<String, dynamic>? splitMerged(
  Map<String, dynamic> split,
  String from,
  String into,
) {
  final out = {...split};
  final among = out['among'];
  if (among is List && among.contains(from)) {
    if (among.contains(into)) return null;
    out['among'] = [for (final x in among) x == from ? into : x];
  }
  for (final member in const ['amounts', 'basisPoints', 'shareCounts']) {
    final figures = out[member];
    if (figures is Map && figures.containsKey(from)) {
      final moved = Map<String, dynamic>.from(figures);
      final theirs = moved.remove(from);
      final held = moved[into];
      if (theirs is! int || (held != null && held is! int)) return null;
      moved[into] = held == null
          ? theirs
          : protocol.checkedAdd(held as int, theirs);
      out[member] = moved;
    }
  }
  final items = out['items'];
  if (items is List) {
    final merged = <Object?>[];
    for (final raw in items) {
      final sharedBy = raw is Map ? raw['sharedBy'] : null;
      if (sharedBy is List && sharedBy.contains(from)) {
        if (sharedBy.contains(into)) return null;
        merged.add({
          ...Map<String, dynamic>.from(raw as Map),
          'sharedBy': [for (final x in sharedBy) x == from ? into : x],
        });
      } else {
        merged.add(raw);
      }
    }
    out['items'] = merged;
  }
  return out;
}

/// Whether writing [merged] in place of [split] changes the share of anybody
/// but [from] and [into], or leaves [into] other than the two shares summed.
///
/// §3 gives leftover units by id and by largest remainder, so a name or a
/// figure moved from one person to another can carry a unit across a third.
/// A merge says nobody else's share changes; one that would is for a person
/// to write.
bool _movesAnyoneElse(
  int amount,
  Map<String, dynamic> split,
  Map<String, dynamic> merged,
  String from,
  String into,
) {
  final Map<String, int> before, after;
  try {
    before = protocol.splitExpense(amount, split);
    after = protocol.splitExpense(amount, merged);
  } on protocol.SplitError {
    return true;
  }
  if (after.containsKey(from)) return true;
  for (final who in {...before.keys, ...after.keys}) {
    if (who == from) continue;
    final want = who == into
        ? (before[from] ?? 0) + (before[into] ?? 0)
        : before[who] ?? 0;
    if ((after[who] ?? 0) != want) return true;
  }
  return false;
}

/// [split] with [id] taken out of every member §10.8's check reads.
Map<String, dynamic> _withoutAnywhere(Map<String, dynamic> split, String id) {
  final out = {...split};
  final among = out['among'];
  if (among is List && among.contains(id)) {
    out['among'] = [
      for (final x in among)
        if (x != id) x,
    ];
  }
  for (final member in const ['amounts', 'basisPoints', 'shareCounts']) {
    final figures = out[member];
    if (figures is Map && figures.containsKey(id)) {
      out[member] = Map<String, dynamic>.from(figures)..remove(id);
    }
  }
  final items = out['items'];
  if (items is List) {
    out['items'] = [
      for (final raw in items)
        if (raw is Map && raw['sharedBy'] is List)
          {
            ...Map<String, dynamic>.from(raw),
            'sharedBy': [
              for (final x in raw['sharedBy'] as List)
                if (x != id) x,
            ],
          }
        else
          raw,
    ];
  }
  return out;
}

Map<String, dynamic>? _typedWithout(Map<String, dynamic> split, String id) {
  List<Object?> drop(Object? ids) => [
    for (final x in ids is List ? ids : const [])
      if (x != id) x,
  ];
  switch (split['type']) {
    case 'equal':
      final among = drop(split['among']);
      return among.isEmpty ? null : {...split, 'among': among};
    case 'shares':
      final held = split['shareCounts'];
      final counts = Map<String, dynamic>.from(held is Map ? held : const {})
        ..remove(id);
      // A count that is not an integer counts for nothing; the sum wraps at
      // 64 bits, which a split the fold applied never reaches (§4.4).
      final total = counts.values.fold<int>(
        0,
        (sum, n) => sum + (n is int ? n : 0),
      );
      return total <= 0 ? null : {...split, 'shareCounts': counts};
    case 'itemized':
      final items = <Map<String, dynamic>>[];
      final held = split['items'];
      for (final raw in held is List ? held : const []) {
        final item = Map<String, dynamic>.from(raw is Map ? raw : const {});
        final sharedBy = item['sharedBy'];
        final had = sharedBy is List ? sharedBy : const <Object?>[];
        final left = drop(had);
        if (left.isEmpty && had.contains(id)) return null;
        items.add({...item, 'sharedBy': left});
      }
      return {...split, 'items': items};
    default:
      return null;
  }
}

/// Whether a decoded split names [id] under the member its `type` reads.
bool _names(Map<String, dynamic> split, String id) {
  bool listed(Object? ids) => ids is List && ids.contains(id);
  bool keyed(Object? figures) => figures is Map && figures.containsKey(id);
  return switch (split['type']) {
    'equal' => listed(split['among']),
    'exact' => keyed(split['amounts']),
    'percentage' => keyed(split['basisPoints']),
    'shares' => keyed(split['shareCounts']),
    'itemized' =>
      split['items'] is List &&
          (split['items'] as List).any(
            (item) => item is Map && listed(item['sharedBy']),
          ),
    _ => false,
  };
}

Map<String, dynamic> _map(Object? value) =>
    value is Map<String, dynamic> ? value : const {};

/// Whether an expense payload as a peer wrote it names [id], read the way
/// §10.8's check reads it: as payer, or under any member of its split,
/// whatever its `type` says.
bool _expenseNames(Map<String, dynamic> expense, String id) {
  if (expense['paidBy'] == id) return true;
  final split = _map(expense['split']);
  List<Object?> list(Object? v) => v is List ? v : const [];
  bool keyed(Object? v) => v is Map && v.containsKey(id);
  return list(split['among']).contains(id) ||
      keyed(split['amounts']) ||
      keyed(split['basisPoints']) ||
      keyed(split['shareCounts']) ||
      list(
        split['items'],
      ).any((item) => item is Map && list(item['sharedBy']).contains(id));
}

/// What taking [id] off a bill needs, as seen from [me] (§10.8).
///
/// [folded] is [log] folded, and [creatorId] the author of the bill's
/// create. Only the entries [splitz.FoldedBill.inForce] names are read — the
/// set §10.8's check reads, so an entry refused at ingress, withdrawn,
/// replaced or a restatement that does not apply names nobody — in the order
/// [log] gives them.
///
/// §10.8 counts somebody as named by every entry still in force — an
/// expense or payment the fold set aside included — and by an amended entry
/// when either the amendment or the entry it corrects names them, so both are
/// read here; reading the folded bill alone tells a person somebody edited
/// out of an expense is on nothing, and the removal is then refused.
///
/// An expense becomes an edit when the fold applies it, they did not pay for
/// it, [me] wrote it or opened the bill, and [splitWithout] can take them out
/// of it. Every other entry naming them is a [RemovalBlocker], in log order.
/// One reading per entry id: §10.2's union keeps copies of an id under
/// different signatures.
RemovalPlan planRemoval({
  required splitz.FoldedBill folded,
  required String creatorId,
  required List<Map<String, dynamic>> log,
  required String id,
  required String me,
}) => _plan(folded, creatorId, log, id, me, null);

/// What merging [from] into [into] needs, as seen from [me]: the plan that
/// takes [from] off the bill with every expense naming them written again
/// naming [into] instead — as payer, and in the split by [splitMerged].
///
/// For somebody the creator added before the person joined under their own
/// key: the two are one person, and the bill should say so. Only the creator
/// may merge ([RemovalPlan.mayWithdrawJoins] is false for anybody else), and
/// an expense [splitMerged] cannot rewrite, a payment, or a confirmation
/// naming [from] is a [RemovalBlocker], as for a removal.
///
/// Refused with `unknown_participant` when either is not on the bill or they
/// are the same, and with `unauthorized_entry` when [from] states a key or
/// §10.7 binds one to them: a person who joined themselves is never folded
/// into somebody else.
RemovalPlan planMerge({
  required splitz.FoldedBill folded,
  required String creatorId,
  required List<Map<String, dynamic>> log,
  required String from,
  required String into,
  required String me,
}) {
  final stand = folded.bill.participant(from);
  if (stand == null || folded.bill.participant(into) == null || from == into) {
    throw const protocol.SplitError(
      protocol.SplitCode.unknownParticipant,
      'Both must be on the bill, and be two people',
    );
  }
  if (stand.identityKey != null || folded.identities.bound.containsKey(from)) {
    throw const protocol.SplitError(
      protocol.SplitCode.unauthorizedEntry,
      'They joined with a key of their own, so they are somebody',
    );
  }
  return _plan(folded, creatorId, log, from, me, into);
}

RemovalPlan _plan(
  splitz.FoldedBill folded,
  String creatorId,
  List<Map<String, dynamic>> log,
  String id,
  String me,
  String? into,
) {
  final bill = folded.bill;
  final inForce = folded.inForce.toSet();
  final byId = {for (final e in log) e['id']: e};

  // The amendment §10.4 applies to each entry, as the fold chose it.
  Map<String, dynamic>? amendmentOf(Object? entryId) {
    final amendmentId = folded.amendmentOf[entryId];
    return amendmentId == null ? null : byId[amendmentId];
  }

  List<Map<String, dynamic>> readings(Map<String, dynamic> e, String member) =>
      [_map(e[member]), _map(amendmentOf(e['id'])?[member])];

  final edits = <RemovalEdit>[];
  final blockers = <RemovalBlocker>[];
  final joins = <String>[];
  final read = <Object?>{};
  for (final entry in log) {
    final entryId = entry['id'];
    if (entryId is! String ||
        !inForce.contains(entryId) ||
        !read.add(entryId)) {
      continue;
    }
    switch (entry['kind']) {
      case 'addExpense':
        if (!readings(entry, 'expense').any((x) => _expenseNames(x, id))) {
          continue;
        }
        final e = bill.expenses
            .where((x) => folded.expenseEntries[x.id] == entryId)
            .firstOrNull;
        if (e == null) {
          final written = _map(entry['expense'])['description'];
          blockers.add(
            RemovalBlocker(
              RemovalBlock.unapplied,
              entryId: entryId,
              description: written is String ? written : '',
            ),
          );
          continue;
        }
        if (e.paidBy == id && into == null) {
          blockers.add(
            RemovalBlocker(
              RemovalBlock.paidFor,
              entryId: entryId,
              description: e.description,
            ),
          );
          continue;
        }
        final author = folded.expenseAuthors[e.id];
        // §10.8: an expense's author or the bill's creator may withdraw it.
        if (author != me && me != creatorId) {
          blockers.add(
            RemovalBlocker(
              RemovalBlock.addedByAnother,
              entryId: entryId,
              description: e.description,
              author: author,
            ),
          );
          continue;
        }
        // Named only by the entry it corrects, or by a member its type does
        // not read: written again as it reads now, without them.
        final split = into != null
            ? splitMerged(e.split, id, into)
            : _names(e.split, id)
            ? splitWithout(e.split, id)
            : _withoutAnywhere(e.split, id);
        if (split == null ||
            (into != null &&
                _movesAnyoneElse(e.amount, e.split, split, id, into))) {
          blockers.add(
            RemovalBlocker(
              RemovalBlock.splitByHand,
              entryId: entryId,
              description: e.description,
            ),
          );
          continue;
        }
        edits.add(
          RemovalEdit(
            entryId: entryId,
            seen: e,
            author: author,
            split: split,
            basis: folded.amendmentOf[entryId],
            paidBy: into != null && e.paidBy == id ? into : null,
          ),
        );
      case 'recordPayment':
        final payer = readings(entry, 'payment').any((x) => x['from'] == id);
        final payee = readings(entry, 'payment').any((x) => x['to'] == id);
        if (payer || payee) {
          blockers.add(
            RemovalBlocker(
              RemovalBlock.payment,
              entryId: entryId,
              fromThem: payer,
            ),
          );
        }
      case 'confirmPayment':
        if (entry['author'] == id) {
          blockers.add(
            RemovalBlocker(RemovalBlock.confirmation, entryId: entryId),
          );
        }
      case 'joinBill':
        if (_map(entry['participant'])['id'] == id) joins.add(entryId);
    }
  }
  return RemovalPlan(
    edits: edits,
    blockers: blockers,
    joins: joins,
    mayWithdrawJoins: me == creatorId || (into == null && me == id),
  );
}

/// The entries that carry out a [plan] that is [RemovalPlan.complete], as
/// [host] writes them and before they are signed: each expense written again
/// without the person, naming the entry it replaces and the correction it read
/// (§10.8), then a withdrawal of every join stating them.
///
/// Written together, in one merge: written apart, a sync between them leaves
/// the expenses restated and the person on the bill. Refused for a plan that
/// is not complete, which this would leave half done: with
/// `unauthorized_entry` when this device may not withdraw their joins, and
/// `participant_still_named` when something else still names them.
List<Map<String, dynamic>> removalEntries({
  required splitz.BillHost host,
  required RemovalPlan plan,
}) {
  if (!plan.mayWithdrawJoins) {
    throw const protocol.SplitError(
      protocol.SplitCode.unauthorizedEntry,
      "Only the bill's creator or the person may take them off",
    );
  }
  if (plan.blockers.isNotEmpty) {
    throw const protocol.SplitError(
      protocol.SplitCode.participantStillNamed,
      'Something else on the bill still names them',
    );
  }
  return [
    for (final edit in plan.edits)
      splitz.restateExpense(
        host: host,
        targetId: edit.entryId,
        basis: edit.basis,
        expenseId: 'r-${edit.entryId}',
        paidBy: edit.paidBy ?? edit.seen.paidBy,
        amount: edit.seen.amount,
        split: edit.split,
        description: edit.seen.description.isEmpty
            ? null
            : edit.seen.description,
      ),
    for (final join in plan.joins) splitz.voidEntry(host: host, targetId: join),
  ];
}
