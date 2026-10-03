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

/// The payout a payer's wallet settles a debt by when it cannot pay by the
/// recipient's first (§14.8), and why it passed the first over.
class PayoutFallback {
  const PayoutFallback({required this.index, required this.passedOver});

  /// The position, in the recipient's declared order, of the payout to pay by.
  final int index;

  /// Why the first could not be paid, in the wallet's words: what §14.8
  /// requires the payer be shown.
  final String passedOver;
}

/// [cannotPay] holds, for each of a recipient's declared payouts in their
/// order, why this wallet cannot pay by it, or null when it can — what the
/// wallet can pay is the wallet's to say. Answers the next payout it can pay,
/// in the recipient's order, when it cannot pay the first (§14.8); null when
/// it can pay the first, or none at all.
PayoutFallback? payoutFallback(List<String?> cannotPay) {
  if (cannotPay.isEmpty) return null;
  final first = cannotPay.first;
  if (first == null) return null;
  for (var i = 1; i < cannotPay.length; i++) {
    if (cannotPay[i] == null) {
      return PayoutFallback(index: i, passedOver: first);
    }
  }
  return null;
}
