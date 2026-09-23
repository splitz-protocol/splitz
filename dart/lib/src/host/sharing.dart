/// Getting a bill from one phone to another.
///
/// Two scans and no network: an invite carries the bill's id and the key its
/// contents are encrypted under, and a payload carries the log itself so a
/// joiner holds a bill rather than a name to go looking for.
///
/// This layer carries the key; it does not encrypt. §11.3 puts the cipher in
/// the wallet, and nothing here pretends otherwise.
library;

import 'package:splitz_core/splitz_core.dart' as splitz;

import 'bill_log.dart';
import 'host.dart';

/// What a scan produced.
sealed class Scanned {
  const Scanned();
}

/// An invite: a bill's id and its key, and nothing else. The log still has to
/// arrive from somewhere.
final class ScannedInvite extends Scanned {
  const ScannedInvite(this.invite);
  final splitz.Invite invite;
}

/// A bill and, on the full form, the invite that opens it.
final class ScannedBill extends Scanned {
  const ScannedBill({
    required this.entries,
    required this.invite,
    this.notEntries = 0,
  });

  final List<Map<String, dynamic>> entries;

  /// Items in the scanned log that are not objects, and so not entries.
  /// [acceptScan] reports each as §10.1 refuses it, as the merge does for
  /// every other entry it will not take.
  final int notEntries;

  /// Present on a `splitz1:` payload, absent on a `splitzd1:` delta — a delta
  /// is for a reader that already holds the key (§11.2).
  final splitz.Invite? invite;
}

/// What a scan could not be read as.
final class ScanRefused extends Scanned {
  const ScanRefused(this.code);

  /// A §12 code. A wallet's message is derived from this, never written
  /// beside it.
  final String code;
}

/// Reads whatever a camera or a clipboard produced.
///
/// One entry point because a person points a camera at a square and does not
/// know which kind it is. Tried in order: a payload carries more, so it is
/// tried first.
Scanned readScan(String text) {
  try {
    final payload = splitz.decodePayload(text);
    final entries = <Map<String, dynamic>>[
      for (final e in payload.log)
        if (e is Map) e.cast<String, dynamic>(),
    ];
    final notEntries = payload.log.length - entries.length;
    splitz.Invite? invite;
    final raw = payload.invite;
    if (raw != null) {
      // §11.2 carries the invite verbatim and validates nothing inside it, so
      // every member here is whatever a peer wrote — a number, a list, absent.
      // Tested before use, never cast: a cast that fails throws a type error
      // rather than a refusal, and this is reached by pointing a camera at a
      // square somebody else made.
      final b = raw['b'];
      final k = raw['k'];
      if (b is String && k is String) {
        // §11.1 is what decides whether it is an invite, so it goes through
        // the real parser rather than being read field by field here.
        try {
          invite = splitz.parseInvite(
              splitz.renderInvite(splitz.Invite(billId: b, key: k)));
        } on splitz.SplitError {
          invite = null;
        }
      }
    }
    return ScannedBill(
        entries: entries, invite: invite, notEntries: notEntries);
  } on splitz.SplitError catch (e) {
    // Text claiming to be a payload is answered as one. Falling through to
    // the invite parser would hand a person "not an invite" for a bill QR
    // that is merely damaged, which names the wrong thing to fix.
    final trimmed = splitz.stripScanPadding(text);
    if (trimmed.startsWith(splitz.billPrefix) ||
        trimmed.startsWith(splitz.deltaPrefix)) {
      return ScanRefused(e.code);
    }
    // Not a payload at all. It may still be an invite.
  }

  try {
    return ScannedInvite(splitz.parseInvite(text));
  } on splitz.SplitError catch (e) {
    return ScanRefused(e.code);
  }
}

/// The invite for a bill this device holds.
///
/// The key is the wallet's: §11.1 says the invite carries it and nothing here
/// mints one.
String inviteFor({
  required splitz.Bill bill,
  required String key,
  String? name,
  DateTime? expiry,
}) {
  return splitz.renderInvite(splitz.Invite(
    billId: bill.id,
    key: key,
    name: name ?? '',
    expiry:
        expiry == null ? null : expiry.toUtc().millisecondsSinceEpoch ~/ 1000,
  ));
}

/// One square carrying the whole bill, or null when it has outgrown a scan.
///
/// §11.2 caps a payload, and a bill with several people carrying payout
/// addresses reaches that cap quickly — two people and one expense, or three
/// people and none. A bill past it needs a relay, and this returns null rather
/// than a code so a caller has a state to show instead of an error to report.
String? shareableBill({
  required BillLog log,
  required String key,
  required splitz.Bill bill,
}) {
  final body = <String, dynamic>{
    'v': 1,
    'invite': <String, dynamic>{'v': 1, 'b': bill.id, 'k': key},
    'log': log.entries,
  };
  try {
    return splitz.encodePayload(splitz.billPrefix, body);
  } on splitz.SplitError {
    return null;
  }
}

/// What [theyHave] is missing, as one square when it fits (§14.5).
///
/// Delegates to the protocol: which entries a peer lacks and whether they fit
/// §11.2's cap is a function of the log, so a second implementation here is a
/// second place for the rule to drift.
splitz.Delta deltaFor({
  required BillLog log,
  required Iterable<String> theyHave,
}) =>
    splitz.deltaFor(log.entries, theyHave.toSet());

/// Folds a scanned bill into this device's own log, reporting what it refused.
List<splitz.SetAside> acceptScan(BillLog log, ScannedBill scan) {
  final refused = [
    ...log.add(scan.entries),
    for (var i = 0; i < scan.notEntries; i++)
      const splitz.SetAside('', splitz.SplitCode.billTypeError),
  ];
  // §10.2's order for a refusal list, as the merge gives it.
  refused.sort((a, b) {
    final byId = splitz.compareUtf8(a.id, b.id);
    return byId != 0 ? byId : splitz.compareUtf8(a.code, b.code);
  });
  return refused;
}

/// Whether this host can act on [bill] — that is, whether it has joined.
bool hasJoined(BillHost host, splitz.Bill bill) =>
    bill.participants.any((p) => p.id == host.me);
