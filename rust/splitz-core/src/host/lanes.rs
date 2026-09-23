//! Which lane each debt settles in, and what that lane needs from a wallet.
//!
//! §6 decides who owes what; it says nothing about how the money travels.
//! A recipient's first payout preference (§9.1) decides that, and there are
//! three answers: a Zcash output this device can put in a payment request, a
//! swap off this chain into the asset they asked for, or cash.
//!
//! §8.5 already separates the first from the other two — `render_obligation`
//! carries what it can and reports the rest as unpayable, with
//! `payout_not_zec` distinguished from `no_address` and `bad_address`. This
//! module is the other half: what to do with a recipient it reported.

use crate::model::Participant;

/// How one debt settles.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettleLane {
    /// An output in the payment request. Several recipients share one
    /// transaction.
    Zec,
    /// Swapped off this chain into the asset the recipient asked for. **One
    /// swap per recipient** — a swap has one destination, so these cannot be
    /// batched the way [`SettleLane::Zec`] outputs can.
    Swap,
    /// Handed over outside this protocol. Unverifiable by construction (§9.2):
    /// anyone on the bill can claim it, and a UI must not present it with the
    /// confidence of an on-chain payment.
    Cash,
    /// The recipient has published nothing to pay to. Not a lane — a debt that
    /// cannot be settled until they declare somewhere.
    None,
}

/// The lane `participant` is paid in.
///
/// The **first** declared preference decides, and only the first: §9.1 makes
/// the order the preference order, so falling through to the second because
/// the first is inconvenient pays them somewhere they ranked lower.
///
/// A participant who declared nothing is paid by `pay_to` if they have one.
pub fn lane_for(participant: &Participant) -> SettleLane {
    let Some(first) = participant.payouts.first() else {
        return match participant.pay_to.as_deref() {
            Some(pay_to) if !pay_to.is_empty() => SettleLane::Zec,
            _ => SettleLane::None,
        };
    };
    match first.kind.as_str() {
        "zec" => match first.address.as_deref() {
            Some(address) if !address.is_empty() => SettleLane::Zec,
            _ => SettleLane::None,
        },
        "swap" => SettleLane::Swap,
        "cash" => SettleLane::Cash,
        // Unreachable through a decoded bill: §9.1 makes a reader refuse a
        // payout type it does not define rather than admit it, so no other
        // string survives into a `Participant`. Answering `None` rather than
        // panicking keeps a hand-built participant from taking down a settle
        // screen.
        _ => SettleLane::None,
    }
}
