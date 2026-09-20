//! Builds the entries a bill's log is made of.
//!
//! The protocol derives an entry's id and refuses one that does not match
//! (§9.4, §9.5) but exports nothing that assembles an entry, because the shape
//! of an entry is the specification's business and building one is the
//! wallet's. That leaves every wallet to hand-assemble an object and derive a
//! digest, which is a thing to get wrong once per wallet.
//!
//! Every function here returns an entry whose `id` is already the digest the
//! protocol will check. Nothing here writes to a log; that is
//! [`crate::host::BillLog`]'s job.

use serde_json::{Map, Value};

use crate::authority::signing_message;
use crate::error::Result;
use crate::instant::canonical_instant;
use crate::log::{derive_bill_id, derive_entry_id};

use super::host::BillHost;

/// Unpadded base64url, the encoding every key, nonce and id in the protocol
/// uses (§9.4).
///
/// Re-exported rather than written again: a wallet has to produce one for the
/// creator key and the nonce, and two encoders is two places for the alphabet
/// or the padding rule to drift.
pub use crate::zip321::base64url as base64url_no_pad;

/// The version every entry this layer writes carries.
pub const ENTRY_VERSION: i64 = 1;

/// A `createBill`'s `creatorKey` is 32 bytes and its `nonce` is 16, both
/// unpadded base64url — §9.4 refuses an entry whose members are any other
/// length, and the bill id is the digest of the entry that states them.
pub const CREATOR_KEY_BYTES: usize = 32;
pub const NONCE_BYTES: usize = 16;

/// The instant to stamp an entry with, as §9.3 writes one.
fn at(host: &dyn BillHost) -> Result<String> {
    canonical_instant(&host.now())
}

/// Opens a bill. Its id is the digest of this entry (§9.4).
///
/// `creator_key` is the key §10.7 binds the creator by, so the creator needs
/// no prior acquaintance. The nonce makes two bills opened in one second by
/// one person two bills.
pub fn create_bill(
    host: &dyn BillHost,
    name: &str,
    currency: &str,
    split_mode: &str,
    creator_key: &str,
) -> Result<Value> {
    let mut entry = Map::new();
    entry.insert("v".to_owned(), Value::from(ENTRY_VERSION));
    entry.insert("author".to_owned(), Value::from(host.me()));
    entry.insert("kind".to_owned(), Value::from("createBill"));
    entry.insert("at".to_owned(), Value::from(at(host)?));
    entry.insert("name".to_owned(), Value::from(name));
    entry.insert("currency".to_owned(), Value::from(currency));
    entry.insert("splitMode".to_owned(), Value::from(split_mode));
    entry.insert("creatorKey".to_owned(), Value::from(creator_key));
    entry.insert(
        "nonce".to_owned(),
        Value::from(base64url_no_pad(&host.random_bytes(NONCE_BYTES))),
    );
    let value = Value::Object(entry);
    let id = derive_bill_id(&value)?;
    Ok(with_id(value, id))
}

/// Joins a bill, or restates this device's own participant record.
///
/// `identity_key` is a self-claim: §10.7 binds it only when the entry's author
/// is the participant it names and the signature verifies against it. Naming
/// somebody else proves nothing about them.
pub fn join_bill(
    host: &dyn BillHost,
    name: Option<&str>,
    pay_to: Option<&str>,
    identity_key: Option<&str>,
) -> Result<Value> {
    let mut participant = Map::new();
    participant.insert("id".to_owned(), Value::from(host.me()));
    if let Some(name) = name {
        participant.insert("name".to_owned(), Value::from(name));
    }
    if let Some(pay_to) = pay_to {
        participant.insert("payTo".to_owned(), Value::from(pay_to));
    }
    if let Some(key) = identity_key {
        participant.insert("identityKey".to_owned(), Value::from(key));
    }
    let mut body = Map::new();
    body.insert("kind".to_owned(), Value::from("joinBill"));
    body.insert("participant".to_owned(), Value::Object(participant));
    sealed(host, body)
}

/// Adds an expense. `amount` is minor units of the bill's currency (§2.1).
///
/// `split` is §4's own shape and is passed through untouched: nothing here
/// invents a split method the specification does not define.
pub fn add_expense(
    host: &dyn BillHost,
    expense_id: &str,
    paid_by: &str,
    amount: i64,
    split: Value,
    description: Option<&str>,
) -> Result<Value> {
    let mut expense = Map::new();
    expense.insert("id".to_owned(), Value::from(expense_id));
    expense.insert("paidBy".to_owned(), Value::from(paid_by));
    expense.insert("amount".to_owned(), Value::from(amount));
    expense.insert("at".to_owned(), Value::from(at(host)?));
    expense.insert("split".to_owned(), split);
    if let Some(description) = description {
        expense.insert("description".to_owned(), Value::from(description));
    }
    let mut body = Map::new();
    body.insert("kind".to_owned(), Value::from("addExpense"));
    body.insert("expense".to_owned(), Value::Object(expense));
    sealed(host, body)
}

/// Records a payment that was made. It moves no balance until a confirmation
/// settles it (§10.5) — a record is a claim, not a settlement.
///
/// `payment_id` is the transaction id, so the record and the transaction carry
/// one identifier and a reader can check the second from the first.
pub fn record_payment(
    host: &dyn BillHost,
    payment_id: &str,
    to: &str,
    amount: i64,
    method: &str,
) -> Result<Value> {
    let mut payment = Map::new();
    payment.insert("id".to_owned(), Value::from(payment_id));
    payment.insert("from".to_owned(), Value::from(host.me()));
    payment.insert("to".to_owned(), Value::from(to));
    payment.insert("amount".to_owned(), Value::from(amount));
    payment.insert("method".to_owned(), Value::from(method));
    payment.insert("at".to_owned(), Value::from(at(host)?));
    let mut body = Map::new();
    body.insert("kind".to_owned(), Value::from("recordPayment"));
    body.insert("payment".to_owned(), Value::Object(payment));
    sealed(host, body)
}

/// Confirms a payment. Which methods settle a debt, and who may claim each, is
/// §10.5's decision and nothing here widens it.
pub fn confirm_payment(
    host: &dyn BillHost,
    payment_id: &str,
    method: &str,
    reference: Option<&str>,
) -> Result<Value> {
    let mut confirmation = Map::new();
    confirmation.insert("paymentId".to_owned(), Value::from(payment_id));
    confirmation.insert("method".to_owned(), Value::from(method));
    if let Some(reference) = reference {
        confirmation.insert("reference".to_owned(), Value::from(reference));
    }
    let mut body = Map::new();
    body.insert("kind".to_owned(), Value::from("confirmPayment"));
    body.insert("confirmation".to_owned(), Value::Object(confirmation));
    sealed(host, body)
}

/// Snapshots a price onto the bill. §7 makes only `source` optional.
pub fn set_rate(
    host: &dyn BillHost,
    currency: &str,
    minor_units_per_zec: i64,
    source: Option<&str>,
) -> Result<Value> {
    let mut rate = Map::new();
    rate.insert("currency".to_owned(), Value::from(currency));
    rate.insert(
        "minorUnitsPerZec".to_owned(),
        Value::from(minor_units_per_zec),
    );
    rate.insert("at".to_owned(), Value::from(at(host)?));
    if let Some(source) = source {
        rate.insert("source".to_owned(), Value::from(source));
    }
    let mut body = Map::new();
    body.insert("kind".to_owned(), Value::from("setRate"));
    body.insert("rate".to_owned(), Value::Object(rate));
    sealed(host, body)
}

/// Withdraws an entry. Who may is §10.8's decision.
pub fn void_entry(host: &dyn BillHost, target_id: &str) -> Result<Value> {
    let mut body = Map::new();
    body.insert("kind".to_owned(), Value::from("voidEntry"));
    body.insert("targetId".to_owned(), Value::from(target_id));
    sealed(host, body)
}

/// Signs `entry` with the host's signer (§10.6), or returns it unchanged when
/// the host does not sign.
///
/// Signing is a separate step rather than part of each builder because the
/// curve operation is the host's (§13) and may be expensive — a hardware
/// signer, a user prompt — while assembling an entry is neither.
///
/// Safe to call after the id has been derived, and it must be: §9.5's digest
/// covers every member but `id`, `sig` and `v`, so attaching a signature does
/// not move the id, while §10.6's message covers `id` and would be a message
/// about a different entry if it were taken first.
///
/// A host with no signer gets its entry back unsigned rather than an error.
/// §10.7 then binds no key to that author, and a folded bill reports no
/// identity binding rather than claiming one it cannot make.
pub fn sign_entry(host: &dyn BillHost, entry: &Value) -> Result<Value> {
    let Some(sign) = host.signer() else {
        return Ok(entry.clone());
    };
    let signature = sign(signing_message(entry)?.as_bytes());
    let mut object = entry
        .as_object()
        .cloned()
        .unwrap_or_else(serde_json::Map::new);
    object.insert("sig".to_owned(), Value::from(signature));
    Ok(Value::Object(object))
}

/// Fills in the members every entry carries and derives §9.5's id.
///
/// The id is derived last, over the finished entry, because the digest covers
/// every member but `id`, `sig` and `v` — deriving it earlier would digest an
/// entry that is not the one written.
fn sealed(host: &dyn BillHost, body: Map<String, Value>) -> Result<Value> {
    let mut entry = Map::new();
    entry.insert("v".to_owned(), Value::from(ENTRY_VERSION));
    entry.insert("author".to_owned(), Value::from(host.me()));
    entry.insert("at".to_owned(), Value::from(at(host)?));
    for (key, value) in body {
        entry.insert(key, value);
    }
    let value = Value::Object(entry);
    let id = derive_entry_id(&value)?;
    Ok(with_id(value, id))
}

fn with_id(value: Value, id: String) -> Value {
    let mut object = value.as_object().cloned().unwrap_or_else(Map::new);
    object.insert("id".to_owned(), Value::from(id));
    Value::Object(object)
}
