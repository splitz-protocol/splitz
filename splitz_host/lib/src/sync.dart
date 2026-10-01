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

  /// §9.4. Throws when [entries] hold [billId]'s create and it commits to a
  /// key other than [key].
  ///
  /// A key handed over with a real bill's id opens whatever its maker sealed
  /// under it: a copy of the bill with an entry only this device sees, and
  /// everything this device writes then reaches nobody else. The bill's own
  /// create names the key it was made with, so a log that disagrees is not
  /// merged and a held log is not sealed under a key that is not its own.
  void _refuseForeignKey(
    String billId,
    String key,
    List<Map<String, dynamic>> entries,
  ) {
    for (final e in entries) {
      if (protocol.createRefusesKey(e, billId, key)) {
        throw SplitsSyncException(
          protocol.describeCode(protocol.SplitCode.inviteKeyMismatch)!,
          code: protocol.SplitCode.inviteKeyMismatch,
          kind: SyncFailure.foreignKey,
        );
      }
    }
  }

  /// Pulls what this device does not hold, then pushes what the channel does
  /// not.
  ///
  /// Pull first, so a push the relay refuses — a store that is full, a log
  /// that has outgrown what one request carries — never stops this device
  /// seeing what the others wrote. The push then sends only blobs the fetch
  /// did not return: sealing is deterministic, so a blob the channel holds is
  /// an entry it holds. Both directions merge by content, so running this
  /// twice, or on two devices at once, converges.
  Future<SyncResult> sync(String billId) async {
    final (result, fetched) = await _pull(billId);
    await push(billId, held: fetched);
    return result;
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
  /// often it is sent. Blobs in [held] — what the channel is known to hold —
  /// are not sent again.
  Future<List<Map<String, dynamic>>> push(
    String billId, {
    Set<String> held = const {},
  }) async {
    final entries = await _store.read(billId);
    if (entries.isEmpty) return entries;
    final key = await _requireKey(billId);
    _refuseForeignKey(billId, key, entries);
    final blobs = [
      for (final entry in entries) await _sealing.seal(entry, key),
    ];
    final fresh = [
      for (final blob in blobs)
        if (!held.contains(blob)) blob,
    ];
    if (fresh.isNotEmpty) {
      await _relay.push(SplitsChannel.forBill(billId), fresh);
    }
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
  Future<SyncResult> pull(String billId) async => (await _pull(billId)).$1;

  /// [pull], and the blobs the channel returned.
  Future<(SyncResult, Set<String>)> _pull(String billId) async {
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

    _refuseForeignKey(billId, key, entries);

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
      throw SplitsSyncException(
        '$billId was forgotten while it synced',
        kind: SyncFailure.forgotten,
      );
    }
    return (
      SyncResult(
        entries: merged.entries,
        refused: merged.refused,
        unopenable: unopenable,
      ),
      blobs.toSet(),
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
        kind: SyncFailure.keyLocked,
      );
    }
    if (key == null || key.isEmpty) {
      throw SplitsSyncException(
        'No key for $billId; it cannot be synced',
        kind: SyncFailure.noKey,
      );
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

/// Why a bill could not be synced, for a wallet to put in its own words.
enum SyncFailure {
  /// This device holds no key for the bill.
  noKey,

  /// The keychain refused to read the key, as it does while locked.
  keyLocked,

  /// The bill was forgotten on this device while the sync ran.
  forgotten,

  /// The key held is not the one the bill was made with (§9.4).
  foreignKey,
}

/// Raised when a bill cannot be synced.
class SplitsSyncException implements Exception {
  const SplitsSyncException(this.message, {this.code, required this.kind});

  /// For a developer: names the bill and the cause.
  final String message;

  /// The §12 code, when the refusal has one.
  final String? code;

  final SyncFailure kind;

  @override
  String toString() => 'SplitsSyncException: $message';
}
