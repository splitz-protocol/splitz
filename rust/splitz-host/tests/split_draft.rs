//! A split being edited, against the protocol that decides what it may be.
//!
//! Every refusal asserted here is produced by building the real §4 payload and
//! splitting a real expense with it — so none of these codes can outlive the
//! rule that causes them, and none can be invented.

use splitz_core::code;
use splitz_host::{DraftItem, SplitDraft, SplitKind};

fn among(kind: SplitKind, who: &[&str]) -> SplitDraft {
    let mut draft = SplitDraft::new(kind);
    draft.among = who.iter().map(|s| (*s).to_owned()).collect();
    draft
}

fn weighted(kind: SplitKind, pairs: &[(&str, i64)]) -> SplitDraft {
    let mut draft = SplitDraft::new(kind);
    let map = pairs
        .iter()
        .map(|(id, n)| ((*id).to_owned(), *n))
        .collect::<std::collections::BTreeMap<_, _>>();
    match kind {
        SplitKind::Exact => draft.amounts = map,
        SplitKind::Percentage => draft.basis_points = map,
        SplitKind::Shares => draft.share_counts = map,
        _ => panic!("that kind carries no weights"),
    }
    draft
}

fn item(description: &str, minor_units: i64, shared_by: &[&str]) -> DraftItem {
    DraftItem {
        description: description.to_owned(),
        minor_units,
        shared_by: shared_by.iter().map(|s| (*s).to_owned()).collect(),
    }
}

// --- each of the five reaches the protocol ---------------------------------

#[test]
fn equal() {
    let draft = among(SplitKind::Equal, &["ana", "ben"]);
    assert_eq!(draft.refusal_code(9000), None);
    assert_eq!(
        draft.allocation(9000).unwrap(),
        [("ana".to_owned(), 4500), ("ben".to_owned(), 4500)].into()
    );
}

#[test]
fn equal_with_a_remainder_the_protocol_places() {
    // Largest remainder, exact integers: nobody loses a cent and the total is
    // preserved.
    let draft = among(SplitKind::Equal, &["ana", "ben", "cai"]);
    let allocated = draft.allocation(1000).unwrap();
    assert_eq!(allocated.values().sum::<i64>(), 1000);
    let mut parts: Vec<i64> = allocated.values().copied().collect();
    parts.sort_unstable();
    assert_eq!(parts, vec![333, 333, 334]);
}

#[test]
fn exact() {
    let draft = weighted(SplitKind::Exact, &[("ana", 6000), ("ben", 3000)]);
    assert_eq!(draft.refusal_code(9000), None);
    assert_eq!(
        draft.allocation(9000).unwrap(),
        [("ana".to_owned(), 6000), ("ben".to_owned(), 3000)].into()
    );
}

#[test]
fn percentage_carried_in_basis_points() {
    // 33.33% is 3333. No float ever touches an amount.
    let draft = weighted(SplitKind::Percentage, &[("ana", 3333), ("ben", 6667)]);
    assert_eq!(draft.refusal_code(9000), None);
    assert_eq!(draft.allocation(9000).unwrap().values().sum::<i64>(), 9000);
}

#[test]
fn shares() {
    let draft = weighted(SplitKind::Shares, &[("ana", 2), ("ben", 1)]);
    assert_eq!(draft.refusal_code(9000), None);
    assert_eq!(
        draft.allocation(9000).unwrap(),
        [("ana".to_owned(), 6000), ("ben".to_owned(), 3000)].into()
    );
}

#[test]
fn itemized_with_the_extra_spread_over_what_people_ate() {
    let mut draft = SplitDraft::new(SplitKind::Itemized);
    draft.items = vec![
        item("tacos", 6000, &["ana"]),
        item("beer", 2000, &["ana", "ben"]),
    ];
    draft.extra_minor_units = 1000;
    assert_eq!(draft.refusal_code(9000), None);
    let allocated = draft.allocation(9000).unwrap();
    assert_eq!(allocated.values().sum::<i64>(), 9000);
    // Ana ate more, so she carries more of the tip.
    assert!(allocated["ana"] > allocated["ben"]);
}

// --- what the protocol refuses, by its code --------------------------------

#[test]
fn nobody_sharing_it() {
    assert_eq!(
        SplitDraft::new(SplitKind::Equal).refusal_code(9000),
        Some(code::EMPTY_SPLIT)
    );
}

#[test]
fn exact_amounts_that_do_not_come_to_the_total() {
    let draft = weighted(SplitKind::Exact, &[("ana", 6000), ("ben", 2000)]);
    assert_eq!(draft.refusal_code(9000), Some(code::EXACT_TOTAL_MISMATCH));
    assert!(draft.allocation(9000).is_none());
}

#[test]
fn percentages_that_do_not_come_to_100() {
    let draft = weighted(SplitKind::Percentage, &[("ana", 3000), ("ben", 6000)]);
    assert_eq!(
        draft.refusal_code(9000),
        Some(code::PERCENTAGE_NOT_FULL_SCALE)
    );
}

#[test]
fn everybody_on_zero_shares() {
    let draft = weighted(SplitKind::Shares, &[("ana", 0), ("ben", 0)]);
    assert_eq!(draft.refusal_code(9000), Some(code::ZERO_WEIGHT_SUM));
}

#[test]
fn an_itemized_split_with_no_items() {
    assert_eq!(
        SplitDraft::new(SplitKind::Itemized).refusal_code(9000),
        Some(code::ITEMIZED_NO_ITEMS)
    );
}

#[test]
fn an_item_nobody_shared() {
    let mut draft = SplitDraft::new(SplitKind::Itemized);
    draft.items = vec![item("tacos", 9000, &[])];
    assert_eq!(
        draft.refusal_code(9000),
        Some(code::ITEMIZED_UNASSIGNED_ITEM)
    );
}

#[test]
fn items_that_do_not_come_to_the_total() {
    let mut draft = SplitDraft::new(SplitKind::Itemized);
    draft.items = vec![item("tacos", 5000, &["ana"])];
    draft.extra_minor_units = 1000;
    assert_eq!(
        draft.refusal_code(9000),
        Some(code::ITEMIZED_TOTAL_MISMATCH)
    );
}

#[test]
fn a_share_running_the_opposite_way_to_the_expense() {
    let draft = weighted(SplitKind::Exact, &[("ana", 10000), ("ben", -1000)]);
    assert_eq!(draft.refusal_code(9000), Some(code::NEGATIVE_SHARE));
}

// --- editing ---------------------------------------------------------------

#[test]
fn toggling_puts_somebody_in_and_takes_their_figure_back_out() {
    // A stale amount for a person no longer in the split is exactly what
    // `exact_total_mismatch` is.
    let mut draft = SplitDraft::new(SplitKind::Exact);
    draft.toggle("ana");
    assert!(draft.amounts.contains_key("ana"));
    draft.amounts.insert("ana".to_owned(), 9000);
    draft.toggle("ana");
    assert!(!draft.amounts.contains_key("ana"));
    assert!(draft.participants().is_empty());
}

#[test]
fn somebody_added_to_shares_starts_on_one_not_zero() {
    // Zero shares owes nothing, and every count zero is refused outright.
    let mut draft = SplitDraft::new(SplitKind::Shares);
    draft.toggle("ana");
    assert_eq!(draft.share_counts.get("ana"), Some(&1));
    assert_eq!(draft.refusal_code(9000), None);
}

#[test]
fn the_payload_is_the_protocol_shape_whatever_the_form_holds() {
    let draft = among(SplitKind::Equal, &["ben", "ana"]);
    let split = draft.to_split();
    assert_eq!(split["type"], "equal");
    // §4.1 sorts the surviving set; two devices building one split from one
    // form must produce one payload.
    assert_eq!(split["among"], serde_json::json!(["ana", "ben"]));
}

#[test]
fn every_kind_names_the_wire_type_the_protocol_reads() {
    for kind in SplitKind::ALL {
        let mut draft = SplitDraft::new(kind);
        draft.toggle("ana");
        assert_eq!(draft.to_split()["type"], kind.wire_type());
    }
}
