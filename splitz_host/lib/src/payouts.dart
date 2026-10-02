/// Editing the payouts a participant declares (SPEC.md §9.1).
library;

import 'package:splitz_core/host.dart' show Payout;
import 'package:splitz_core/splitz_core.dart' show Participant;

/// [first], then every payout [who] declares that it does not take the place
/// of, in their declared order.
///
/// A record with no payouts declares its `payTo` as its one Zcash payout, and
/// one with neither declares nothing. [first] takes the place of every
/// declared payout of its own type, and a `swap` only of the same asset
/// (compared case-insensitively; the chain does not distinguish): one way to
/// be paid per kind, so order — which is preference — is never reshuffled by
/// an edit. Pure; nothing is written.
List<Payout> rankedPayouts(Participant who, Payout first) {
  final payTo = who.payTo;
  final declared = who.payouts.isNotEmpty
      ? who.payouts
      : [
          if (payTo != null && payTo.isNotEmpty)
            Payout(type: 'zec', address: payTo),
        ];
  bool replaced(Payout p) =>
      p.type == first.type &&
      (p.type != 'swap' ||
          (p.asset ?? '').toUpperCase() == (first.asset ?? '').toUpperCase());
  return [first, ...declared.where((p) => !replaced(p))];
}
