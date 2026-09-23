/// Which lane each debt settles in, and what that lane needs from a wallet.
///
/// §6 decides who owes what; it says nothing about how the money travels.
/// A recipient's first payout preference (§9.1) decides that, and there are
/// three answers: a Zcash output this device can put in a payment request, a
/// swap off this chain into the asset they asked for, or cash.
///
/// §8.5 already separates the first from the other two — `renderObligation`
/// carries what it can and reports the rest as unpayable, with
/// `payout_not_zec` distinguished from `no_address` and `bad_address`. This
/// file is the other half: what to do with a recipient it reported.
library;

import 'package:splitz_core/splitz_core.dart' as splitz;

/// How one debt settles.
enum SettleLane {
  /// An output in the payment request. Several recipients share one
  /// transaction.
  zec,

  /// Swapped off this chain into the asset the recipient asked for. **One
  /// swap per recipient** — a swap has one destination, so these cannot be
  /// batched the way [zec] outputs can.
  swap,

  /// Handed over outside this protocol. Unverifiable by construction (§9.2):
  /// anyone on the bill can claim it, and a UI must not present it with the
  /// confidence of an on-chain payment.
  cash,

  /// The recipient has published nothing to pay to. Not a lane — a debt that
  /// cannot be settled until they declare somewhere.
  none,
}

/// The lane [participant] is paid in.
///
/// The **first** declared preference decides, and only the first: §9.1 makes
/// the order the preference order, so falling through to the second because
/// the first is inconvenient pays them somewhere they ranked lower.
///
/// A participant who declared nothing is paid by `payTo` if they have one.
SettleLane laneFor(splitz.Participant participant) {
  if (participant.payouts.isEmpty) {
    final payTo = participant.payTo;
    return (payTo == null || payTo.isEmpty) ? SettleLane.none : SettleLane.zec;
  }
  switch (participant.payouts.first.type) {
    case 'zec':
      final address = participant.payouts.first.address;
      return (address == null || address.isEmpty)
          ? SettleLane.none
          : SettleLane.zec;
    case 'swap':
      return SettleLane.swap;
    case 'cash':
      return SettleLane.cash;
  }
  // Unreachable through a decoded bill: §9.1 makes a reader refuse a payout
  // type it does not define rather than admit it, so no other string survives
  // into a `Participant`. Answering `none` rather than throwing keeps a
  // hand-built participant from taking down a settle screen.
  return SettleLane.none;
}
