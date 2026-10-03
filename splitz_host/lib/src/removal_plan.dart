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
  });

  /// The `addExpense` entry the restated expense replaces.
  final String entryId;

  /// The expense as the plan read it. What is written again is this, under
  /// [split]: payer, amount and description come from the same reading.
  final protocol.Expense seen;

  /// Who wrote the expense being withdrawn. The expense written in its place
  /// is the restating device's, and only its author may correct an expense
  /// (§10.4).
  final String? author;

  /// [seen]'s split without the person, the others sharing what was theirs.
  final Map<String, dynamic> split;
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

  /// Whether any entry still in force names them.
  bool get namesThem => edits.isNotEmpty || blockers.isNotEmpty;

  /// Whether [other] writes exactly what this does and is held back by the
  /// same things: what a person confirmed is still what would be written.
  bool sameAs(RemovalPlan other) => _fingerprint(this) == _fingerprint(other);

  static String _fingerprint(RemovalPlan plan) => jsonEncode({
    'edits': [
      for (final e in plan.edits)
        [
          e.entryId,
          e.seen.id,
          e.seen.paidBy,
          e.seen.amount,
          e.seen.description,
          e.seen.split,
          e.split,
        ],
    ],
    'blockers': [
      for (final b in plan.blockers)
        [b.block.name, b.entryId, b.description, b.author, b.fromThem],
    ],
    'joins': plan.joins,
  });
}

/// [split] without [id] in it, the others sharing what was theirs, or null
/// when that leaves nobody or needs a choice only a person can make: exact
/// amounts and percentages must still add up, an item they alone had
/// belongs to nobody else, and shares that leave nobody a share divide
/// nothing (§4.4).
Map<String, dynamic>? splitWithout(Map<String, dynamic> split, String id) {
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
/// create. [log] is read in the order given. An entry named by
/// [splitz.FoldedBill.withdrawn] names nobody.
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
/// different signatures, and an expense restated once per copy would be on
/// the bill twice.
RemovalPlan planRemoval({
  required splitz.FoldedBill folded,
  required String creatorId,
  required List<Map<String, dynamic>> log,
  required String id,
  required String me,
}) {
  final bill = folded.bill;
  final gone = folded.withdrawn.toSet();
  final byId = {for (final e in log) e['id']: e};

  // The amendment §10.4 applies to each entry: the last in log order whose
  // author wrote its target, carrying the target's kind and subject — and
  // none when that one is withdrawn, since an earlier one does not stand in
  // for it (§10.8).
  final amended = <String, Map<String, dynamic>>{};
  for (final e in log) {
    if (e['kind'] != 'amendEntry') continue;
    final targetId = e['targetId'];
    if (targetId is! String) continue;
    final target = byId[targetId];
    if (target == null || e['author'] != target['author']) continue;
    final member = protocol.payloadForKind[target['kind']];
    if (member == null || e[member] is! Map) continue;
    if (_map(e[member])['id'] != _map(target[member])['id']) continue;
    amended[targetId] = e;
  }
  amended.removeWhere((_, e) => gone.contains(e['id']));
  List<Map<String, dynamic>> readings(Map<String, dynamic> e, String member) =>
      [_map(e[member]), _map(amended[e['id']]?[member])];

  final edits = <RemovalEdit>[];
  final blockers = <RemovalBlocker>[];
  final joins = <String>[];
  final read = <Object?>{};
  for (final entry in log) {
    final entryId = entry['id'];
    if (entryId is! String || gone.contains(entryId) || !read.add(entryId)) {
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
        if (e.paidBy == id) {
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
        // Named only by the entry it corrects: written again as it reads now.
        final split = _names(e.split, id) ? splitWithout(e.split, id) : e.split;
        if (split == null) {
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
          RemovalEdit(entryId: entryId, seen: e, author: author, split: split),
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
  return RemovalPlan(edits: edits, blockers: blockers, joins: joins);
}
