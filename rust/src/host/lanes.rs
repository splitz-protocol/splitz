//! Which lane each debt settles in, and what that lane needs from a wallet.
//!
//! §6 decides who owes what; it says nothing about how the money travels.
//! A recipient's first payout preference (§9.1) decides that, and there are
//! three answers: a Zcash output this device can put in a payment request, a
//! swap off this chain into the asset they asked for, or cash.
//!
//! §8.5 already separates the first from the other two — `render_obligation`
//! carries what it can and reports the rest as unpayable, with
//! `payout_not_zec` distinguished from `no_address`. This module is the other
//! half: what to do with a recipient it reported.

use crate::error::{code, Result, SplitError};
use crate::model::{Bill, Participant, Payout};
use crate::settle::Settlement;

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

/// One debt, and the lane it settles in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LanedDebt {
    /// The participant owed.
    pub to: String,
    /// Minor units of the bill's currency (§2.1).
    pub amount: i64,
    pub lane: SettleLane,
    /// The preference that chose `lane`, absent when the participant declared
    /// none and `pay_to` stood in.
    pub payout: Option<Payout>,
}

impl LanedDebt {
    /// The asset this debt is swapped into, for [`SettleLane::Swap`].
    pub fn asset(&self) -> Option<&str> {
        self.payout.as_ref()?.asset.as_deref()
    }

    /// The chain that asset is delivered on, for [`SettleLane::Swap`].
    ///
    /// **Read together with [`LanedDebt::asset`], never separately.** One
    /// symbol exists on many chains, and a swap that matches the symbol alone
    /// delivers the right token to the wrong network, where the recipient
    /// cannot reach it.
    pub fn chain(&self) -> Option<&str> {
        self.payout.as_ref()?.chain.as_deref()
    }

    /// Where the money goes, for [`SettleLane::Swap`] and [`SettleLane::Zec`].
    pub fn address(&self) -> Option<&str> {
        self.payout.as_ref()?.address.as_deref()
    }
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

/// Every debt in `settlements`, sorted into lanes.
///
/// The whole obligation, not the part one lane can carry: a screen that shows
/// only the batchable debts is the §8.5 hazard restated one layer up.
pub fn lane_debts(settlements: &[Settlement], bill: &Bill) -> Result<Vec<LanedDebt>> {
    let mut laned = Vec::with_capacity(settlements.len());
    for s in settlements {
        // Same fault `render_obligation` raises on, and the same remedy: this
        // is a merge or storage problem, not a missing address.
        let who = bill.participant(&s.to).ok_or_else(|| {
            SplitError::new(
                code::UNKNOWN_PARTICIPANT,
                format!("The plan settles to {}, who is not on this bill", s.to),
            )
        })?;
        laned.push(LanedDebt {
            to: s.to.clone(),
            amount: s.amount,
            lane: lane_for(who),
            payout: who.payouts.first().cloned(),
        });
    }
    Ok(laned)
}

/// The debts in `lane`, in the order they were laned.
pub fn in_lane(debts: &[LanedDebt], lane: SettleLane) -> Vec<LanedDebt> {
    debts.iter().filter(|d| d.lane == lane).cloned().collect()
}
