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
use splitz_host::{plan_removal, split_without, RemovalBlock, RemovalPlan, WalletBillHost};
use std::collections::HashMap;
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
