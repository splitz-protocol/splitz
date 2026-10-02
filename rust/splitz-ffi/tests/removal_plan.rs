//! Taking somebody off a bill through the binding (§10.8): the plan, the
//! check that it still stands, and a split without them.

use serde_json::{json, Value};
use splitz_ffi::{
    add_expense_entry, create_bill_entry, fold_entries, identity_key_from_seed, join_bill_entry,
    plan_removal, same_removal_plan, split_without, void_entry_for, HostFacts, RemovalBlock,
    SplitzError,
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
    assert_eq!(first.edits.len(), 1);
    assert_eq!(first.edits[0].entry_id, id_of(&taxi));
    let split: Value = serde_json::from_str(&first.edits[0].split_json).unwrap();
    assert_eq!(split["among"], json!([ana.me]));
    assert!(same_removal_plan(first.clone(), plan(&entries)).unwrap());

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
    assert!(!same_removal_plan(first, after).unwrap());

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
