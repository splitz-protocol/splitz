//! Whether a refund accounts for a settlement's unexplained part (§6.3).
//!
//! Mirrors `splitz_host/test/refunds_test.dart` case for case.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::json;
use splitz_core::host::FoldedBill;
use splitz_core::{Bill, DirectDebt, Expense, Identities, Settlement};
use splitz_host::refunds_behind;

fn expense(id: &str, paid_by: &str, amount: i64, among: &[&str]) -> Expense {
    Expense {
        id: id.into(),
        description: id.into(),
        paid_by: paid_by.into(),
        amount,
        currency: "USD".into(),
        at: "2026-10-28T19:30:00.000Z".into(),
        split: json!({"type": "equal", "among": among}),
    }
}

fn folded(expenses: Vec<Expense>, authors: &[(&str, &str)]) -> FoldedBill {
    FoldedBill {
        bill: Bill {
            id: "b".into(),
            name: "Trip".into(),
            currency: "USD".into(),
            split_mode: "equal".into(),
            participants: vec![],
            expenses,
            payments: vec![],
            confirmed_payments: BTreeSet::new(),
            rate: None,
        },
        creator_id: "ana".into(),
        set_aside: vec![],
        withdrawn: vec![],
        replaced_addresses: vec![],
        identities: Identities::default(),
        payment_authors: BTreeMap::new(),
        payment_digests: BTreeMap::new(),
        expense_entries: BTreeMap::new(),
        expense_authors: authors
            .iter()
            .map(|(e, a)| ((*e).to_owned(), (*a).to_owned()))
            .collect(),
        payment_entries: BTreeMap::new(),
        rate_entry: None,
        rate_author: None,
        in_force: vec![],
        amendment_of: BTreeMap::new(),
        close_entry: None,
        closed_over: String::new(),
    }
}

fn settlement(amount: i64, covers: Vec<DirectDebt>) -> Settlement {
    Settlement {
        from: "me".into(),
        to: "ben".into(),
        amount,
        covers,
    }
}

#[test]
fn a_refund_that_moves_the_whole_unexplained_part_onto_the_payer_names_it() {
    let f = folded(
        vec![expense("cara:r", "me", -1000, &["me", "ben"])],
        &[("cara:r", "cara")],
    );
    let found = refunds_behind(&settlement(500, vec![]), &f).expect("a refund");
    assert_eq!(found.refunded, 500);
    assert_eq!(found.authors, vec!["cara".to_owned()]);
}

#[test]
fn an_overpayment_is_not_a_refund() {
    let f = folded(vec![expense("me:d", "me", 1000, &["me", "ben"])], &[]);
    assert_eq!(refunds_behind(&settlement(500, vec![]), &f), None);
}

#[test]
fn a_refund_smaller_than_the_unexplained_part_does_not_account_for_it() {
    let f = folded(
        vec![expense("cara:r", "me", -200, &["me", "ben"])],
        &[("cara:r", "cara")],
    );
    assert_eq!(refunds_behind(&settlement(500, vec![]), &f), None);
}

#[test]
fn a_settlement_its_covers_explain_has_nothing_to_account_for() {
    let f = folded(
        vec![expense("cara:r", "me", -1000, &["me", "ben"])],
        &[("cara:r", "cara")],
    );
    let covered = settlement(
        500,
        vec![DirectDebt {
            from: "me".into(),
            to: "ben".into(),
            amount: 500,
        }],
    );
    assert_eq!(refunds_behind(&covered, &f), None);
}
