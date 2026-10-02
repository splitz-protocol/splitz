//! SPEC.md §9.1: editing a participant's declared payouts.

use splitz_core::{Participant, Payout};
use splitz_host::ranked_payouts;

fn zec(address: &str) -> Payout {
    Payout {
        kind: "zec".into(),
        address: Some(address.into()),
        asset: None,
        chain: None,
    }
}

fn swap(asset: &str, chain: &str, address: &str) -> Payout {
    Payout {
        kind: "swap".into(),
        address: Some(address.into()),
        asset: Some(asset.into()),
        chain: Some(chain.into()),
    }
}

fn cash() -> Payout {
    Payout {
        kind: "cash".into(),
        address: None,
        asset: None,
        chain: None,
    }
}

fn who(pay_to: Option<&str>, payouts: Vec<Payout>) -> Participant {
    Participant {
        id: "p".into(),
        name: "P".into(),
        pay_to: pay_to.map(str::to_owned),
        identity_key: None,
        payouts,
    }
}

fn keys(ps: &[Payout]) -> Vec<String> {
    ps.iter()
        .map(|p| {
            format!(
                "{}:{}:{}",
                p.kind,
                p.asset.as_deref().unwrap_or(""),
                p.address.as_deref().unwrap_or("")
            )
        })
        .collect()
}

#[test]
fn a_pay_to_only_record_declares_one_zec_payout_which_a_zec_replaces() {
    assert_eq!(
        keys(&ranked_payouts(&who(Some("zOld"), vec![]), &zec("zA"))),
        ["zec::zA"]
    );
}

#[test]
fn a_pay_to_only_record_keeps_its_zec_behind_a_payout_of_another_kind() {
    assert_eq!(
        keys(&ranked_payouts(
            &who(Some("zOld"), vec![]),
            &swap("USDC", "eth", "0xa")
        )),
        ["swap:USDC:0xa", "zec::zOld"]
    );
}

#[test]
fn a_record_declaring_nothing_yields_only_the_new_payout() {
    assert_eq!(
        keys(&ranked_payouts(&who(None, vec![]), &cash())),
        ["cash::"]
    );
    assert_eq!(
        keys(&ranked_payouts(&who(Some(""), vec![]), &cash())),
        ["cash::"]
    );
}

#[test]
fn the_new_payout_goes_first_and_replaces_one_of_its_own_kind() {
    let p = who(None, vec![swap("USDC", "eth", "0xa"), zec("zA"), cash()]);
    assert_eq!(
        keys(&ranked_payouts(&p, &zec("zB"))),
        ["zec::zB", "swap:USDC:0xa", "cash::"]
    );
}

#[test]
fn every_other_payout_keeps_its_declared_order() {
    let p = who(
        None,
        vec![
            cash(),
            swap("USDT", "eth", "0xb"),
            zec("zA"),
            swap("USDC", "eth", "0xa"),
        ],
    );
    assert_eq!(
        keys(&ranked_payouts(&p, &zec("zB"))),
        ["zec::zB", "cash::", "swap:USDT:0xb", "swap:USDC:0xa"]
    );
}

#[test]
fn a_swap_replaces_only_the_same_asset_whatever_its_case_or_chain() {
    let p = who(
        None,
        vec![
            zec("zA"),
            swap("USDC", "eth", "0xa"),
            swap("USDT", "eth", "0xb"),
        ],
    );
    assert_eq!(
        keys(&ranked_payouts(&p, &swap("usdc", "sol", "sA"))),
        ["swap:usdc:sA", "zec::zA", "swap:USDT:0xb"]
    );
}

#[test]
fn a_swap_leaves_a_zec_and_cash_where_they_were() {
    let p = who(None, vec![cash(), zec("zA")]);
    assert_eq!(
        keys(&ranked_payouts(&p, &swap("USDT", "eth", "0xb"))),
        ["swap:USDT:0xb", "cash::", "zec::zA"]
    );
}

#[test]
fn payouts_take_precedence_over_pay_to() {
    let p = who(Some("zOld"), vec![cash()]);
    assert_eq!(keys(&ranked_payouts(&p, &zec("zA"))), ["zec::zA", "cash::"]);
}

#[test]
fn declared_payouts_of_one_kind_are_all_replaced() {
    let p = who(None, vec![zec("zA"), cash(), zec("zB")]);
    assert_eq!(keys(&ranked_payouts(&p, &zec("zA"))), ["zec::zA", "cash::"]);
}

#[test]
fn the_input_is_not_modified() {
    let p = who(None, vec![zec("zA"), cash()]);
    let before = p.clone();
    ranked_payouts(&p, &zec("zB"));
    assert_eq!(p, before);
}
