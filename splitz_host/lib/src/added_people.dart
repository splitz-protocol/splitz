/// Somebody the creator puts on a bill before they have joined (§14.11).
library;

import 'package:splitz_core/host.dart' as splitz;
import 'package:splitz_core/splitz_core.dart' as protocol;

/// A host writing as [me] on [inner]'s clock and randomness, for somebody
/// added by hand.
///
/// Signs nothing: this device holds no key of theirs, and signing an entry
/// authored by them with this device's key would assert something false.
/// §10.7 binds nothing to an entry written this way, which is the honest
/// state — the person binds their own identity by joining from a device of
/// their own.
class HostAs implements splitz.BillHost {
  HostAs(this._inner, this.me);

  final splitz.BillHost _inner;

  @override
  final String me;

  @override
  splitz.Clock get now => _inner.now;

  @override
  splitz.Randomness get randomBytes => _inner.randomBytes;

  @override
  splitz.Broadcast get broadcast => _inner.broadcast;

  @override
  splitz.SignEntry? get sign => null;

  @override
  splitz.VerifyEntry? get verify => _inner.verify;

  @override
  splitz.ReadsAddress? get readsAddress => _inner.readsAddress;
}

/// The `joinBill` that puts [name] on [folded] under [id], written as them
/// and unsigned through [HostAs] (§14.11).
///
/// Refused with `duplicate_participant` when [id] is this device's own or
/// already on the bill: a join under a taken id renames the person holding it
/// and wipes their payout, and one under this device's own id renames its
/// holder. Refused with `bill_missing_entry_payload` for an empty [id], which
/// names nobody (§9.1).
Map<String, dynamic> addPersonEntry({
  required splitz.BillHost host,
  required splitz.FoldedBill folded,
  required String id,
  required String name,
}) {
  if (id.isEmpty) {
    throw const protocol.SplitError(
      protocol.SplitCode.billMissingEntryPayload,
      'A person added needs an id',
    );
  }
  if (id == host.me || folded.bill.participant(id) != null) {
    throw const protocol.SplitError(
      protocol.SplitCode.duplicateParticipant,
      'Somebody on this bill already goes by that',
    );
  }
  return splitz.joinBill(host: HostAs(host, id), name: name);
}

/// Whether this device is on [folded] as itself (§10.7): a record under [me]
/// that the fold binds to a key — this device's, since [me] is the id its key
/// derives.
///
/// A record under [me] alone is not: anybody holding the invite can write an
/// unsigned join under an id they know, with their own payout, before the
/// person joins. A device that took that record for its own would never write
/// its signed join, and every payment owed to it would go to the address the
/// record names. While this is false, the device writes its own join, which
/// binds its key and takes the record back.
bool joinedAsMe(splitz.FoldedBill folded, String me) =>
    folded.bill.participant(me) != null &&
    folded.identities.bound.containsKey(me);
