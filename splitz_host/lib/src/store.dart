/// Where a device keeps the bills it holds.
library;

import 'dart:convert';

import 'package:splitz_core/splitz_core.dart' as protocol;

/// Durable storage for this device's bills, as raw entries.
///
/// Entries, not folded bills. §10.2 merges by set union, so a device holds
/// entries and derives everything else; a stored summary is a second source of
/// truth that goes stale without saying so.
///
/// Specified in SPEC.md §15.4.
abstract interface class BillStorage {
  Future<String?> read(String key);
  Future<void> write(String key, String value);
  Future<void> delete(String key);
  Future<List<String>> keys(String prefix);

  /// Removes whatever a write that did not finish left behind, and returns
  /// how many. Zero for a store that cannot leave anything.
  ///
  /// Called once when the feature loads. A leftover is already invisible to
  /// [keys]; this stops them accumulating across the crashes of a year.
  Future<int> sweepUnfinishedWrites();
}

/// Storage that does not outlive the process. For tests.
class InMemoryBillStorage implements BillStorage {
  final Map<String, String> _values = {};

  @override
  Future<String?> read(String key) async => _values[key];

  @override
  Future<void> write(String key, String value) async => _values[key] = value;

  @override
  Future<void> delete(String key) async => _values.remove(key);

  @override
  Future<List<String>> keys(String prefix) async =>
      _values.keys.where((k) => k.startsWith(prefix)).toList()..sort();

  /// Nothing to sweep: a map cannot be half written.
  @override
  Future<int> sweepUnfinishedWrites() async => 0;
}

/// The bills this device holds, and the entries each is made of.
class BillStore {
  BillStore(this._storage);

  final BillStorage _storage;

  /// The storage underneath, for a device-local list that is not a bill.
  ///
  /// Exposed rather than wrapped because what else a device keeps beside its
  /// bills is not this class's business — it owns the `bill/` namespace and
  /// says so, and another list picks its own.
  BillStorage get storage => _storage;

  static const String _prefix = 'splitz_bill_';

  String _name(String billId) => '$_prefix$billId';

  /// Every bill id this device holds entries for.
  Future<List<String>> billIds() async {
    final keys = await _storage.keys(_prefix);
    return [for (final key in keys) key.substring(_prefix.length)];
  }

  /// The entries held for [billId], in the order §10.2 puts them, or an empty
  /// list when none are held.
  ///
  /// Stored text that will not decode is returned as no entries rather than
  /// raised: a bill this device cannot read is a state to show, and raising
  /// here would take down whatever listed the bills.
  Future<List<Map<String, dynamic>>> read(String billId) async {
    final stored = await _storage.read(_name(billId));
    if (stored == null || stored.isEmpty) return const [];
    final Object? decoded;
    try {
      decoded = jsonDecode(stored);
    } on FormatException {
      return const [];
    }
    if (decoded is! List) return const [];
    // Only what §10.1 admits. The store writes nothing else, so anything else
    // is a file damaged or written by something that is not this store — and
    // ordering it would raise on the first entry with no `at`.
    final entries = <Map<String, dynamic>>[];
    for (final e in decoded) {
      if (e is! Map<String, dynamic>) continue;
      try {
        protocol.checkEntry(e);
      } on protocol.SplitError {
        continue;
      }
      entries.add(e);
    }
    return protocol.orderEntries(entries);
  }

  /// Merges [incoming] into what is held, and returns the merged log.
  ///
  /// The merge is the protocol's, so it is the same set union a peer performs:
  /// idempotent, commutative, and deciding a collision by content rather than
  /// by which copy arrived first. Whatever it refuses at ingress is returned
  /// with the log, because an entry that vanished silently is indistinguishable
  /// from one that was never sent.
  ///
  /// Merges into one bill run one at a time, with [forget] among them: each
  /// reads the log, merges and writes it back, so two at once would each write
  /// a log missing the other's entries — a sync overwriting the record of a
  /// payment just sent. [onlyIf], when given, is checked inside that turn, and
  /// when it answers false nothing is written and [MergedBill.applied] is
  /// false: a sync that fetched before the bill was forgotten must not write
  /// it back.
  Future<MergedBill> merge(
    String billId,
    Iterable<Map<String, dynamic>> incoming, {
    Future<bool> Function()? onlyIf,
  }) => _serial(billId, () async {
    if (onlyIf != null && !await onlyIf()) {
      return const MergedBill(entries: [], refused: [], applied: false);
    }
    final held = await read(billId);
    final merged = protocol.mergeLogs([held, incoming.toList()]);
    await _storage.write(_name(billId), jsonEncode(merged.merged));
    return MergedBill(entries: merged.merged, refused: merged.refused);
  });

  /// Forgets a bill entirely, in its turn among merges into it.
  Future<void> forget(String billId) =>
      _serial(billId, () => _storage.delete(_name(billId)));

  /// The last operation queued on each bill, per storage.
  ///
  /// Shared by every store in the process and keyed by the storage object,
  /// so two stores over one storage queue behind each other. A storage over a
  /// shared medium hands out one object per medium ([FileBillStorage] does,
  /// per directory) for this to cover it.
  static final Map<Object, Map<String, Future<void>>> _tailsByScope = {};

  Future<T> _serial<T>(String billId, Future<T> Function() body) {
    final scope = _storage;
    final tails = _tailsByScope.putIfAbsent(scope, () => {});
    final run = (tails[billId] ?? Future<void>.value()).then((_) => body());
    final tail = run.then<void>((_) {}, onError: (Object _) {});
    tails[billId] = tail;
    // Dropped once nothing is queued behind it, so the map holds only bills
    // with work in flight.
    tail.then((_) {
      if (identical(tails[billId], tail)) tails.remove(billId);
      if (tails.isEmpty) _tailsByScope.remove(scope);
    });
    return run;
  }

  /// Clears anything an interrupted write left behind. See
  /// [BillStorage.sweepUnfinishedWrites].
  Future<int> sweepUnfinishedWrites() => _storage.sweepUnfinishedWrites();
}

/// A merged log, and what the merge would not take.
class MergedBill {
  const MergedBill({
    required this.entries,
    required this.refused,
    this.applied = true,
  });

  /// False when the condition given to [BillStore.merge] refused the merge,
  /// and nothing was written.
  final bool applied;

  final List<Map<String, dynamic>> entries;

  /// Refused at ingress under §10.1. A caller that ignores this has dropped
  /// somebody's entry without telling them.
  final List<protocol.SetAside> refused;
}
