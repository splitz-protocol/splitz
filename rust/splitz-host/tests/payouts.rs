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

#[test]
fn a_first_payout_this_wallet_cannot_pay_is_passed_over_for_the_next_it_can() {
    use splitz_host::payout_fallback;
    let no = |why: &str| Some(why.to_owned());
    // The first is payable: nothing is passed over.
    assert_eq!(payout_fallback(&[None, no("x")]), None);
    // The next payable one, in their order, and why the first was passed.
    let next = payout_fallback(&[no("not on base"), None]).unwrap();
    assert_eq!((next.index, next.passed_over.as_str()), (1, "not on base"));
    assert_eq!(payout_fallback(&[no("a"), no("b"), None]).unwrap().index, 2);
    // None payable, nothing declared, or only the first: no fallback.
    assert_eq!(payout_fallback(&[no("a"), no("b")]), None);
    assert_eq!(payout_fallback(&[]), None);
    assert_eq!(payout_fallback(&[no("a")]), None);
}

const UA: &str = "u1l8xunezsvhq8fgzfl7404m450nwnd76zshscn6nfys7vyz2ywyh4cc5daaq0c7q2su5lqfh23sp7fkf3kt27ve5948mzpfdvckzaect2jtte308mkwlycj2u0eac077wu70vqcetkxf";
const ZS: &str = "zs1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqpq6d8g";
const T1: &str = "t1Hsc1LR8yKnbbe3twRp88p6vFfC5t7DLbs";
const T3: &str = "t3JZcvsuaXE6ygokL4XUiZSTrQBUoPYFnXJ";
const TEX: &str = "tex1s2rt77ggv6q989lr49rkgzmh5slsksa9khdgte";

fn ranked(declared: Vec<Payout>, first: Payout, pay_to: Option<&str>) -> Vec<String> {
    ranked_payouts(&who(pay_to, declared), &first)
        .into_iter()
        .map(|p| match p.address {
            Some(a) if p.kind == "zec" => a.chars().take(4).collect(),
            _ => p.kind,
        })
        .collect()
}

#[test]
fn a_transparent_zec_payout_goes_after_the_shielded_one_never_ahead() {
    assert_eq!(ranked(vec![zec(UA)], zec(T1), None), ["u1l8", "t1Hs"]);
    assert_eq!(
        ranked(
            vec![swap("USDC", "eth", "0xa"), zec(UA), cash()],
            zec(T1),
            None
        ),
        ["swap", "u1l8", "t1Hs", "cash"]
    );
    assert_eq!(ranked(vec![], zec(T1), Some(UA)), ["u1l8", "t1Hs"]);
}

#[test]
fn p2sh_and_tex_are_transparent_and_replace_a_transparent_one_in_place() {
    assert_eq!(
        ranked(vec![zec(UA), zec(T1)], zec(T3), None),
        ["u1l8", "t3JZ"]
    );
    assert_eq!(
        ranked(vec![cash(), zec(UA), zec(T1)], zec(TEX), None),
        ["cash", "u1l8", "tex1"]
    );
}

#[test]
fn a_transparent_zec_payout_with_no_shielded_one_goes_first() {
    assert_eq!(ranked(vec![cash()], zec(T1), None), ["t1Hs", "cash"]);
    assert_eq!(ranked(vec![], zec(T1), None), ["t1Hs"]);
}

#[test]
fn a_shielded_payout_replaces_only_a_shielded_one_and_goes_first() {
    assert_eq!(ranked(vec![zec(T1)], zec(UA), None), ["u1l8", "t1Hs"]);
    assert_eq!(
        ranked(vec![zec(UA), zec(T1), cash()], zec(ZS), None),
        ["zs1q", "t1Hs", "cash"]
    );
}

#[test]
fn an_address_section_8_6_does_not_decode_is_not_taken_for_transparent() {
    assert_eq!(ranked(vec![zec("zA")], zec(T1), None), ["zA", "t1Hs"]);
    assert_eq!(
        ranked(vec![zec(T1), zec("zA")], zec(UA), None),
        ["u1l8", "t1Hs"]
    );
}

#[test]
fn an_asset_is_compared_by_ascii_case_alone() {
    let swap_of = |asset: &str| swap(asset, "eth", &format!("0x{asset}"));
    assert_eq!(
        keys(&ranked_payouts(
            &who(None, vec![swap_of("usdc")]),
            &swap_of("USDC")
        )),
        keys(&[swap_of("USDC")])
    );
    for (held, next) in [("ß", "SS"), ("ﬁ", "FI"), ("é", "É")] {
        assert_eq!(
            keys(&ranked_payouts(
                &who(None, vec![swap_of(held)]),
                &swap_of(next)
            )),
            keys(&[swap_of(next), swap_of(held)]),
            "{held}"
        );
    }
}
