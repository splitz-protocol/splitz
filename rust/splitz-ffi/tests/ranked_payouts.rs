//! §9.1: the binding carries the host's payout edit unchanged.

use splitz_ffi::{ranked_payouts, Participant, Payout};

fn payout(kind: &str, asset: Option<&str>, address: Option<&str>) -> Payout {
    Payout {
        kind: kind.into(),
        address: address.map(str::to_owned),
        asset: asset.map(str::to_owned),
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

#[test]
fn a_pay_to_only_record_keeps_its_zec_behind_a_swap() {
    let out = ranked_payouts(
        who(Some("zOld"), vec![]),
        payout("swap", Some("USDC"), Some("0xa")),
    );
    assert_eq!(
        out,
        [
            payout("swap", Some("USDC"), Some("0xa")),
            payout("zec", None, Some("zOld"))
        ]
    );
}

#[test]
fn the_new_payout_replaces_its_own_kind_and_the_rest_keep_their_order() {
    let declared = vec![
        payout("cash", None, None),
        payout("zec", None, Some("zA")),
        payout("swap", Some("USDT"), Some("0xb")),
    ];
    let out = ranked_payouts(who(None, declared), payout("zec", None, Some("zB")));
    assert_eq!(
        out,
        [
            payout("zec", None, Some("zB")),
            payout("cash", None, None),
            payout("swap", Some("USDT"), Some("0xb"))
        ]
    );
}
