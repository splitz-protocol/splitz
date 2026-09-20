/// The log a device holds for one bill, and the bill it folds to.
///
/// §10.2 merges by set union keyed by entry id, so a device holds entries and
/// derives everything else. Nothing here caches a bill across a change: the
/// bill is a function of the entries, and a cached one is a second source of
/// truth that goes stale without saying so.
library;

import 'package:splitz_core/splitz_core.dart' as splitz;

import 'host.dart';

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
    required this.setAside,
    required this.withdrawn,
    required this.replacedAddresses,
    required this.identities,
  });

  final splitz.Bill bill;
  final List<splitz.SetAside> setAside;
  final List<String> withdrawn;

  /// Every pay-to address that changed, which §13 says a wallet MUST show
  /// before settling.
  final List<splitz.ReplacedAddress> replacedAddresses;

  /// Which keys §10.7 binds, and which ids two keys each claim.
  ///
  /// A contested id is not an error and its entries still apply — refusing
  /// them would let anyone make a bill unopenable by minting a rival claim.
  /// What a contest costs is the ability to be paid: §10.7 says a wallet MUST
  /// NOT settle to a contested participant's address without putting it in
  /// front of the payer first.
  ///
  /// Empty of contests when the host does not verify: without a verifier no
  /// self-claim is checked, so nothing is bound and nothing is contested.
  final splitz.Identities identities;
}

/// One bill's entries, and the answers derived from them.
class BillLog {
  BillLog(this._host, {List<Map<String, dynamic>>? entries})
      : _entries = [...?entries];

  final BillHost _host;
  final List<Map<String, dynamic>> _entries;

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
  /// Throws only when the log opens no bill at all — no entries, or none that
  /// creates one. An entry that cannot be applied is set aside and reported,
  /// never raised, so one bad entry does not take the bill with it.
  FoldedBill fold() {
    final verify = _host.verify;
    final result = verify == null
        ? splitz.foldLog(_entries)
        : splitz.foldLog(_entries, verify: verify);
    return FoldedBill(
      bill: splitz.decodeBill(result.bill),
      setAside: result.setAside,
      withdrawn: result.withdrawn,
      replacedAddresses: result.replacedAddresses,
      identities: result.identities,
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
