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
  });

  final String entryId;
  final BillEventKind kind;

  /// Who wrote the entry. §10.7 binds this to a key; it is not a display name.
  final String author;

  /// §9.3's canonical instant. Fixed width, so lexicographic order is
  /// chronological order and a screen sorts on the string.
  final String at;

  /// Who the entry is about, where that differs from [author] — the payee of
  /// a payment, the participant a vouch names.
  final String? subject;

  /// Minor units of the bill's currency (§2.1).
  final int? amountMinorUnits;

  final String? description;

  /// `shieldedZec`, `swap` or `cash` for a payment (§9.2).
  final String? method;

  /// A swap's own identifier. **Not a Zcash txid** — a screen that renders it
  /// as one is wrong for every swap (§9.2).
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
  for (final entry in entries) {
    var rejoined = false;
    if (entry['kind'] == 'joinBill') {
      final participant = entry['participant'];
      final id = participant is Map<String, dynamic>
          ? participant['id'] as String?
          : null;
      final carriesAddress =
          participant is Map<String, dynamic> && participant['payTo'] != null;
      rejoined = id != null && carriesAddress && !joined.add(id);
    }
    events.add(_event(entry, refusals, gone, confirmed, rejoined: rejoined));
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
  Set<String> confirmed, {
  bool rejoined = false,
}) {
  final id = (entry['id'] ?? '') as String;
  final refusal = refusals[id];
  final base = (
    entryId: id,
    author: (entry['author'] ?? '') as String,
    at: (entry['at'] ?? '') as String,
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
  );

  Map<String, dynamic>? object(String key) {
    final raw = entry[key];
    return raw is Map<String, dynamic> ? raw : null;
  }

  switch (entry['kind']) {
    case 'createBill':
      return make(
        BillEventKind.opened,
        description: object('bill')?['name'] as String?,
      );

    case 'joinBill':
      final participant = object('participant');
      // §13 requires a payer be shown a changed address before settling to
      // it, so it is its own event rather than a second "joined".
      return make(
        rejoined ? BillEventKind.addressChanged : BillEventKind.joined,
        subject: participant?['id'] as String?,
        description: participant?['name'] as String?,
      );

    case 'addExpense':
      final expense = object('expense');
      return make(
        BillEventKind.expenseAdded,
        subject: expense?['paidBy'] as String?,
        amount: expense?['amount'] as int?,
        description: expense?['description'] as String?,
      );

    case 'amendEntry':
      return make(BillEventKind.expenseAmended);

    case 'voidEntry':
      return make(
        BillEventKind.entryWithdrawn,
        subject: object('void')?['target'] as String?,
      );

    case 'recordPayment':
      final payment = object('payment');
      final paymentId = payment?['id'] as String?;
      return make(
        BillEventKind.paymentRecorded,
        subject: payment?['to'] as String?,
        amount: payment?['amount'] as int?,
        method: payment?['method'] as String?,
        reference: payment?['reference'] as String?,
        isConfirmed: paymentId != null && confirmed.contains(paymentId),
      );

    case 'confirmPayment':
      final confirmation = object('confirmation');
      return make(
        BillEventKind.paymentConfirmed,
        subject: confirmation?['paymentId'] as String?,
        method: confirmation?['method'] as String?,
        reference: confirmation?['reference'] as String?,
      );

    case 'setRate':
      final rate = object('rate');
      return make(
        BillEventKind.priced,
        amount: rate?['minorUnitsPerZec'] as int?,
        description: rate?['source'] as String?,
      );

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
