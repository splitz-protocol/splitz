/// The log a device holds for one bill, and the bill it folds to.
///
/// §10.2 merges by set union keyed by entry id, so a device holds entries and
/// derives everything else. Nothing here caches a bill across a change: the
/// bill is a function of the entries, and a cached one is a second source of
/// truth that goes stale without saying so.
library;

import 'package:splitz_core/splitz_core.dart' as splitz;

import 'host.dart';

/// Refusals an entry outgrows: each names a participant, entry or payment
/// this device may not hold yet, and the entry applies once a sync brings it
/// (§10.3). See [BillLog.refusalOf].
const Set<String> codesAnEntryOutgrows = {
  splitz.SplitCode.unknownParticipant,
  splitz.SplitCode.unknownEntry,
  splitz.SplitCode.unknownPayment,
};

/// What a fold produced, and what it refused.
///
/// The refusals travel with the bill because an entry that vanished silently
/// is indistinguishable from one that was never sent. §10.3 makes the report
/// per occurrence and not part of the convergent state, so two devices may
/// list different refusals for one history and still hold the same bill —
/// which is why these are not compared across devices anywhere.
class FoldedBill {
  const FoldedBill({
    required this.bill,
    required this.creatorId,
    required this.setAside,
    required this.withdrawn,
    required this.replacedAddresses,
    required this.identities,
    this.paymentAuthors = const {},
    this.paymentDigests = const {},
    this.expenseEntries = const {},
    this.expenseAuthors = const {},
    this.paymentEntries = const {},
    this.rateEntry,
    this.rateAuthor,
  });

  final splitz.Bill bill;

  /// Who opened the bill: the author of the one `createBill` the fold kept —
  /// for this bill's id and, when the host verifies, signed by the key that
  /// entry states (§10.1). §10.8 and §10.4 give this participant alone some
  /// powers, so a reader decides them from this and never from whichever
  /// create a log happens to list first.
  final String creatorId;
  final List<splitz.SetAside> setAside;
  final List<String> withdrawn;

  /// Every pay-to address that changed, which §13 says a wallet MUST show
  /// before settling.
  final List<splitz.ReplacedAddress> replacedAddresses;

  /// Which keys §10.7 binds.
  ///
  /// Empty when the host does not verify: without a verifier no self-claim
  /// is checked, so nothing is bound.
  final splitz.Identities identities;

  /// Who wrote each payment record on the bill, by the payment's id (§14.4).
  final Map<String, String> paymentAuthors;

  /// What each payment record says, by the payment's id: the digest a
  /// confirmation of it carries as `record` (§10.5).
  final Map<String, String> paymentDigests;

  /// The entry that introduced each expense, by the expense's own id: what
  /// an amendment or a withdrawal of it targets. The fold's answer, not the
  /// log's, which also holds entries the fold set aside.
  final Map<String, String> expenseEntries;

  /// Who wrote each expense, by the expense's own id.
  final Map<String, String> expenseAuthors;

  /// The entry that recorded each payment, by the payment's id.
  final Map<String, String> paymentEntries;

  /// The `setRate` entry whose rate the bill carries.
  final String? rateEntry;

  /// Who wrote that `setRate`: the name §14.2 puts beside the rate.
  final String? rateAuthor;
}

/// One bill's entries, and the answers derived from them.
class BillLog {
  /// [billId] names the bill these entries belong to. A device that holds a
  /// bill always knows it, and a fold that is not told reads whatever single
  /// create the log holds — so anyone holding the invite who pushes a valid
  /// create for another bill into the channel makes this one unopenable
  /// (§10.3's `ambiguous_create`). It is omitted only by a caller about to
  /// learn the id from the log, such as one opening a bill it just created.
  BillLog(this._host, {List<Map<String, dynamic>>? entries, this.billId})
      : _entries = [...?entries];

  final BillHost _host;
  final List<Map<String, dynamic>> _entries;

  /// The bill these entries belong to, when the caller named it.
  final String? billId;

  /// The entries this device holds, in the order §10.2 puts them.
  List<Map<String, dynamic>> get entries =>
      List.unmodifiable(splitz.orderEntries(_entries));

  /// Adds entries this device wrote, or a peer's.
  ///
  /// Returns what the merge refused at ingress (§10.1). A caller that ignores
  /// it has dropped somebody's entry without telling them.
  List<splitz.SetAside> add(Iterable<Map<String, dynamic>> incoming) {
    final merged = splitz.mergeLogs([_entries, incoming.toList()]);
    _entries
      ..clear()
      ..addAll(merged.merged);
    return merged.refused;
  }

  /// Folds to a bill (§10.3).
  ///
  /// The §12 code the fold would set [entry] aside with were it appended to
  /// this log, or null when it would apply (§10.8, "Asking before writing").
  ///
  /// Answered by folding the log with [entry] in it, so it is the fold's own
  /// rule and cannot drift from it. Pass the entry as it would be written —
  /// signed, where this host verifies. A refusal in [codesAnEntryOutgrows]
  /// waits on an entry this device may not hold yet and applies once a sync
  /// brings it; any other is written, synced and refused on every device for
  /// good, so a host writes nothing on one.
  String? refusalOf(Map<String, dynamic> entry) {
    try {
      splitz.checkEntry(entry);
    } on splitz.SplitError catch (e) {
      return e.code;
    }
    final trial = BillLog(_host, entries: [..._entries, entry], billId: billId);
    try {
      return trial
          .fold()
          .setAside
          .where((s) => s.id == entry['id'])
          .firstOrNull
          ?.code;
    } on splitz.SplitError catch (e) {
      // A log the entry leaves opening no single bill — a second create on a
      // log naming none — is refused whole.
      return e.code;
    }
  }

  /// Throws only when the log opens no bill at all — no entries, or none that
  /// creates one. An entry that cannot be applied is set aside and reported,
  /// never raised, so one bad entry does not take the bill with it.
  FoldedBill fold() {
    final verify = _host.verify;
    final result = verify == null
        ? splitz.foldLog(_entries, billId: billId)
        : splitz.foldLog(_entries, billId: billId, verify: verify);
    return FoldedBill(
      bill: splitz.decodeBill(result.bill),
      creatorId: result.creator,
      setAside: result.setAside,
      withdrawn: result.withdrawn,
      replacedAddresses: result.replacedAddresses,
      identities: result.identities,
      paymentAuthors: result.paymentAuthors,
      paymentDigests: result.paymentDigests,
      expenseEntries: result.expenseEntries,
      expenseAuthors: result.expenseAuthors,
      paymentEntries: result.paymentEntries,
      rateEntry: result.rateEntry,
      rateAuthor: result.rateAuthor,
    );
  }

  /// True when this log opens a bill. A log a peer has only half-delivered
  /// does not, and that is a state to show rather than an exception to throw.
  bool get opensABill {
    if (_entries.isEmpty) return false;
    try {
      fold();
      return true;
    } on splitz.SplitError {
      return false;
    }
  }
}
