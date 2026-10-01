//! §10.3 step 5: every expense and payment id is minted by its entry's author.
//!
//! An id is minted by an author when it is that author's participant id, `:`,
//! then anything, and an author whose id holds `:` mints nothing. An entry, or
//! an amendment, whose id its author did not mint is set aside with
//! `id_not_minted`.

use serde_json::{json, Value};
use splitz_core::{code, derive_bill_id, derive_entry_id, fold_log, owns_id, FoldResult};

fn at(minute: u32) -> String {
    format!("2026-10-28T19:{minute:02}:00.000Z")
}

fn sealed(mut entry: Value) -> Value {
    entry["id"] = Value::from(derive_entry_id(&entry).unwrap());
    entry
}

fn create() -> Value {
    let mut e = json!({
        "v": 1, "author": "ana", "kind": "createBill", "at": at(0),
        "name": "Dinner", "currency": "EUR", "splitMode": "equal",
        "creatorKey": "A".repeat(43), "nonce": "A".repeat(22),
    });
    e["id"] = Value::from(derive_bill_id(&e).unwrap());
    e
}

fn join(who: &str, minute: u32) -> Value {
    sealed(json!({
        "v": 1, "author": who, "kind": "joinBill", "at": at(minute),
        "participant": {"id": who, "name": who},
    }))
}

fn expense(author: &str, id: &str, minute: u32) -> Value {
    sealed(json!({
        "v": 1, "author": author, "kind": "addExpense", "at": at(minute),
        "expense": {
            "id": id, "paidBy": author, "amount": 9000, "at": at(minute),
            "split": {"type": "equal", "among": ["ana", "ben"]},
        },
    }))
}

fn payment(author: &str, from: &str, id: &str, amount: i64, minute: u32) -> Value {
    sealed(json!({
        "v": 1, "author": author, "kind": "recordPayment", "at": at(minute),
        "payment": {
            "id": id, "from": from, "to": "ana", "amount": amount,
            "method": "cash", "at": at(minute),
        },
    }))
}

fn amendment(target: &Value, amount: i64) -> Value {
    let mut payload = target["expense"].clone();
    payload["amount"] = json!(amount);
    sealed(json!({
        "v": 1, "author": target["author"], "kind": "amendEntry", "at": at(30),
        "targetId": target["id"], "expense": payload,
    }))
}

fn bill(others: &[&str]) -> Vec<Value> {
    let mut out = vec![create(), join("ana", 1), join("ben", 2)];
    for (i, who) in others.iter().enumerate() {
        out.push(join(who, 3 + i as u32));
    }
    out
}

fn fold(extra: Vec<Value>, others: &[&str]) -> FoldResult {
    let mut entries = bill(others);
    let bill_id = entries[0]["id"].as_str().unwrap().to_owned();
    entries.extend(extra);
    fold_log(&entries, Some(&bill_id)).unwrap()
}

fn ids(r: &FoldResult, member: &str) -> Vec<String> {
    r.bill[member]
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e["id"].as_str().unwrap().to_owned())
        .collect()
}

fn codes(r: &FoldResult) -> Vec<&'static str> {
    r.set_aside.iter().map(|s| s.code).collect()
}

#[test]
fn an_expense_id_its_author_did_not_mint_is_set_aside() {
    let r = fold(vec![expense("ana", "hotel", 10)], &[]);
    assert!(ids(&r, "expenses").is_empty());
    assert_eq!(codes(&r), [code::ID_NOT_MINTED]);
}

#[test]
fn a_payment_id_its_author_did_not_mint_is_set_aside() {
    let r = fold(vec![payment("ben", "ben", "t1", 4500, 10)], &[]);
    assert!(ids(&r, "payments").is_empty());
    assert_eq!(codes(&r), [code::ID_NOT_MINTED]);
}

#[test]
fn a_backdated_copy_of_somebody_elses_id_is_set_aside_and_theirs_stands() {
    let r = fold(
        vec![
            expense("ana", "ana:hotel", 20),
            expense("ben", "ana:hotel", 8),
            payment("ben", "ben", "ben:t1:ana", 4500, 20),
            payment("ana", "ben", "ben:t1:ana", 1, 8),
        ],
        &[],
    );
    assert_eq!(ids(&r, "expenses"), ["ana:hotel"]);
    assert_eq!(ids(&r, "payments"), ["ben:t1:ana"]);
    assert_eq!(r.bill["payments"][0]["amount"], 4500);
    assert_eq!(codes(&r), [code::ID_NOT_MINTED, code::ID_NOT_MINTED]);
}

#[test]
fn the_authors_own_id_is_not_minted_and_nothing_after_the_colon_is() {
    let r = fold(
        vec![expense("ana", "ana", 10), expense("ana", "ana:", 11)],
        &[],
    );
    assert_eq!(ids(&r, "expenses"), ["ana:"]);
    assert_eq!(codes(&r), [code::ID_NOT_MINTED]);
}

#[test]
fn an_id_that_only_begins_with_the_author_id_is_not_minted() {
    let r = fold(vec![expense("ana", "anabel:x", 10)], &[]);
    assert!(ids(&r, "expenses").is_empty());
    assert_eq!(codes(&r), [code::ID_NOT_MINTED]);
}

#[test]
fn an_id_holding_a_colon_cannot_join_so_records_nothing() {
    let r = fold(
        vec![payment("ben:t1", "ben:t1", "ben:t1:own", 1, 10)],
        &["ben:t1"],
    );
    assert!(ids(&r, "payments").is_empty());
    assert!(codes(&r).contains(&code::BILL_BAD_PARTICIPANT_ID));
}

#[test]
fn an_amendment_of_an_unminted_expense_is_set_aside_with_it() {
    let target = expense("ana", "hotel", 10);
    let amend = amendment(&target, 1);
    let r = fold(vec![target.clone(), amend.clone()], &[]);
    assert!(ids(&r, "expenses").is_empty());
    let mut got: Vec<(String, &str)> = r.set_aside.iter().map(|s| (s.id.clone(), s.code)).collect();
    got.sort();
    let mut want = vec![
        (
            amend["id"].as_str().unwrap().to_owned(),
            code::ID_NOT_MINTED,
        ),
        (
            target["id"].as_str().unwrap().to_owned(),
            code::ID_NOT_MINTED,
        ),
    ];
    want.sort();
    assert_eq!(got, want);
}

#[test]
fn minted_ids_apply_and_an_amendment_of_one_applies() {
    let target = expense("ana", "ana:hotel", 10);
    let amend = amendment(&target, 1);
    let r = fold(
        vec![target, amend, payment("ben", "ben", "ben:t1:ana", 4500, 12)],
        &[],
    );
    assert_eq!(ids(&r, "expenses"), ["ana:hotel"]);
    assert_eq!(r.bill["expenses"][0]["amount"], 1);
    assert_eq!(ids(&r, "payments"), ["ben:t1:ana"]);
    assert!(r.set_aside.is_empty());
    assert!(owns_id("ana", "ana:hotel"));
    assert!(!owns_id("ana", "ana"));
    assert!(!owns_id("ben:t1", "ben:t1:ana"));
}
