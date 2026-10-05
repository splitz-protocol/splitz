//! Taking somebody off a bill through the binding (§10.8): the plan, the
//! check that it still stands, and a split without them.

use serde_json::{json, Value};
use splitz_ffi::{
    add_expense_entry, create_bill_entry, fold_entries, identity_key_from_seed, join_bill_entry,
    plan_removal, removal_entries, removal_share_changes, same_removal_plan, split_without,
    void_entry_for, HostFacts, RemovalBlock, RemovalPlanStanding, ShareChange, SplitzError,
};

struct Device {
    seed: String,
    key: String,
    me: String,
}

impl Device {
    fn new(byte: u8) -> Self {
        let seed = splitz_host::base64url_encode(&[byte; 32]);
        let key = identity_key_from_seed(seed.clone()).unwrap();
        let me = splitz_ffi::participant_id_for_key(key.clone()).unwrap();
        Self { seed, key, me }
    }

    fn facts(&self, minute: u32) -> HostFacts {
        HostFacts {
            me: self.me.clone(),
            now: format!("2026-10-28T19:{minute:02}:00.000Z"),
            nonce: vec![self.seed.as_bytes()[0]; 16],
        }
    }

    fn join(&self, minute: u32, bill_id: &str) -> String {
        join_bill_entry(
            self.facts(minute),
            bill_id.to_owned(),
            Some(self.me.clone()),
            Some(format!("u1{}", &self.me[..8])),
            Some(self.key.clone()),
            vec![],
            self.seed.clone(),
        )
        .unwrap()
    }

    fn expense(&self, minute: u32, bill_id: &str, local: &str, among: &[&str]) -> String {
        add_expense_entry(
            self.facts(minute),
            bill_id.to_owned(),
            local.to_owned(),
            self.me.clone(),
            3000,
            json!({"type": "equal", "among": among}).to_string(),
            Some(local.to_owned()),
            self.seed.clone(),
        )
        .unwrap()
    }
}

fn id_of(entry: &str) -> String {
    serde_json::from_str::<Value>(entry).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned()
}

/// Ana's bill with Ana and Ben on it: three entries.
fn bill() -> (Device, Device, String, Vec<String>) {
    let ana = Device::new(1);
    let ben = Device::new(90);
    let create = create_bill_entry(
        ana.facts(1),
        "Dinner".into(),
        "EUR".into(),
        "equal".into(),
        ana.key.clone(),
        None,
        ana.seed.clone(),
    )
    .unwrap();
    let bill_id = id_of(&create);
    let entries = vec![create, ana.join(2, &bill_id), ben.join(3, &bill_id)];
    (ana, ben, bill_id, entries)
}

#[test]
fn the_creator_is_offered_their_expense_and_the_plan_holds_until_written() {
    let (ana, ben, bill_id, mut entries) = bill();
    let taxi = ana.expense(4, &bill_id, "taxi", &[&ana.me, &ben.me]);
    entries.push(taxi.clone());
    let plan = |entries: &[String]| {
        plan_removal(
            ana.facts(9),
            bill_id.clone(),
            entries.to_vec(),
            ben.me.clone(),
            ana.me.clone(),
        )
        .unwrap()
    };
    let first = plan(&entries);
    assert!(first.blockers.is_empty());
    assert!(first.complete);
    // 30.00 between two is 15.00 each; Ana alone takes it all.
    let mut moved = vec![
        ShareChange {
            participant_id: ana.me.clone(),
            minor_units: 1500,
        },
        ShareChange {
            participant_id: ben.me.clone(),
            minor_units: -1500,
        },
    ];
    moved.sort_by(|a, b| a.participant_id.cmp(&b.participant_id));
    assert_eq!(removal_share_changes(first.clone()).unwrap(), moved);
    assert_eq!(first.edits.len(), 1);
    assert_eq!(first.edits[0].entry_id, id_of(&taxi));
    let split: Value = serde_json::from_str(&first.edits[0].split_json).unwrap();
    assert_eq!(split["among"], json!([ana.me]));
    assert_eq!(
        same_removal_plan(first.clone(), plan(&entries)).unwrap(),
        RemovalPlanStanding::Stands
    );

    entries.push(
        add_expense_entry(
            ana.facts(5),
            bill_id.clone(),
            "taxi-again".into(),
            first.edits[0].seen.paid_by.clone(),
            first.edits[0].seen.amount,
            first.edits[0].split_json.clone(),
            Some(first.edits[0].seen.description.clone()),
            ana.seed.clone(),
        )
        .unwrap(),
    );
    entries.push(
        void_entry_for(
            ana.facts(6),
            bill_id.clone(),
            first.edits[0].entry_id.clone(),
            ana.seed.clone(),
        )
        .unwrap(),
    );
    let after = plan(&entries);
    assert!(after.edits.is_empty() && after.blockers.is_empty());
    assert_eq!(
        same_removal_plan(first, after).unwrap(),
        RemovalPlanStanding::Changed
    );

    let join = id_of(&entries[2]);
    entries.push(void_entry_for(ana.facts(7), bill_id.clone(), join, ana.seed.clone()).unwrap());
    let folded = fold_entries(ana.facts(9), bill_id, entries).unwrap();
    assert!(folded.set_aside.is_empty(), "{:?}", folded.set_aside);
    assert!(folded.bill.participants.iter().all(|p| p.id != ben.me));
}

#[test]
fn what_they_paid_for_is_a_blocker() {
    let (ana, ben, bill_id, mut entries) = bill();
    let hotel = ben.expense(4, &bill_id, "hotel", &[&ana.me, &ben.me]);
    entries.push(hotel.clone());
    let plan = plan_removal(ana.facts(9), bill_id, entries, ben.me.clone(), ana.me).unwrap();
    assert!(plan.edits.is_empty());
    assert!(!plan.complete);
    assert_eq!(removal_share_changes(plan.clone()).unwrap(), vec![]);
    assert_eq!(plan.blockers.len(), 1);
    assert_eq!(plan.blockers[0].block, RemovalBlock::PaidFor);
    assert_eq!(plan.blockers[0].entry_id, id_of(&hotel));
    assert_eq!(plan.blockers[0].description, "hotel");
}

#[test]
fn a_split_without_them_crosses_as_json() {
    let left = split_without(
        json!({"type": "equal", "among": ["ana", "ben"]}).to_string(),
        "ben".into(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(&left).unwrap(),
        json!({"type": "equal", "among": ["ana"]})
    );
    assert_eq!(
        split_without(
            json!({"type": "exact", "amounts": {"ana": 1, "ben": 1}}).to_string(),
            "ben".into()
        )
        .unwrap(),
        None
    );
    assert!(matches!(
        split_without("{".into(), "ben".into()),
        Err(SplitzError::Host { .. })
    ));
}

#[test]
fn every_join_is_listed_the_creator_is_the_folds_and_a_refusal_is_asked_first() {
    let (ana, ben, bill_id, mut entries) = bill();
    let first_join = id_of(&entries[2]);
    // Ben restates how he is paid: a second join.
    let again = ben.join(4, &bill_id);
    entries.push(again.clone());
    let taxi = ana.expense(5, &bill_id, "taxi", &[&ana.me, &ben.me]);
    entries.push(taxi.clone());

    let folded = splitz_ffi::fold_entries(ana.facts(9), bill_id.clone(), entries.clone()).unwrap();
    assert_eq!(folded.creator_id, ana.me);

    let plan = plan_removal(
        ana.facts(9),
        bill_id.clone(),
        entries.clone(),
        ben.me.clone(),
        ana.me.clone(),
    )
    .unwrap();
    assert_eq!(plan.joins, vec![first_join.clone(), id_of(&again)]);

    // Taking Ben off while the taxi names him is refused before it is written,
    // and Ana withdrawing her own taxi is not.
    let off = void_entry_for(ana.facts(10), bill_id.clone(), first_join, ana.seed.clone()).unwrap();
    assert_eq!(
        splitz_ffi::entry_refusal(ana.facts(10), bill_id.clone(), entries.clone(), off)
            .unwrap()
            .as_deref(),
        Some("participant_still_named")
    );
    let own = void_entry_for(
        ana.facts(10),
        bill_id.clone(),
        id_of(&taxi),
        ana.seed.clone(),
    )
    .unwrap();
    assert_eq!(
        splitz_ffi::entry_refusal(ana.facts(10), bill_id, entries, own).unwrap(),
        None
    );
}

#[test]
fn share_changes_refuse_a_plan_that_does_not_split() {
    let (ana, ben, bill_id, mut entries) = bill();
    entries.push(ana.expense(4, &bill_id, "taxi", &[&ana.me, &ben.me]));
    let plan = plan_removal(ana.facts(9), bill_id, entries, ben.me.clone(), ana.me).unwrap();
    let with = |split: &str| {
        let mut p = plan.clone();
        p.edits[0].split_json = split.to_owned();
        removal_share_changes(p)
    };
    assert!(with(&plan.edits[0].split_json).is_ok());
    assert!(matches!(
        with(r#"{"type":"equal","among":[]}"#),
        Err(SplitzError::Protocol { code, .. }) if code == "empty_split"
    ));
    assert!(matches!(with("{"), Err(SplitzError::Host { .. })));
}

#[test]
fn removal_entries_write_it_whole_and_two_devices_leave_one_expense() {
    let (ana, ben, bill_id, mut entries) = bill();
    let cai = Device::new(150);
    entries.push(cai.join(4, &bill_id));
    entries.push(ana.expense(5, &bill_id, "taxi", &[&ana.me, &ben.me, &cai.me]));
    let plan = plan_removal(
        ana.facts(9),
        bill_id.clone(),
        entries.clone(),
        cai.me.clone(),
        ana.me.clone(),
    )
    .unwrap();
    assert!(plan.complete && plan.may_withdraw_joins);
    // One creator, two devices, two instants, one log read by both.
    let phone = removal_entries(
        ana.facts(10),
        bill_id.clone(),
        plan.clone(),
        ana.seed.clone(),
    )
    .unwrap();
    let tablet = removal_entries(ana.facts(11), bill_id.clone(), plan, ana.seed.clone()).unwrap();
    assert_eq!(phone.len(), 2);
    entries.extend(phone);
    entries.extend(tablet);
    let folded = fold_entries(ana.facts(12), bill_id, entries).unwrap();
    assert_eq!(folded.bill.expenses.len(), 1);
    assert_eq!(folded.bill.expenses[0].amount, 3000);
    assert!(folded.bill.participants.iter().all(|p| p.id != cai.me));
    let codes: Vec<&str> = folded.set_aside.iter().map(|s| s.code.as_str()).collect();
    assert!(codes.contains(&"restatement_superseded"), "{codes:?}");
}

#[test]
fn removal_entries_refuse_a_plan_that_is_not_complete() {
    let (ana, ben, bill_id, mut entries) = bill();
    let cai = Device::new(150);
    entries.push(cai.join(4, &bill_id));
    entries.push(ana.expense(5, &bill_id, "taxi", &[&ana.me, &ben.me, &cai.me]));
    // Ben neither opened the bill nor is Cai.
    let by_ben = plan_removal(
        ben.facts(9),
        bill_id.clone(),
        entries.clone(),
        cai.me.clone(),
        ben.me.clone(),
    )
    .unwrap();
    assert!(!by_ben.may_withdraw_joins && !by_ben.complete);
    match removal_entries(ben.facts(10), bill_id.clone(), by_ben, ben.seed.clone()) {
        Err(SplitzError::Protocol { code, .. }) => assert_eq!(code, "unauthorized_entry"),
        other => panic!("{other:?}"),
    }
    // Ana paid for the taxi: nobody takes Ana off while it stands.
    let ana_off = plan_removal(
        ben.facts(9),
        bill_id.clone(),
        entries,
        ana.me.clone(),
        ana.me.clone(),
    )
    .unwrap();
    assert!(!ana_off.blockers.is_empty());
    match removal_entries(ana.facts(10), bill_id, ana_off, ana.seed.clone()) {
        Err(SplitzError::Protocol { code, .. }) => assert_eq!(code, "participant_still_named"),
        other => panic!("{other:?}"),
    }
}
