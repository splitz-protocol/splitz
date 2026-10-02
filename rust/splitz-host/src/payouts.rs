//! Editing the payouts a participant declares (SPEC.md §9.1).

use splitz_core::{Participant, Payout};

/// `first`, then every payout `who` declares that it does not take the place
/// of, in their declared order.
///
/// A record with no payouts declares its `pay_to` as its one Zcash payout, and
/// one with neither declares nothing. `first` takes the place of every
/// declared payout of its own kind, and a `swap` only of the same asset
/// (compared case-insensitively; the chain does not distinguish): one way to
/// be paid per kind, so order — which is preference — is never reshuffled by
/// an edit. Pure; nothing is written.
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
    let replaced = |p: &Payout| {
        p.kind == first.kind
            && (p.kind != "swap"
                || p.asset.as_deref().unwrap_or("").to_uppercase()
                    == first.asset.as_deref().unwrap_or("").to_uppercase())
    };
    std::iter::once(first.clone())
        .chain(declared.iter().filter(|p| !replaced(p)).cloned())
        .collect()
}
