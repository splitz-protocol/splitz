//! What taking somebody off a bill needs first (§10.8).
//!
//! Mirrors `splitz_host/test/removal_plan_test.dart` case for case.

mod support;

use serde_json::{json, Value};
use splitz_core::code;
use splitz_core::host::{
    add_expense, amend_entry, base64url_no_pad, confirm_payment, create_bill, join_bill,
    record_payment, void_entry, BillLog, FoldedBill, CREATOR_KEY_BYTES,
};
use splitz_core::Expense;
use splitz_core::{net_balances, participant_id};
use splitz_host::{
    plan_merge, plan_removal, removal_entries, split_merged, split_without, RemovalBlock,
    RemovalEdit, RemovalPlan, WalletBillHost,
};
use std::collections::{BTreeMap, HashMap};
use support::FakeWallet;

fn fake_key(who: &str) -> String {
    let first = who.as_bytes()[0];
    base64url_no_pad(
        &(0..CREATOR_KEY_BYTES)
            .map(|i| first.wrapping_add(i as u8))
            .collect::<Vec<u8>>(),
    )
}

fn equal(among: &[&str]) -> Value {
    json!({"type": "equal", "among": among})
}

fn id_of(entry: &Value) -> String {
    entry["id"].as_str().unwrap().to_owned()
}

/// One bill's writers on one clock, and the log they write.
struct Bill {
    wallets: HashMap<String, FakeWallet>,
    clock: HashMap<String, usize>,
    log: Vec<Value>,
    joins: HashMap<String, String>,
}

impl Bill {
    fn new() -> Self {
        let mut b = Bill {
            wallets: HashMap::new(),
            clock: HashMap::new(),
            log: Vec::new(),
            joins: HashMap::new(),
        };
        b.write("ana", |h| {
            create_bill(h, "Trip", "USD", "equal", &fake_key("ana"), None).unwrap()
        });
        b.join("ana");
        b
    }

    /// `who` writes one entry. Every entry is a minute after the last,
    /// whoever writes it, so §10.2's order is the order written.
    fn write(&mut self, who: &str, entry: impl FnOnce(&WalletBillHost) -> Value) -> Value {
        let at = self.log.len();
        let wallet = self
            .wallets
            .entry(who.to_owned())
            .or_insert_with(|| FakeWallet::new(who, Some(&format!("u1{who}"))));
        let last = self.clock.insert(who.to_owned(), at).unwrap_or(0);
        for _ in last..at {
            wallet.tick();
        }
        let e = entry(&WalletBillHost::new(wallet));
        self.log.push(e.clone());
        e
    }

    fn join(&mut self, who: &str) {
        let pay_to = format!("u1{who}");
        let e = self.write(who, |h| {
            join_bill(h, Some(who), Some(&pay_to), None, None).unwrap()
        });
        self.joins.insert(who.to_owned(), id_of(&e));
    }

    fn expense(
        &mut self,
        who: &str,
        expense_id: &str,
        paid_by: &str,
        amount: i64,
        split: Value,
        description: Option<&str>,
    ) -> Value {
        self.write(who, |h| {
            add_expense(h, expense_id, paid_by, amount, split, description).unwrap()
        })
    }

    fn amend(&mut self, who: &str, target: &Value, split: Value) -> Value {
        let mut payload = target["expense"].clone();
        payload["split"] = split;
        let target_id = id_of(target);
        self.write(who, |h| {
            amend_entry(h, &target_id, "expense", payload).unwrap()
        })
    }

    fn withdraw(&mut self, who: &str, target_id: &str) -> Value {
        self.write(who, |h| void_entry(h, target_id).unwrap())
    }

    fn held<R>(&self, read: impl FnOnce(&BillLog) -> R) -> R {
        let ana = &self.wallets["ana"];
        let host = WalletBillHost::new(ana);
        let log = BillLog::with_entries(&host, self.log.clone()).for_bill(id_of(&self.log[0]));
        read(&log)
    }

    fn fold(&self) -> FoldedBill {
        self.held(|log| log.fold().expect("the log folds"))
    }

    /// `id`'s removal as `me` plans it, over the log in §10.2's order.
    fn plan(&self, id: &str, me: &str) -> RemovalPlan {
        let ordered = self.held(|log| log.entries());
        plan_removal(&self.fold(), "ana", &ordered, id, me)
    }

    /// Writes `plan` as `me` would: each expense again under its new split,
    /// and the one it replaces withdrawn.
    fn restate(&mut self, plan: &RemovalPlan, me: &str) {
        for (n, edit) in plan.edits.iter().enumerate() {
            let seen = edit.seen.clone();
            let split = edit.split.clone();
            self.write(me, |h| {
                add_expense(
                    h,
                    &format!("restated-{n}"),
                    &seen.paid_by,
                    seen.amount,
                    split,
                    (!seen.description.is_empty()).then_some(seen.description.as_str()),
                )
                .unwrap()
            });
            self.withdraw(me, &edit.entry_id);
        }
    }
}

/// A taxi of 30.00 Ana paid, split by Ana, Ben and Cai.
fn taxi() -> (Bill, Value) {
    let mut b = Bill::new();
    b.join("ben");
    b.join("cai");
    let taxi = b.expense(
        "ana",
        "taxi",
        "ana",
        3000,
        equal(&["ana", "ben", "cai"]),
        Some("Taxi"),
    );
    (b, taxi)
}

/// A boat of 40.00 Cai wrote and paid, shared by Ana, Ben and Cai.
fn boat() -> (Bill, Value) {
    let mut b = Bill::new();
    b.join("ben");
    b.join("cai");
    let boat = b.expense(
        "cai",
        "boat",
        "cai",
        4000,
        equal(&["ana", "ben", "cai"]),
        Some("Boat"),
    );
    (b, boat)
}

// --- a split without somebody ----------------------------------------------

#[test]
fn equal_and_shares_drop_them_the_rest_share_it() {
    assert_eq!(
        split_without(&equal(&["ana", "ben", "cai"]), "ben"),
        Some(equal(&["ana", "cai"]))
    );
    assert_eq!(
        split_without(
            &json!({"type": "shares", "shareCounts": {"ana": 1, "ben": 2}}),
            "ben"
        ),
        Some(json!({"type": "shares", "shareCounts": {"ana": 1}}))
    );
}

#[test]
fn nobody_left_or_a_figure_that_must_still_add_up_is_by_hand() {
    assert_eq!(split_without(&equal(&["ben"]), "ben"), None);
    assert_eq!(
        split_without(
            &json!({"type": "exact", "amounts": {"ana": 500, "ben": 500}}),
            "ben"
        ),
        None
    );
    assert_eq!(
        split_without(
            &json!({"type": "percentage", "basisPoints": {"ana": 5000, "ben": 5000}}),
            "ben"
        ),
        None
    );
}

#[test]
fn itemized_drops_them_per_item_an_item_only_they_had_is_by_hand() {
    let itemized = |shared_by: &[&str]| {
        json!({
            "type": "itemized",
            "extraMinorUnits": 0,
            "items": [{"description": "pizza", "minorUnits": 1000, "sharedBy": shared_by}],
        })
    };
    let left = split_without(&itemized(&["ana", "ben"]), "ben").unwrap();
    assert_eq!(left["items"][0]["sharedBy"], json!(["ana"]));
    assert_eq!(split_without(&itemized(&["ben"]), "ben"), None);
}

#[test]
fn zero_shares_left_is_by_hand_some_left_is_not() {
    assert_eq!(
        split_without(
            &json!({"type": "shares", "shareCounts": {"ana": 0, "ben": 2, "cai": 0}}),
            "ben"
        ),
        None
    );
    assert_eq!(
        split_without(
            &json!({"type": "shares", "shareCounts": {"ana": 0, "ben": 2, "cai": 1}}),
            "ben"
        ),
        Some(json!({"type": "shares", "shareCounts": {"ana": 0, "cai": 1}}))
    );
}

// --- the plan ----------------------------------------------------------------

#[test]
fn off_an_expense_this_device_wrote_then_off_the_bill() {
    let (mut b, taxi) = taxi();
    let plan = b.plan("ben", "ana");
    assert!(plan.blockers.is_empty());
    assert_eq!(plan.edits.len(), 1);
    assert_eq!(plan.edits[0].entry_id, id_of(&taxi));
    assert_eq!(plan.edits[0].split, equal(&["ana", "cai"]));

    b.restate(&plan, "ana");
    let ben = b.joins["ben"].clone();
    b.withdraw("ana", &ben);
    let folded = b.fold();
    assert!(folded.set_aside.is_empty());
    assert!(folded.bill.participants.iter().all(|p| p.id != "ben"));
    let now = &folded.bill.expenses[0];
    assert_eq!(folded.bill.expenses.len(), 1);
    assert_eq!(
        (now.paid_by.as_str(), now.amount, now.description.as_str()),
        ("ana", 3000, "Taxi")
    );
    assert_eq!(now.split["among"], json!(["ana", "cai"]));
    assert!(!b.plan("ben", "ana").names_them());
}

#[test]
fn what_they_paid_for_is_said_not_done() {
    let mut b = Bill::new();
    b.join("ben");
    let hotel = b.expense(
        "ben",
        "hotel",
        "ben",
        2000,
        equal(&["ana", "ben"]),
        Some("Hotel"),
    );
    let plan = b.plan("ben", "ana");
    assert!(plan.edits.is_empty());
    assert_eq!(plan.blockers.len(), 1);
    let blocker = &plan.blockers[0];
    assert_eq!(blocker.block, RemovalBlock::PaidFor);
    assert_eq!(blocker.entry_id, id_of(&hotel));
    assert_eq!(blocker.description, "Hotel");
}

#[test]
fn the_creator_or_the_author_restates_anybody_else_is_told_whose() {
    let (mut b, boat) = boat();
    b.join("dee");
    assert!(!b.plan("ben", "cai").edits.is_empty());
    let other = b.plan("ben", "dee");
    assert!(other.edits.is_empty());
    assert_eq!(other.blockers.len(), 1);
    let blocker = &other.blockers[0];
    assert_eq!(blocker.block, RemovalBlock::AddedByAnother);
    assert_eq!(
        (blocker.description.as_str(), blocker.author.as_deref()),
        ("Boat", Some("cai"))
    );

    let plan = b.plan("ben", "ana");
    assert!(plan.blockers.is_empty());
    assert_eq!(plan.edits.len(), 1);
    assert_eq!(plan.edits[0].entry_id, id_of(&boat));
    assert_eq!(plan.edits[0].author.as_deref(), Some("cai"));
    b.restate(&plan, "ana");
    let ben = b.joins["ben"].clone();
    b.withdraw("ana", &ben);
    let folded = b.fold();
    assert!(folded.set_aside.is_empty());
    assert!(folded.bill.participants.iter().all(|p| p.id != "ben"));
    let now = &folded.bill.expenses[0];
    assert_eq!(
        (now.paid_by.as_str(), now.amount, now.description.as_str()),
        ("cai", 4000, "Boat")
    );
    assert_eq!(now.split["among"], json!(["ana", "cai"]));
}

#[test]
fn an_amendment_adding_them_names_them_too() {
    let (mut b, boat) = boat();
    b.join("dee");
    b.amend("cai", &boat, equal(&["ana", "ben", "cai", "dee"]));
    let plan = b.plan("dee", "ana");
    assert!(plan.names_them());
    assert_eq!(plan.edits.len(), 1);
    assert_eq!(plan.edits[0].seen.description, "Boat");
    assert_eq!(plan.edits[0].split, equal(&["ana", "ben", "cai"]));
}

#[test]
fn named_only_by_the_entry_an_amendment_corrects_offered_as_it_reads_now_and_they_come_off() {
    let (mut b, taxi) = taxi();
    b.amend("ana", &taxi, equal(&["ana", "cai"]));
    let plan = b.plan("ben", "ana");
    assert!(plan.names_them());
    assert_eq!(plan.edits.len(), 1);
    assert_eq!(plan.edits[0].split, equal(&["ana", "cai"]));
    b.restate(&plan, "ana");
    let ben = b.joins["ben"].clone();
    b.withdraw("ana", &ben);
    let folded = b.fold();
    assert!(folded.set_aside.is_empty());
    assert!(folded.bill.participants.iter().all(|p| p.id != "ben"));
    assert_eq!(folded.bill.expenses[0].amount, 3000);
}

#[test]
fn somebody_on_nothing_is_on_nothing() {
    let (mut b, _) = taxi();
    b.join("dee");
    assert!(!b.plan("dee", "ana").names_them());
}

#[test]
fn two_copies_of_one_expense_are_one_restatement() {
    let (mut b, taxi) = taxi();
    let mut copy = taxi.clone();
    copy["sig"] = json!("BBBB");
    b.log.push(copy);
    let plan = b.plan("ben", "ana");
    assert_eq!(plan.edits.len(), 1);
    b.restate(&plan, "ana");
    let folded = b.fold();
    assert_eq!(folded.bill.expenses.len(), 1);
    assert_eq!(folded.bill.expenses[0].amount, 3000);
}

fn withdrawn_amendment(only_naming: bool) {
    let mut b = Bill::new();
    b.join("cai");
    b.join("dee");
    let taxi = b.expense(
        "ana",
        "taxi",
        "ana",
        3000,
        equal(&["ana", "cai"]),
        Some("Taxi"),
    );
    let mut corrections = vec![vec!["ana", "cai", "dee"]];
    if !only_naming {
        corrections.push(vec!["ana", "cai"]);
    }
    let mut last = None;
    for among in corrections {
        last = Some(b.amend("ana", &taxi, equal(&among)));
    }
    b.withdraw("ana", &id_of(&last.unwrap()));
    assert!(!b.plan("dee", "ana").names_them());
    let dee = b.joins["dee"].clone();
    b.withdraw("ana", &dee);
    let folded = b.fold();
    assert!(folded.set_aside.is_empty());
    assert!(folded.bill.participants.iter().all(|p| p.id != "dee"));
}

#[test]
fn an_earlier_amendment_does_not_stand_in_for_a_withdrawn_one() {
    withdrawn_amendment(false);
}

#[test]
fn a_withdrawn_amendment_naming_them_does_not_count() {
    withdrawn_amendment(true);
}

#[test]
fn shares_where_only_they_held_one_are_by_hand() {
    let mut b = Bill::new();
    b.join("ben");
    let ticket = b.expense(
        "ana",
        "ticket",
        "ana",
        3000,
        json!({"type": "shares", "shareCounts": {"ben": 1, "ana": 0}}),
        Some("Ben’s ticket"),
    );
    let plan = b.plan("ben", "ana");
    assert!(plan.edits.is_empty());
    assert_eq!(plan.blockers.len(), 1);
    let blocker = &plan.blockers[0];
    assert_eq!(blocker.block, RemovalBlock::SplitByHand);
    assert_eq!(
        (blocker.entry_id.as_str(), blocker.description.as_str()),
        (id_of(&ticket).as_str(), "Ben’s ticket")
    );
}

#[test]
fn an_expense_the_fold_sets_aside_still_names_them() {
    let mut b = Bill::new();
    b.join("ben");
    let wrong = b.expense(
        "ana",
        "wrong",
        "ana",
        3000,
        json!({"type": "exact", "amounts": {"ana": 1000, "ben": 1000}}),
        Some("Dinner"),
    );
    assert!(b.fold().set_aside.iter().any(|s| s.id == id_of(&wrong)));
    let plan = b.plan("ben", "ana");
    assert_eq!(plan.blockers.len(), 1);
    let blocker = &plan.blockers[0];
    assert_eq!(blocker.block, RemovalBlock::Unapplied);
    assert_eq!(
        (blocker.entry_id.as_str(), blocker.description.as_str()),
        (id_of(&wrong).as_str(), "Dinner")
    );
}

#[test]
fn a_payment_from_them_to_them_and_a_confirmation_by_them() {
    let mut b = Bill::new();
    b.join("ben");
    b.join("cai");
    b.expense(
        "ana",
        "taxi",
        "ana",
        3000,
        equal(&["ana", "ben", "cai"]),
        None,
    );
    let paid = b.write("ben", |h| {
        record_payment(h, "p1", "ana", 1000, "cash", None, None, None, None).unwrap()
    });
    let received = b.write("cai", |h| {
        record_payment(h, "p2", "ben", 1, "cash", None, None, None, None).unwrap()
    });
    let payment = received["payment"]["id"].as_str().unwrap().to_owned();
    let record = b.fold().payment_digests[&payment].clone();
    let confirmed = b.write("ben", |h| {
        confirm_payment(h, &payment, "cash", None, &record).unwrap()
    });
    let blockers: Vec<(RemovalBlock, String, bool)> = b
        .plan("ben", "ana")
        .blockers
        .into_iter()
        .map(|x| (x.block, x.entry_id, x.from_them))
        .collect();
    assert_eq!(
        blockers,
        vec![
            (RemovalBlock::Payment, id_of(&paid), true),
            (RemovalBlock::Payment, id_of(&received), false),
            (RemovalBlock::Confirmation, id_of(&confirmed), false),
        ]
    );
}

// --- the same plan -------------------------------------------------------------

#[test]
fn the_same_plan_read_twice_is_the_same() {
    let (b, _) = taxi();
    assert!(b.plan("ben", "ana").same_as(&b.plan("ben", "ana")));
}

#[test]
fn the_same_plan_once_written_is_not_the_plan_any_more() {
    let (mut b, _) = taxi();
    let plan = b.plan("ben", "ana");
    b.restate(&plan, "ana");
    assert!(!b.plan("ben", "ana").names_them());
    assert!(!b.plan("ben", "ana").same_as(&plan));
}

#[test]
fn the_same_plan_with_a_correction_synced_in_is_not_the_plan_any_more() {
    let (mut b, boat) = boat();
    let plan = b.plan("ben", "ana");
    b.join("dee");
    b.amend("cai", &boat, equal(&["ana", "ben", "cai", "dee"]));
    let now = b.plan("ben", "ana");
    assert!(!now.same_as(&plan));
    assert_eq!(now.edits[0].split, equal(&["ana", "cai", "dee"]));
}

#[test]
fn the_same_plan_held_back_by_something_else_is_not_the_plan_any_more() {
    let (mut b, _) = taxi();
    let plan = b.plan("ben", "ana");
    b.write("ben", |h| {
        record_payment(h, "p1", "ana", 1000, "cash", None, None, None, None).unwrap()
    });
    assert!(!b.plan("ben", "ana").same_as(&plan));
}

#[test]
fn the_protocol_refuses_the_removal_the_plan_says_is_held_back() {
    // The fold's own §10.8 check agrees with the plan: a removal planned as
    // blocked is set aside with participant_still_named.
    let mut b = Bill::new();
    b.join("ben");
    b.expense("ben", "hotel", "ben", 2000, equal(&["ana", "ben"]), None);
    assert!(b.plan("ben", "ana").names_them());
    let ben = b.joins["ben"].clone();
    let removal = b.withdraw("ana", &ben);
    let folded = b.fold();
    let aside: Vec<_> = folded
        .set_aside
        .iter()
        .filter(|s| s.id == id_of(&removal))
        .collect();
    assert_eq!(aside.len(), 1);
    assert_eq!(aside[0].code, code::PARTICIPANT_STILL_NAMED);
}

#[test]
fn every_join_still_stating_them_is_listed_and_one_left_keeps_them_on() {
    let (mut b, _) = taxi();
    b.join("dee");
    let first = b.joins["dee"].clone();
    // Changing how Dee is paid restates her record in a second join.
    b.join("dee");
    let second = b.joins["dee"].clone();
    assert_ne!(first, second);

    let plan = b.plan("dee", "ana");
    assert!(!plan.names_them());
    assert_eq!(plan.joins, vec![first.clone(), second.clone()]);

    // One withdrawn, one standing: she is still on the bill, and the plan now
    // lists only the one left.
    b.withdraw("ana", &first);
    assert!(b.fold().bill.participants.iter().any(|p| p.id == "dee"));
    assert_eq!(b.plan("dee", "ana").joins, vec![second.clone()]);

    b.withdraw("ana", &second);
    assert!(b.fold().bill.participants.iter().all(|p| p.id != "dee"));
    assert!(b.plan("dee", "ana").joins.is_empty());
}

#[test]
fn a_plan_whose_joins_changed_no_longer_stands() {
    let (mut b, _) = taxi();
    b.join("dee");
    let before = b.plan("dee", "ana");
    b.join("dee");
    assert!(!before.same_as(&b.plan("dee", "ana")));
    assert!(b.plan("dee", "ana").same_as(&b.plan("dee", "ana")));
}

// Whole or not at all, and what a removal moves.

#[test]
fn only_shared_expenses_name_them_the_plan_takes_them_off() {
    let (b, _) = taxi();
    assert!(b.plan("ben", "ana").complete());
}

#[test]
fn something_they_paid_for_keeps_them_on_though_the_shared_ones_are_listed() {
    let (mut b, _) = taxi();
    b.expense(
        "ben",
        "hotel",
        "ben",
        2000,
        equal(&["ana", "ben"]),
        Some("Hotel"),
    );
    let plan = b.plan("ben", "ana");
    assert_eq!(plan.edits.len(), 1);
    assert_eq!(plan.blockers[0].block, RemovalBlock::PaidFor);
    assert!(!plan.complete());
}

#[test]
fn somebody_on_nothing_is_complete_with_nothing_to_write() {
    let mut b = Bill::new();
    b.join("ben");
    let plan = b.plan("ben", "ana");
    assert!(!plan.names_them());
    assert!(plan.complete());
    assert!(plan.share_changes().unwrap().is_empty());
}

fn changes(pairs: &[(&str, i64)]) -> BTreeMap<String, i64> {
    pairs.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect()
}

#[test]
fn an_even_split_their_share_shared_by_the_rest() {
    // 30.00 among three is 10.00 each; among two, 15.00.
    let (b, _) = taxi();
    assert_eq!(
        b.plan("ben", "ana").share_changes().unwrap(),
        changes(&[("ana", 500), ("ben", -1000), ("cai", 500)])
    );
}

#[test]
fn a_split_with_a_remainder_takes_exactly_their_share_and_sums_to_zero() {
    // 10.00 among three is 3.34, 3.33, 3.33 (§3, the extra cent to the first
    // id); among two, 5.00 each.
    let mut b = Bill::new();
    b.join("ben");
    b.join("cai");
    b.expense(
        "ana",
        "cab",
        "ana",
        1000,
        equal(&["ana", "ben", "cai"]),
        None,
    );
    let moved = b.plan("ben", "ana").share_changes().unwrap();
    assert_eq!(moved, changes(&[("ana", 166), ("ben", -333), ("cai", 167)]));
    assert_eq!(moved.values().sum::<i64>(), 0);
}

#[test]
fn shares_the_rest_take_it_in_proportion() {
    // 40.00 in shares 2:1:1 is 20.00, 10.00, 10.00; without Ben, 2:1 is
    // 26.67 and 13.33.
    let mut b = Bill::new();
    b.join("ben");
    b.join("cai");
    b.expense(
        "ana",
        "villa",
        "ana",
        4000,
        json!({"type": "shares", "shareCounts": {"ana": 2, "ben": 1, "cai": 1}}),
        None,
    );
    assert_eq!(
        b.plan("ben", "ana").share_changes().unwrap(),
        changes(&[("ana", 667), ("ben", -1000), ("cai", 333)])
    );
}

#[test]
fn several_expenses_add_up_per_person() {
    let (mut b, _) = taxi();
    b.expense(
        "ana",
        "cab",
        "ana",
        1000,
        equal(&["ana", "ben", "cai"]),
        None,
    );
    // 500 + 166 for Ana, 500 + 167 for Cai, 1000 + 333 off Ben.
    assert_eq!(
        b.plan("ben", "ana").share_changes().unwrap(),
        changes(&[("ana", 666), ("ben", -1333), ("cai", 667)])
    );
}

#[test]
fn what_is_not_restated_moves_nothing() {
    // Ben paid for the hotel: it stays as it is, and only the taxi moves.
    let (mut b, _) = taxi();
    b.expense("ben", "hotel", "ben", 2000, equal(&["ana", "ben"]), None);
    assert_eq!(
        b.plan("ben", "ana").share_changes().unwrap(),
        changes(&[("ana", 500), ("ben", -1000), ("cai", 500)])
    );
}

#[test]
fn a_running_total_past_the_bound_is_refused_not_wrapped() {
    // Two halves of the largest amount still fit; three do not.
    let edit = |n: usize| RemovalEdit {
        entry_id: format!("e{n}"),
        seen: Expense {
            id: format!("x{n}"),
            description: String::new(),
            paid_by: "x".into(),
            amount: i64::MAX,
            currency: "USD".into(),
            at: "2026-10-05T00:00:00Z".into(),
            split: equal(&["x", "y"]),
        },
        author: Some("x".into()),
        split: equal(&["x"]),
        basis: None,
        paid_by: None,
    };
    let of = |n: usize| RemovalPlan {
        edits: (0..n).map(edit).collect(),
        blockers: vec![],
        joins: vec![],
        may_withdraw_joins: true,
    };
    assert_eq!(
        of(2).share_changes().unwrap(),
        changes(&[
            ("x", 9_223_372_036_854_775_806),
            ("y", -9_223_372_036_854_775_806)
        ])
    );
    assert_eq!(
        of(3).share_changes().unwrap_err().code,
        code::AMOUNT_OVERFLOW
    );
}

// --- restating in one write (§10.8) -----------------------------------------

impl Bill {
    /// What `me` writes to take `id` off, computed from the log as it stands
    /// and not yet added to it: the entries one device would push.
    fn removal(&mut self, id: &str, me: &str) -> Vec<Value> {
        let plan = self.plan(id, me);
        let wallet = &self.wallets[me];
        for _ in 0..60 {
            wallet.tick();
        }
        removal_entries(&WalletBillHost::new(wallet), &plan).expect("a complete plan")
    }
}

fn total(f: &FoldedBill) -> i64 {
    f.bill.expenses.iter().map(|e| e.amount).sum()
}

#[test]
fn an_honest_removal_takes_them_off_in_one_write() {
    let (mut b, _) = taxi();
    let written = b.removal("cai", "ana");
    b.log.extend(written);
    let f = b.fold();
    assert!(f.bill.participants.iter().all(|p| p.id != "cai"));
    assert!(f.set_aside.is_empty());
    assert_eq!(total(&f), 3000);
}

#[test]
fn a_member_the_split_type_does_not_read_is_taken_out_too() {
    let mut b = Bill::new();
    b.join("ben");
    b.join("cai");
    b.expense(
        "ana",
        "snacks",
        "ana",
        400,
        json!({"type": "equal", "among": ["ana", "ben"], "amounts": {"cai": 1}}),
        None,
    );
    let plan = b.plan("cai", "ana");
    assert!(plan.complete());
    assert_eq!(plan.edits[0].split["amounts"], json!({}));
    let written = b.removal("cai", "ana");
    b.log.extend(written);
    let f = b.fold();
    assert!(f.bill.participants.iter().all(|p| p.id != "cai"));
    assert!(f.set_aside.is_empty());
}

#[test]
fn one_creator_on_two_devices_restating_at_once_leaves_one_expense() {
    let (mut b, _) = taxi();
    let phone = b.removal("cai", "ana");
    let tablet = b.removal("cai", "ana");
    assert_ne!(phone, tablet);
    b.log.extend(phone);
    b.log.extend(tablet);
    let f = b.fold();
    assert_eq!(f.bill.expenses.len(), 1);
    assert_eq!(total(&f), 3000);
    assert!(f.bill.participants.iter().all(|p| p.id != "cai"));
    let codes: Vec<&str> = f.set_aside.iter().map(|s| s.code).collect();
    assert!(codes.contains(&code::RESTATEMENT_SUPERSEDED));
}

#[test]
fn a_correction_written_meanwhile_is_kept_and_they_stay_until_planned_again() {
    let (mut b, boat) = boat();
    let removal = b.removal("ben", "ana");
    let mut corrected = boat["expense"].clone();
    corrected["amount"] = json!(5000);
    let target = id_of(&boat);
    b.write("cai", |h| {
        amend_entry(h, &target, "expense", corrected).unwrap()
    });
    b.log.extend(removal);
    let f = b.fold();
    assert_eq!(total(&f), 5000);
    assert!(f.bill.participants.iter().any(|p| p.id == "ben"));
    let codes: Vec<&str> = f.set_aside.iter().map(|s| s.code).collect();
    assert!(codes.contains(&code::RESTATEMENT_STALE));
    assert!(codes.contains(&code::PARTICIPANT_STILL_NAMED));
    let again = b.removal("ben", "ana");
    b.log.extend(again);
    let done = b.fold();
    assert_eq!(total(&done), 5000);
    assert!(done.bill.participants.iter().all(|p| p.id != "ben"));
}

#[test]
fn entries_for_a_plan_that_is_not_complete_are_refused() {
    let (b, _) = boat();
    // Cai paid for the boat: nobody can take Cai off while it stands.
    let plan = b.plan("cai", "ana");
    assert!(!plan.complete());
    let ana = &b.wallets["ana"];
    assert_eq!(
        removal_entries(&WalletBillHost::new(ana), &plan)
            .unwrap_err()
            .code,
        code::PARTICIPANT_STILL_NAMED
    );
    // Ben may not withdraw Cai's join, whatever names them.
    let (b, _) = taxi();
    let plan = b.plan("cai", "ben");
    assert!(plan.blockers.is_empty() || !plan.complete());
    assert!(!plan.may_withdraw_joins);
    let ben = &b.wallets["ben"];
    assert_eq!(
        removal_entries(&WalletBillHost::new(ben), &plan)
            .unwrap_err()
            .code,
        code::UNAUTHORIZED_ENTRY
    );
    // The person themselves may leave.
    assert!(b.plan("cai", "cai").may_withdraw_joins);
}

// --- merging somebody added before they joined ------------------------------

/// Ana added "josh" herself; Jo joined from his own device. Josh paid a
/// 30.00 dinner split by Ana, Josh and Cai, and shares a 20.00 taxi Ana paid
/// with Ana.
fn josh_and_jo() -> Bill {
    let mut b = Bill::new();
    b.join("cai");
    b.join("jo");
    b.write("josh", |h| {
        join_bill(h, Some("Josh"), None, None, None).unwrap()
    });
    b.expense(
        "ana",
        "dinner",
        "josh",
        3000,
        equal(&["ana", "cai", "josh"]),
        None,
    );
    b.expense("ana", "taxi", "ana", 2000, equal(&["ana", "josh"]), None);
    b
}

fn merge(b: &Bill, me: &str, from: &str) -> splitz_core::Result<RemovalPlan> {
    let ordered = b.held(|log| log.entries());
    plan_merge(&b.fold(), "ana", &ordered, from, "jo", me)
}

#[test]
fn a_merge_names_jo_in_his_place_as_payer_too_and_the_fold_applies_it() {
    let mut b = josh_and_jo();
    let before = net_balances(&b.fold().bill).unwrap();
    let plan = merge(&b, "ana", "josh").unwrap();
    assert!(plan.complete());
    let payers: Vec<_> = plan.edits.iter().map(|e| e.paid_by.clone()).collect();
    assert_eq!(payers, vec![Some("jo".to_owned()), None]);
    let written = {
        let ana = &b.wallets["ana"];
        removal_entries(&WalletBillHost::new(ana), &plan).unwrap()
    };
    for e in written {
        b.write("ana", |_| e);
    }
    let folded = b.fold();
    assert!(folded.set_aside.is_empty(), "{:?}", folded.set_aside);
    assert!(folded.bill.participant("josh").is_none());
    let after = net_balances(&folded.bill).unwrap();
    assert_eq!(after["jo"], before["josh"] + before["jo"]);
    assert_eq!(after["ana"], before["ana"]);
    assert_eq!(after["cai"], before["cai"]);
}

#[test]
fn merged_figures_add_onto_his_so_every_other_figure_stays() {
    assert_eq!(
        split_merged(
            &json!({"type": "exact", "amounts": {"ana": 500, "josh": 300, "jo": 200}}),
            "josh",
            "jo"
        )
        .unwrap(),
        Some(json!({"type": "exact", "amounts": {"ana": 500, "jo": 500}}))
    );
    assert_eq!(
        split_merged(
            &json!({"type": "shares", "shareCounts": {"ana": 1, "josh": 2}}),
            "josh",
            "jo"
        )
        .unwrap(),
        Some(json!({"type": "shares", "shareCounts": {"ana": 1, "jo": 2}}))
    );
    let over = split_merged(
        &json!({"type": "exact", "amounts": {"josh": i64::MAX, "jo": 1}}),
        "josh",
        "jo",
    );
    assert_eq!(over.unwrap_err().code, code::AMOUNT_OVERFLOW);
}

#[test]
fn a_list_already_naming_both_is_by_hand() {
    assert_eq!(
        split_merged(&equal(&["jo", "josh"]), "josh", "jo").unwrap(),
        None
    );
    let mut b = josh_and_jo();
    b.expense(
        "ana",
        "boat",
        "ana",
        900,
        equal(&["ana", "jo", "josh"]),
        None,
    );
    let plan = merge(&b, "ana", "josh").unwrap();
    assert!(!plan.complete());
    assert_eq!(plan.blockers.len(), 1);
    assert_eq!(plan.blockers[0].block, RemovalBlock::SplitByHand);
}

#[test]
fn a_payment_to_him_holds_the_merge_back() {
    let mut b = josh_and_jo();
    b.write("cai", |h| {
        record_payment(h, "p1", "josh", 1000, "cash", None, None, None, None).unwrap()
    });
    let plan = merge(&b, "ana", "josh").unwrap();
    assert_eq!(plan.blockers.len(), 1);
    assert_eq!(plan.blockers[0].block, RemovalBlock::Payment);
}

#[test]
fn only_the_creator_merges() {
    let b = josh_and_jo();
    let plan = merge(&b, "cai", "josh").unwrap();
    assert!(!plan.may_withdraw_joins);
    assert!(!plan.complete());
    let cai = &b.wallets["cai"];
    assert_eq!(
        removal_entries(&WalletBillHost::new(cai), &plan)
            .unwrap_err()
            .code,
        code::UNAUTHORIZED_ENTRY
    );
}

#[test]
fn somebody_with_a_key_nobody_or_one_person_into_themselves_is_never_merged() {
    let mut b = josh_and_jo();
    let key = fake_key("kim");
    let kim = participant_id(&key).unwrap();
    b.write(&kim, |h| {
        join_bill(h, Some("Kim"), None, Some(&key), None).unwrap()
    });
    assert_eq!(
        b.fold()
            .bill
            .participant(&kim)
            .unwrap()
            .identity_key
            .as_deref(),
        Some(key.as_str())
    );
    assert_eq!(
        merge(&b, "ana", &kim).unwrap_err().code,
        code::UNAUTHORIZED_ENTRY
    );
    assert_eq!(
        merge(&b, "ana", "nobody").unwrap_err().code,
        code::UNKNOWN_PARTICIPANT
    );
    assert_eq!(
        merge(&b, "ana", "jo").unwrap_err().code,
        code::UNKNOWN_PARTICIPANT
    );
    assert!(merge(&b, "ana", "josh").unwrap().complete());
}

// --- a merge that moves a third person is by hand ---------------------------

/// Josh, added by Ana, merged into Bo over one dinner split `split`. §3 gives
/// leftover units by id and by largest remainder, so moving Josh's name or
/// figure onto Bo can carry a unit across Cai or Ana: such a merge is
/// blocked, and one that moves nobody writes Bo exactly Josh's and Bo's sum.
fn merge_josh_into_bo(split: Value, amount: i64, whole: bool) {
    let mut b = Bill::new();
    b.join("bo");
    b.join("cai");
    b.write("josh", |h| {
        join_bill(h, Some("Josh"), None, None, None).unwrap()
    });
    b.expense("ana", "dinner", "ana", amount, split, None);
    let before = net_balances(&b.fold().bill).unwrap();
    let ordered = b.held(|log| log.entries());
    let plan = plan_merge(&b.fold(), "ana", &ordered, "josh", "bo", "ana").unwrap();
    if !whole {
        assert!(!plan.complete());
        assert_eq!(plan.blockers.len(), 1);
        assert_eq!(plan.blockers[0].block, RemovalBlock::SplitByHand);
        return;
    }
    assert!(plan.complete());
    let written = removal_entries(&WalletBillHost::new(&b.wallets["ana"]), &plan).unwrap();
    for e in written {
        b.write("ana", |_| e);
    }
    let folded = b.fold();
    assert!(folded.set_aside.is_empty(), "{:?}", folded.set_aside);
    let after = net_balances(&folded.bill).unwrap();
    assert_eq!(after["bo"], before["josh"] + before["bo"]);
    assert_eq!(after["ana"], before["ana"]);
    assert_eq!(after.get("cai"), before.get("cai"));
}

#[test]
fn a_merge_where_no_leftover_crosses_anybody_is_whole() {
    merge_josh_into_bo(equal(&["ana", "cai", "josh"]), 100, true);
}

#[test]
fn a_merge_carrying_a_leftover_unit_across_cai_is_by_hand() {
    merge_josh_into_bo(equal(&["ana", "cai", "josh"]), 200, false);
}

#[test]
fn a_merge_moving_the_largest_remainder_is_by_hand() {
    merge_josh_into_bo(
        json!({"type": "percentage", "basisPoints": {"ana": 3334, "bo": 3333, "josh": 3333}}),
        100,
        false,
    );
}

#[test]
fn a_merge_moving_a_leftover_share_is_by_hand() {
    merge_josh_into_bo(
        json!({"type": "shares", "shareCounts": {"ana": 1, "cai": 1, "josh": 1}}),
        200,
        false,
    );
}

#[test]
fn a_merge_of_exact_figures_moves_nobody_and_is_whole() {
    merge_josh_into_bo(
        json!({"type": "exact", "amounts": {"ana": 67, "cai": 67, "josh": 66}}),
        200,
        true,
    );
}
