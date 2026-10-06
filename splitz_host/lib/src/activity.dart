/// The log, read as a history a person can follow.
///
/// A folded bill says what is true now; it does not say what happened. The log
/// does, and it is the only place three things are visible at all: an entry
/// somebody withdrew, an entry the fold refused and why, and a payment that
/// has been claimed but not confirmed.
///
/// Deriving this is protocol work rather than wallet work — it reads entries
/// and a folded bill and nothing else — so it lives here and every wallet gets
/// the same history.
library;

import 'package:splitz_core/host.dart' as host;
import 'package:splitz_core/splitz_core.dart' as splitz;

/// What one entry did.
enum BillEventKind {
  opened,
  joined,
  addressChanged,
  expenseAdded,
  expenseAmended,
  entryWithdrawn,
  paymentRecorded,
  paymentConfirmed,
  priced,

  /// The creator closed the bill for settling (§10.9). A withdrawal of it,
  /// which reopens the bill, is an [entryWithdrawn] naming it.
  closedForSettling,

  /// An entry kind this reader does not name. Shown rather than hidden: an
  /// entry that vanished silently is indistinguishable from one that was
  /// never sent.
  other,
}

/// One line of a bill's history.
class BillEvent {
  const BillEvent({
    required this.entryId,
    required this.kind,
    required this.author,
    required this.at,
    this.subject,
    this.amountMinorUnits,
    this.description,
    this.method,
    this.reference,
    this.withdrawn = false,
    this.refusedCode,
    this.confirmed = false,
    this.takenOff,
    this.movedTo,
  });

  final String entryId;
  final BillEventKind kind;

  /// Who wrote the entry. §10.7 binds this to a key; it is not a display name.
  final String author;

  /// §9.3's canonical instant. Fixed width, so lexicographic order is
  /// chronological order and a screen sorts on the string.
  final String at;

  /// Who or what the entry is about, where that differs from [author] — the
  /// payee of a payment, the participant a vouch names, the payment a
  /// confirmation confirms, and the entry an amendment or a withdrawal
  /// targets, by its entry id.
  final String? subject;

  /// Minor units of the bill's currency (§2.1).
  final int? amountMinorUnits;

  final String? description;

  /// `shieldedZec`, `swap` or `cash` for a payment (§9.2).
  final String? method;

  /// What a payment or a confirmation names as its evidence (§9.2): the
  /// Zcash transaction id for `shieldedZec`, and the provider's own
  /// identifier for `swap`, which is **not a Zcash txid**. Read with
  /// [method]; a screen that renders one as the other is wrong for every
  /// payment of the other kind.
  final String? reference;

  /// Whether §10.8 withdrew this entry. It stays in the history: removing it
  /// would leave a reader unable to see that it was ever written.
  final bool withdrawn;

  /// Set when the fold would not apply this entry (§10.3).
  ///
  /// The §12 code, which is what a wallet turns into a sentence for its user
  /// (§1). This library writes no such sentence: one written here would be
  /// English only, and no wallet should render it.
  final String? refusedCode;

  /// For [BillEventKind.paymentRecorded]: whether §10.5 has settled it.
  ///
  /// **A recorded payment is a claim.** Presenting an unconfirmed one as
  /// settled tells a payer a debt is discharged that the payee has never
  /// agreed was paid.
  final bool confirmed;

  /// For a restatement (§10.8) — the creator rewriting somebody else's
  /// expense while taking a person off — the participant it takes off.
  final String? takenOff;

  /// For a restatement, the one participant who takes over [takenOff]'s part
  /// — paying in their place, or holding their share — when exactly one
  /// does: a merge (§14.11). Null when the part is spread over the rest.
  final String? movedTo;

  /// Whether this entry took effect at all.
  bool get applied => !withdrawn && refusedCode == null;
}

/// Reads [entries] as a history, newest first.
///
/// [folded] supplies what the log alone cannot: which entries were withdrawn,
/// which the fold set aside, and which payments are confirmed.
List<BillEvent> activityOf(
  List<Map<String, dynamic>> entries,
  splitz.Bill bill, {
  List<splitz.SetAside> setAside = const [],
  List<String> withdrawn = const [],
}) {
  final refusals = <String, splitz.SetAside>{for (final s in setAside) s.id: s};
  final gone = withdrawn.toSet();
  final confirmed = bill.confirmedPayments;

  // A join written again is how somebody amends their own record, and the one
  // amendment that moves money is a new address (§13). Telling the two apart
  // needs the order the entries arrived in, so it is decided here rather than
  // inside `_event`.
  final joined = <String>{};
  final events = <BillEvent>[];
  // One line per entry, not per copy: §10.2's union keeps copies of an id
  // under different signatures, and §9.5 makes them agree in every member a
  // line shows. A copy read twice doubles an expense and turns a join into a
  // changed address.
  final seen = <Object?>{};
  final byId = <Object?, Map<String, dynamic>>{
    for (final e in entries) e['id']: e,
  };
  for (final entry in entries) {
    if (!seen.add(entry['id'])) continue;
    var rejoined = false;
    if (entry['kind'] == 'joinBill') {
      final participant = entry['participant'];
      final id = participant is Map<String, dynamic>
          ? _text(participant['id'])
          : null;
      final carriesAddress =
          participant is Map<String, dynamic> && participant['payTo'] != null;
      rejoined = id != null && carriesAddress && !joined.add(id);
    }
    events.add(
      _event(entry, refusals, gone, confirmed, byId, rejoined: rejoined),
    );
  }
  // Newest first, and no second ordering rule: §10.2 already fixes the order
  // of a log, `entries` arrives in it, and every device agrees on it. Sorting
  // again here — on the instant, with some tie-break of this file's own —
  // would be a second answer to a question the protocol has settled, and two
  // devices could disagree about a history they hold identically.
  return events.reversed.toList();
}

BillEvent _event(
  Map<String, dynamic> entry,
  Map<String, splitz.SetAside> refusals,
  Set<String> withdrawn,
  Set<String> confirmed,
  Map<Object?, Map<String, dynamic>> byId, {
  bool rejoined = false,
}) {
  final id = _text(entry['id']) ?? '';
  final refusal = refusals[id];
  final base = (
    entryId: id,
    author: _text(entry['author']) ?? '',
    at: _text(entry['at']) ?? '',
    withdrawn: withdrawn.contains(id),
    refusedCode: refusal?.code,
  );

  BillEvent make(
    BillEventKind kind, {
    String? subject,
    int? amount,
    String? description,
    String? method,
    String? reference,
    bool isConfirmed = false,
    String? takenOff,
    String? movedTo,
  }) => BillEvent(
    entryId: base.entryId,
    kind: kind,
    author: base.author,
    at: base.at,
    subject: subject,
    amountMinorUnits: amount,
    description: description,
    method: method,
    reference: reference,
    withdrawn: base.withdrawn,
    refusedCode: base.refusedCode,
    confirmed: isConfirmed,
    takenOff: takenOff,
    movedTo: movedTo,
  );

  Map<String, dynamic>? object(String key) {
    final raw = entry[key];
    return raw is Map<String, dynamic> ? raw : null;
  }

  switch (entry['kind']) {
    case 'createBill':
      return make(
        BillEventKind.opened,
        description: _text(object('bill')?['name']),
      );

    case 'joinBill':
      final participant = object('participant');
      // §13 requires a payer be shown a changed address before settling to
      // it, so it is its own event rather than a second "joined".
      return make(
        rejoined ? BillEventKind.addressChanged : BillEventKind.joined,
        subject: _text(participant?['id']),
        description: _text(participant?['name']),
      );

    case 'addExpense':
      final expense = object('expense');
      // A restatement names the entry it replaces (§10.8): a correction of
      // that expense, not a second one.
      final restates = _text(entry['targetId']);
      if (restates != null) {
        final before = byId[restates]?['expense'];
        final moved = before is Map<String, dynamic> && expense != null
            ? _moved(before, expense)
            : null;
        return make(
          BillEventKind.expenseAmended,
          subject: restates,
          amount: _whole(expense?['amount']),
          description: _text(expense?['description']),
          takenOff: moved?.$1,
          movedTo: moved?.$2,
        );
      }
      return make(
        BillEventKind.expenseAdded,
        subject: _text(expense?['paidBy']),
        amount: _whole(expense?['amount']),
        description: _text(expense?['description']),
      );

    // Both name the entry they act on by its id, at the top level (§10.4,
    // §10.8): a reader without it can say something was changed or
    // withdrawn, and not what.
    case 'amendEntry':
      final expense = object('expense');
      return make(
        BillEventKind.expenseAmended,
        subject: _text(entry['targetId']),
        amount: _whole(expense?['amount']),
        description: _text(expense?['description']),
      );

    case 'voidEntry':
      return make(
        BillEventKind.entryWithdrawn,
        subject: _text(entry['targetId']),
      );

    case 'recordPayment':
      final payment = object('payment');
      final paymentId = _text(payment?['id']);
      return make(
        BillEventKind.paymentRecorded,
        subject: _text(payment?['to']),
        amount: _whole(payment?['amount']),
        method: _text(payment?['method']),
        reference: _text(payment?['reference']),
        isConfirmed: paymentId != null && confirmed.contains(paymentId),
      );

    case 'confirmPayment':
      final confirmation = object('confirmation');
      return make(
        BillEventKind.paymentConfirmed,
        subject: _text(confirmation?['paymentId']),
        method: _text(confirmation?['method']),
        reference: _text(confirmation?['reference']),
      );

    case 'setRate':
      final rate = object('rate');
      return make(
        BillEventKind.priced,
        amount: _whole(rate?['minorUnitsPerZec']),
        description: _text(rate?['source']),
      );

    case 'closeBill':
      return make(BillEventKind.closedForSettling);

    default:
      return make(BillEventKind.other);
  }
}

/// The payments this device may confirm, newest first (§10.5).
///
/// **Only the payee confirms.** A payer who could confirm their own payment
/// would settle a debt by asserting twice that they paid it, which is the one
/// thing a confirmation exists to prevent.
List<splitz.PaymentRecord> awaitingConfirmationBy(
  splitz.Bill bill,
  String me,
) => [
  for (final payment in bill.payments)
    if (payment.to == me && !bill.confirmedPayments.contains(payment.id))
      payment,
]..sort((a, b) => b.at.compareTo(a.at));

/// Who this device writes a confirmation of [payment] as, or null when it
/// may not confirm it (§10.5, §14.11).
///
/// The payee confirms their own payment. The creator also confirms one paid
/// to somebody they added by hand — a participant who states no key and has
/// bound none (§10.7) — written as that person and unsigned, as their join
/// was. Nobody else on the bill can, so without this a payment to somebody
/// who never joins from a device of their own, or who joined under a key of
/// their own and so under another id, never settles.
///
/// Never the payer: a creator who paid somebody they added would otherwise
/// settle the debt by asserting twice that they paid it.
String? confirmerFor(
  host.FoldedBill folded,
  splitz.PaymentRecord payment,
  String me,
) {
  if (payment.to == me) return me;
  if (me != folded.creatorId || payment.from == me) return null;
  final payee = folded.bill.participant(payment.to);
  if (payee == null ||
      payee.identityKey != null ||
      folded.identities.bound.containsKey(payment.to)) {
    return null;
  }
  return payment.to;
}

/// The payments this device may confirm, newest first: those [confirmerFor]
/// names a confirmer for, not yet confirmed.
List<splitz.PaymentRecord> awaitingConfirmationFor(
  host.FoldedBill folded,
  String me,
) => [
  for (final payment in folded.bill.payments)
    if (!folded.bill.confirmedPayments.contains(payment.id) &&
        confirmerFor(folded, payment, me) != null)
      payment,
]..sort((a, b) => b.at.compareTo(a.at));

/// [value] when it is a string. An entry's members are whatever its author
/// wrote once ingress has checked the ids (§10.1), and the history reads what
/// it can rather than failing the bill for a member the fold set aside.
String? _text(Object? value) => value is String ? value : null;

/// [value] when it is an integer; see [_text].
int? _whole(Object? value) => value is int ? value : null;

/// Who a restatement takes off [before], and the one participant who takes
/// over their part, or null for either when the two do not say.
///
/// Taken off: the one id [before] names and [after] does not. Taken over by:
/// the one other id whose part differs — paying in their place, named where
/// they were, or holding a larger figure.
(String?, String?)? _moved(
  Map<String, dynamic> before,
  Map<String, dynamic> after,
) {
  final was = _parts(before);
  final now = _parts(after);
  final gone = [
    for (final id in was.keys)
      if (!now.containsKey(id)) id,
  ];
  if (gone.length != 1) return null;
  final changed = [
    for (final id in now.keys)
      if (id != gone.single && was[id] != now[id]) id,
  ];
  return (gone.single, changed.length == 1 ? changed.single : null);
}

/// Each id [expense] names, with what it names them for: paying, and their
/// place in every list and figure of the split.
Map<String, String> _parts(Map<String, dynamic> expense) {
  final parts = <String, List<String>>{};
  void add(Object? id, String part) {
    if (id is String) (parts[id] ??= []).add(part);
  }

  add(expense['paidBy'], 'paidBy');
  final split = expense['split'];
  if (split is Map) {
    final among = split['among'];
    if (among is List) {
      for (final id in among) {
        add(id, 'among');
      }
    }
    for (final member in const ['amounts', 'basisPoints', 'shareCounts']) {
      final figures = split[member];
      if (figures is Map) {
        for (final e in figures.entries) {
          add(e.key, '$member=${e.value}');
        }
      }
    }
    final items = split['items'];
    if (items is List) {
      for (var i = 0; i < items.length; i++) {
        final sharedBy = items[i] is Map ? (items[i] as Map)['sharedBy'] : null;
        if (sharedBy is List) {
          for (final id in sharedBy) {
            add(id, 'item$i');
          }
        }
      }
    }
  }
  return {for (final e in parts.entries) e.key: (e.value..sort()).join(',')};
}
