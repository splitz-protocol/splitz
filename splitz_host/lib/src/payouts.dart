/// Editing the payouts a participant declares (SPEC.md §9.1).
library;

import 'package:splitz_core/host.dart' show Payout;
import 'package:splitz_core/splitz_core.dart'
    show Participant, isTransparentAddress;

import 'ascii.dart';

/// [first], then every payout [who] declares that it does not take the place
/// of, in their declared order — except that a transparent Zcash payout is
/// never put ahead of a shielded one.
///
/// A record with no payouts declares its `payTo` as its one Zcash payout, and
/// one with neither declares nothing. [first] takes the place of every
/// declared payout of its own type, a `zec` payout only of those as public as
/// it (transparent against transparent, shielded against shielded, by
/// [isTransparentAddress]), and a `swap` only of the same asset (compared
/// case-insensitively; the chain does not distinguish): one way to be paid
/// per kind, so order — which is preference — is never reshuffled by an
/// edit. A transparent [first] goes directly after the last shielded Zcash
/// payout the list keeps, when it keeps one: a payment to it is public on
/// chain, so it is opted into as a fallback and never displaces a private
/// one (§9.1). Pure; nothing is written.
List<Payout> rankedPayouts(Participant who, Payout first) {
  final payTo = who.payTo;
  final declared = who.payouts.isNotEmpty
      ? who.payouts
      : [
          if (payTo != null && payTo.isNotEmpty)
            Payout(type: 'zec', address: payTo),
        ];
  bool public(Payout p) => isTransparentAddress(p.address ?? '');
  bool replaced(Payout p) =>
      p.type == first.type &&
      switch (p.type) {
        'swap' => equalsIgnoringAsciiCase(p.asset ?? '', first.asset ?? ''),
        'zec' => public(p) == public(first),
        _ => true,
      };
  final kept = [...declared.where((p) => !replaced(p))];
  final at = first.type == 'zec' && public(first)
      ? kept.lastIndexWhere((p) => p.type == 'zec' && !public(p)) + 1
      : 0;
  return [...kept.take(at), first, ...kept.skip(at)];
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
