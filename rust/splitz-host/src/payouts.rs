//! Editing the payouts a participant declares (SPEC.md §9.1).

use splitz_core::{is_transparent_address, Participant, Payout};

/// `first`, then every payout `who` declares that it does not take the place
/// of, in their declared order — except that a transparent Zcash payout is
/// never put ahead of a shielded one.
///
/// A record with no payouts declares its `pay_to` as its one Zcash payout, and
/// one with neither declares nothing. `first` takes the place of every
/// declared payout of its own kind, a `zec` payout only of those as public as
/// it (transparent against transparent, shielded against shielded, by
/// [`is_transparent_address`]), and a `swap` only of the same asset (compared
/// case-insensitively; the chain does not distinguish): one way to be paid
/// per kind, so order — which is preference — is never reshuffled by an edit.
/// A transparent `first` goes directly after the last shielded Zcash payout
/// the list keeps, when it keeps one: a payment to it is public on chain, so
/// it is opted into as a fallback and never displaces a private one (§9.1).
/// Pure; nothing is written.
pub fn ranked_payouts(who: &Participant, first: &Payout) -> Vec<Payout> {
    let synthesised;
    let declared: &[Payout] = if !who.payouts.is_empty() {
        &who.payouts
    } else if let Some(pay_to) = who.pay_to.as_deref().filter(|a| !a.is_empty()) {
        synthesised = [Payout {
            kind: "zec".to_owned(),
            address: Some(pay_to.to_owned()),
            asset: None,
            chain: None,
        }];
        &synthesised
    } else {
        &[]
    };
    let public = |p: &Payout| is_transparent_address(p.address.as_deref().unwrap_or(""));
    let replaced = |p: &Payout| {
        p.kind == first.kind
            && match p.kind.as_str() {
                "swap" => p
                    .asset
                    .as_deref()
                    .unwrap_or("")
                    .eq_ignore_ascii_case(first.asset.as_deref().unwrap_or("")),
                "zec" => public(p) == public(first),
                _ => true,
            }
    };
    let mut kept: Vec<Payout> = declared.iter().filter(|p| !replaced(p)).cloned().collect();
    let at = if first.kind == "zec" && public(first) {
        kept.iter()
            .rposition(|p| p.kind == "zec" && !public(p))
            .map_or(0, |i| i + 1)
    } else {
        0
    };
    kept.insert(at, first.clone());
    kept
}

/// The payout a payer's wallet settles a debt by when it cannot pay by the
/// recipient's first (§14.8), and why it passed the first over.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PayoutFallback {
    /// The position, in the recipient's declared order, of the payout to pay
    /// by.
    pub index: usize,
    /// Why the first could not be paid, in the wallet's words: what §14.8
    /// requires the payer be shown.
    pub passed_over: String,
}

/// `cannot_pay` holds, for each of a recipient's declared payouts in their
/// order, why this wallet cannot pay by it, or `None` when it can — what the
/// wallet can pay is the wallet's to say. Answers the next payout it can pay,
/// in the recipient's order, when it cannot pay the first (§14.8); `None` when
/// it can pay the first, or none at all.
pub fn payout_fallback(cannot_pay: &[Option<String>]) -> Option<PayoutFallback> {
    let first = cannot_pay.first()?.as_ref()?;
    cannot_pay
        .iter()
        .position(Option::is_none)
        .map(|index| PayoutFallback {
            index,
            passed_over: first.clone(),
        })
}
