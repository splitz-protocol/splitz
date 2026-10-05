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
    bill_key: Option<&str>,
) -> Result<Value> {
    // §2.1: a writer refuses what every reader refuses.
    crate::money::check_currency(currency)?;
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
    // §9.4: the bill key it will be sealed under, so a joiner can tell an
    // invite's key belongs to this bill.
    if let Some(key) = bill_key {
        let digest = crate::invite::bill_key_digest(key).ok_or_else(|| {
            crate::error::SplitError::new(crate::error::code::BILL_TYPE_ERROR, "Not a bill key")
        })?;
        entry.insert("keyDigest".to_owned(), Value::from(digest));
    }
    let value = Value::Object(entry);
    let id = derive_bill_id(&value)?;
    Ok(with_id(value, id))
}

/// Whether `text` holds nothing but Unicode White_Space: the one set every
/// implementation reads as blank. Dart's `trim` also drops U+FEFF and Rust's
/// does not, so two readers of one rule would disagree about one input.
fn blank(text: Option<&Value>) -> bool {
    text.and_then(Value::as_str)
        .is_none_or(|v| v.chars().all(char::is_whitespace))
}

/// Refuses a payout nobody could be paid by (§9.1), with
/// `payout_incomplete`: a `zec` payout with no address, or a `swap` missing
/// its asset, chain or address. A reader takes such a payout as unpayable
/// and sends the payer to the next preference without asking, so it is never
/// written — by a join or by an amendment of one. A type §9.1 does not
/// define is left to every reader, which refuses it
/// (`bill_unknown_payout_method`).
pub fn check_written_payout(payout: &Value) -> Result<()> {
    let blank = |field: &str| blank(payout.get(field));
    let kind = payout.get("type").and_then(Value::as_str).unwrap_or("");
    let fields: &[&str] = match kind {
        "zec" => &["address"],
        "swap" => &["asset", "chain", "address"],
        "cash" => &[],
        // A type §9.1 does not define is every reader's to refuse.
        _ => &[],
    };
    let missing: Vec<&str> = fields.iter().copied().filter(|f| blank(f)).collect();
    if missing.is_empty() {
        Ok(())
    } else {
        Err(crate::error::SplitError::new(
            crate::error::code::PAYOUT_INCOMPLETE,
            format!("A {kind} payout names its {}", missing.join(", ")),
        ))
    }
}

/// Refuses a payment record nobody should write (§9.2), by a record or by an
/// amendment of one: an amount that is not more than nothing
/// (`payment_not_positive`) — while unconfirmed it still withholds the whole
/// debt it names — and a `swap` with no `reference`
/// (`swap_missing_reference`), by which alone either side finds the swap
/// again.
pub fn check_written_payment(payment: &Value) -> Result<()> {
    let amount = payment.get("amount").and_then(Value::as_i64);
    if amount.is_none_or(|a| a <= 0) {
        return Err(crate::error::SplitError::new(
            crate::error::code::PAYMENT_NOT_POSITIVE,
            format!(
                "A payment is more than nothing, got {}",
                payment.get("amount").unwrap_or(&Value::Null)
            ),
        ));
    }
    if payment.get("method").and_then(Value::as_str) == Some("swap")
        && blank(payment.get("reference"))
    {
        return Err(crate::error::SplitError::new(
            crate::error::code::SWAP_MISSING_REFERENCE,
            "A swap payment names its swap in reference",
        ));
    }
    Ok(())
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
    payouts: Option<Vec<Value>>,
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
    // §9.1's own shape, passed through untouched and in the order given:
    // order is the preference order, and a reader that reorders it settles to
    // a different address than the one asked for.
    if let Some(payouts) = payouts {
        for payout in &payouts {
            check_written_payout(payout)?;
        }
        participant.insert("payouts".to_owned(), Value::Array(payouts));
    }
    let mut body = Map::new();
    body.insert("kind".to_owned(), Value::from("joinBill"));
    body.insert("participant".to_owned(), Value::Object(participant));
    sealed(host, body)
}

/// `local` as an id `author` minted: `<author>:<local>`, or `local` itself
/// when it already begins that way.
///
/// §10.3 step 5 keeps such an id for its author whatever `at` another entry
/// states, so every expense and payment written here carries one.
pub fn authored_id(author: &str, local: &str) -> String {
    if local
        .strip_prefix(author)
        .is_some_and(|rest| rest.starts_with(':'))
    {
        local.to_owned()
    } else {
        format!("{author}:{local}")
    }
}

/// Adds an expense. `amount` is minor units of the bill's currency (§2.1).
///
/// The expense's id is `expense_id` under [`authored_id`].
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
    expense.insert(
        "id".to_owned(),
        Value::from(authored_id(host.me(), expense_id)),
    );
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

/// Writes the `addExpense` at `target_id` again as this expense, replacing it
/// (§10.8). `basis` is the id of the amendment applied to the target when it
/// was read, or `None` when none was: the restatement is set aside with
/// `restatement_stale` when the target has changed since, so a correction
/// written meanwhile is kept rather than replaced by a copy that never saw
/// it. Only the target's author or the bill's creator may restate it.
#[allow(clippy::too_many_arguments)]
pub fn restate_expense(
    host: &dyn BillHost,
    target_id: &str,
    basis: Option<&str>,
    expense_id: &str,
    paid_by: &str,
    amount: i64,
    split: Value,
    description: Option<&str>,
) -> Result<Value> {
    let mut expense = Map::new();
    expense.insert(
        "id".to_owned(),
        Value::from(authored_id(host.me(), expense_id)),
    );
    expense.insert("paidBy".to_owned(), Value::from(paid_by));
    expense.insert("amount".to_owned(), Value::from(amount));
    expense.insert("at".to_owned(), Value::from(at(host)?));
    expense.insert("split".to_owned(), split);
    if let Some(description) = description {
        expense.insert("description".to_owned(), Value::from(description));
    }
    let mut body = Map::new();
    body.insert("kind".to_owned(), Value::from("addExpense"));
    body.insert("targetId".to_owned(), Value::from(target_id));
    if let Some(basis) = basis {
        body.insert("basis".to_owned(), Value::from(basis));
    }
    body.insert("expense".to_owned(), Value::Object(expense));
    sealed(host, body)
}

/// Records a payment that was made. It moves no balance until a confirmation
/// settles it (§10.5) — a record is a claim, not a settlement.
///
/// `payment_id` is the transaction id, so the record and the transaction carry
/// one identifier and a reader can check the second from the first. The
/// record's id is `payment_id` under [`authored_id`].
///
/// `amount` is minor units of the bill's currency and is what settles the
/// debt. `zatoshi` and `paid_at_rate` record what actually left the wallet and
/// the rate it was converted at; §9.2 makes both advisory, and neither takes
/// any part in §5 or §6.
///
/// `reference` identifies a `swap` off this chain — the provider's intent id,
/// or the transaction on the destination chain. It is not a Zcash txid, and a
/// reader that renders it as one is wrong for every swap (§9.2).
#[allow(clippy::too_many_arguments)]
pub fn record_payment(
    host: &dyn BillHost,
    payment_id: &str,
    to: &str,
    amount: i64,
    method: &str,
    reference: Option<&str>,
    zatoshi: Option<i64>,
    paid_at_rate: Option<Value>,
    note: Option<&str>,
) -> Result<Value> {
    let mut asked = Map::new();
    asked.insert("amount".to_owned(), Value::from(amount));
    asked.insert("method".to_owned(), Value::from(method));
    if let Some(reference) = reference {
        asked.insert("reference".to_owned(), Value::from(reference));
    }
    check_written_payment(&Value::Object(asked))?;
    let mut payment = Map::new();
    payment.insert(
        "id".to_owned(),
        Value::from(authored_id(host.me(), payment_id)),
    );
    payment.insert("from".to_owned(), Value::from(host.me()));
    payment.insert("to".to_owned(), Value::from(to));
    payment.insert("amount".to_owned(), Value::from(amount));
    payment.insert("method".to_owned(), Value::from(method));
    payment.insert("at".to_owned(), Value::from(at(host)?));
    if let Some(reference) = reference {
        payment.insert("reference".to_owned(), Value::from(reference));
    }
    if let Some(zatoshi) = zatoshi {
        payment.insert("zatoshi".to_owned(), Value::from(zatoshi));
    }
    if let Some(rate) = paid_at_rate {
        payment.insert("paidAtRate".to_owned(), rate);
    }
    if let Some(note) = note {
        payment.insert("note".to_owned(), Value::from(note));
    }
    let mut body = Map::new();
    body.insert("kind".to_owned(), Value::from("recordPayment"));
    body.insert("payment".to_owned(), Value::Object(payment));
    sealed(host, body)
}

/// Confirms a payment. Which methods settle a debt, and who may claim each, is
/// §10.5's decision and nothing here widens it.
///
/// `record` is the digest of the record being confirmed, as the fold reports
/// it in `payment_digests` (§10.5): the confirmation stands only while the
/// bill's record under `payment_id` still says what it said then.
pub fn confirm_payment(
    host: &dyn BillHost,
    payment_id: &str,
    method: &str,
    reference: Option<&str>,
    record: &str,
) -> Result<Value> {
    let mut confirmation = Map::new();
    confirmation.insert("paymentId".to_owned(), Value::from(payment_id));
    confirmation.insert("method".to_owned(), Value::from(method));
    confirmation.insert("record".to_owned(), Value::from(record));
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

/// Corrects an entry by replacing it wholesale (§10.4).
///
/// `payload` is the corrected body, under the member name its kind uses.
/// **An amendment replaces its target entirely**, so a payload that leaves a
/// field out deletes that field rather than keeping it: build it from the
/// current entry, not from the part being changed.
///
/// Only the author of the target may amend it (`unauthorized_entry`), and the
/// payload must be of the target's own kind (`amend_kind_mismatch`).
pub fn amend_entry(
    host: &dyn BillHost,
    target_id: &str,
    member: &str,
    payload: Value,
) -> Result<Value> {
    // What a join or a record may not write, an amendment of one may not
    // either: it replaces the entry wholesale.
    if member == "participant" {
        for payout in payload
            .get("payouts")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            check_written_payout(payout)?;
        }
    }
    if member == "payment" {
        check_written_payment(&payload)?;
    }
    let mut body = Map::new();
    body.insert("kind".to_owned(), Value::from("amendEntry"));
    body.insert("targetId".to_owned(), Value::from(target_id));
    body.insert(member.to_owned(), payload);
    sealed(host, body)
}

/// Corrects the expense `expense_id` on `folded` (§10.4): an amendment of
/// the entry that introduced it, its payload the expense as the bill reads it
/// now with `paid_by`, `amount`, `split` and `description` replacing what they
/// name.
///
/// An amendment replaces its target wholesale, so the payload starts from the
/// expense as applied — after any amendment already standing — and never from
/// the entry as first written: a field a later correction changed would
/// otherwise be changed back. Refused with `unknown_entry` when the bill
/// applies no such expense.
#[allow(clippy::too_many_arguments)]
pub fn amend_expense(
    host: &dyn BillHost,
    folded: &super::bill_log::FoldedBill,
    expense_id: &str,
    paid_by: Option<&str>,
    amount: Option<i64>,
    split: Option<Value>,
    description: Option<&str>,
) -> Result<Value> {
    let expense = folded.bill.expenses.iter().find(|e| e.id == expense_id);
    let target = folded.expense_entries.get(expense_id);
    let (Some(expense), Some(target)) = (expense, target) else {
        return Err(crate::error::SplitError::new(
            crate::error::code::UNKNOWN_ENTRY,
            format!("The bill applies no expense {expense_id}"),
        ));
    };
    let mut payload = crate::serialization::expense_to_json(expense);
    if let Some(paid_by) = paid_by {
        payload["paidBy"] = Value::from(paid_by);
    }
    if let Some(amount) = amount {
        payload["amount"] = Value::from(amount);
    }
    if let Some(split) = split {
        payload["split"] = split;
    }
    if let Some(description) = description {
        payload["description"] = Value::from(description);
    }
    amend_entry(host, target, "expense", payload)
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
///
/// `bill_id` is the bill the entry is written for, and is part of what is
/// signed (§10.6), so the signature does not verify on any other bill. A
/// `createBill` entry's bill is its own id.
pub fn sign_entry(host: &dyn BillHost, entry: &Value, bill_id: &str) -> Result<Value> {
    let Some(sign) = host.signer() else {
        return Ok(entry.clone());
    };
    let signature = sign(signing_message(entry, bill_id)?.as_bytes());
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
