/// Which lane each debt settles in, and what that lane needs from a wallet.
///
/// §6 decides who owes what; it says nothing about how the money travels.
/// A recipient's first payout preference (§9.1) decides that, and there are
/// three answers: a Zcash output this device can put in a payment request, a
/// swap off this chain into the asset they asked for, or cash.
///
/// §8.5 already separates the first from the other two — `renderObligation`
/// carries what it can and reports the rest as unpayable, with
/// `payout_not_zec` distinguished from `no_address`. This file is the other
/// half: what to do with a recipient it reported.
library;

import 'package:splitz/splitz.dart' as splitz;

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

/// One debt, and the lane it settles in.
class LanedDebt {
  const LanedDebt({
    required this.to,
    required this.amount,
    required this.lane,
    this.payout,
  });

  /// The participant owed.
  final String to;

  /// Minor units of the bill's currency (§2.1).
  final int amount;

  final SettleLane lane;

  /// The preference that chose [lane], absent when the participant declared
  /// none and [payTo] stood in.
  final splitz.Payout? payout;

  /// The asset this debt is swapped into, for [SettleLane.swap].
  String? get asset => payout?.asset;

  /// The chain that asset is delivered on, for [SettleLane.swap].
  ///
  /// **Read together with [asset], never separately.** One symbol exists on
  /// many chains, and a swap that matches the symbol alone delivers the right
  /// token to the wrong network, where the recipient cannot reach it.
  String? get chain => payout?.chain;

  /// Where the money goes, for [SettleLane.swap] and [SettleLane.zec].
  String? get address => payout?.address;
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

/// Every debt in [settlements], sorted into lanes.
///
/// The whole obligation, not the part one lane can carry: a screen that shows
/// only the batchable debts is the §8.5 hazard restated one layer up.
List<LanedDebt> laneDebts(
  List<splitz.Settlement> settlements,
  splitz.Bill bill,
) {
  final laned = <LanedDebt>[];
  for (final s in settlements) {
    final who = bill.participant(s.to);
    if (who == null) {
      // Same fault `renderObligation` raises on, and the same remedy: this is
      // a merge or storage problem, not a missing address.
      throw splitz.SplitError(splitz.SplitCode.unknownParticipant,
          'The plan settles to ${s.to}, who is not on this bill');
    }
    laned.add(LanedDebt(
      to: s.to,
      amount: s.amount,
      lane: laneFor(who),
      payout: who.payouts.isEmpty ? null : who.payouts.first,
    ));
  }
  return laned;
}

/// The debts in [lane], in the order they were laned.
List<LanedDebt> inLane(List<LanedDebt> debts, SettleLane lane) => [
      for (final d in debts)
        if (d.lane == lane) d
    ];
