/// Moving a bill between devices through a relay that holds only ciphertext.
library;

import 'package:splitz_core/splitz_core.dart' as protocol;

import 'keys.dart';
import 'relay.dart';
import 'sealing.dart';
import 'store.dart';

/// Syncs one bill through a relay.
///
/// Everything promised about the relay lives here: entries are sealed under the
/// bill key before they leave, the channel is the bill id's hash so the relay
/// cannot name the bill, and merging is the same order-independent set union
/// the local store already performs — so a device that has been offline merges
/// what it missed rather than reconciling two versions of a summary.
class SplitsSync {
  SplitsSync({
    required BillStore store,
    required SplitsKeys keys,
    required SplitsRelay relay,
    SplitsSealing? sealing,
  }) : _store = store,
       _keys = keys,
       _relay = relay,
       _sealing = sealing ?? SplitsSealing();

  final BillStore _store;
  final SplitsKeys _keys;
  final SplitsRelay _relay;
  final SplitsSealing _sealing;

  /// Pushes what this device holds, then pulls what it does not.
  ///
  /// Push first, so a participant syncing right after us sees our entries; then
  /// pull, so we see theirs. Both directions merge by entry id, so running this
  /// twice, or on two devices at once, converges.
  Future<SyncResult> sync(String billId) async {
    await push(billId);
    return pull(billId);
  }

  /// Seals every entry this device holds, as it holds it, and pushes it to
  /// the bill's channel.
  ///
  /// **Nothing is signed here.** An entry is signed by the device that writes
  /// it, when it writes it. The store also holds what peers pushed, and an
  /// unsigned entry a peer wrote in this device's name would otherwise be
  /// signed with this device's key on the next push — a forged confirmation
  /// becoming a genuine one. A blob is keyed by its content, so pushing the
  /// whole log every time is safe: the relay stores each entry once however
  /// often it is sent.
  Future<List<Map<String, dynamic>>> push(String billId) async {
    final entries = await _store.read(billId);
    if (entries.isEmpty) return entries;
    final key = await _requireKey(billId);
    final blobs = [
      for (final entry in entries) await _sealing.seal(entry, key),
    ];
    await _relay.push(SplitsChannel.forBill(billId), blobs);
    return entries;
  }

  /// Fetches the channel, opens what it can, and merges it into what is held.
  ///
  /// Everything that opens is merged. Authorship is **not** judged here: an
  /// entry admitted or refused by what this device happened to hold when it
  /// arrived would make the stored log depend on network order, and two devices
  /// that pulled the same entries in a different order would then hold
  /// different bills. §10.7 decides authorship over the whole log at fold time,
  /// where the answer is the same on every device and a locally written entry
  /// faces exactly the rules a synced one does.
  Future<SyncResult> pull(String billId) async {
    final key = await _requireKey(billId);
    final blobs = await _relay.fetch(SplitsChannel.forBill(billId));

    var unopenable = 0;
    final entries = <Map<String, dynamic>>[];
    for (final blob in blobs) {
      try {
        entries.add(await _sealing.open(blob, key));
      } on SealingException {
        // A foreign or altered blob is skipped rather than failing the whole
        // sync, so one bad blob cannot strand a bill.
        unopenable++;
      }
    }

    // Merged only while this device still holds the bill's key. A bill
    // forgotten while the fetch was in flight is not written back: it would
    // return with no key, and the next Share would mint a key nobody else
    // holds.
    final merged = await _store.merge(
      billId,
      entries,
      onlyIf: () async {
        final held = await _keys.readBillKey(billId);
        return held != null && held.isNotEmpty;
      },
    );
    if (!merged.applied) {
      throw SplitsSyncException('$billId was forgotten while it synced');
    }
    return SyncResult(
      entries: merged.entries,
      refused: merged.refused,
      unopenable: unopenable,
    );
  }

  Future<String> _requireKey(String billId) async {
    final String? key;
    try {
      key = await _keys.readBillKey(billId);
    } on StateError catch (e) {
      // A keychain refuses while the session is locked. A poll that lands then
      // is a sync that could not run, and is reported as one.
      throw SplitsSyncException(
        'Cannot read the key for $billId: ${e.message}',
      );
    }
    if (key == null || key.isEmpty) {
      throw SplitsSyncException('No key for $billId; it cannot be synced');
    }
    return key;
  }
}

/// What one sync produced.
class SyncResult {
  const SyncResult({
    required this.entries,
    required this.refused,
    required this.unopenable,
  });

  /// The merged log, in the order §10.2 puts it.
  final List<Map<String, dynamic>> entries;

  /// What the merge refused at ingress (§10.1).
  final List<protocol.SetAside> refused;

  /// Blobs in the channel that would not open under this bill's key.
  ///
  /// Counted rather than ignored. A channel where every blob is unopenable is
  /// a key that is wrong, and that looks identical to a quiet relay unless
  /// somebody is counting.
  final int unopenable;
}

/// Raised when a bill cannot be synced.
class SplitsSyncException implements Exception {
  const SplitsSyncException(this.message);

  final String message;

  @override
  String toString() => 'SplitsSyncException: $message';
}
