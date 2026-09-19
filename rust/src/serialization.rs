//! Decoding a bill (SPEC.md §9).
//!
//! A field of the wrong type is refused, optional or not: a wrong-typed
//! `payTo` is the address money is sent to, and an `extraMinorUnits` silently
//! defaulted to zero makes §4.5's total check pass on a bill whose tax has
//! vanished.

use serde_json::Value;
use std::collections::BTreeSet;

use crate::error::{code, Result, SplitError};
use crate::instant::canonical_instant;
use crate::model::{Bill, Expense, Participant, PaymentRecord, Payout};
use crate::money::check_currency;
use crate::rate::ExchangeRate;

/// The wire format version this crate writes and the highest it reads.
pub const BILL_VERSION: i64 = 1;

pub const SPLIT_MODES: [&str; 2] = ["equal", "percentage"];
const PAYOUT_TYPES: [&str; 3] = ["zec", "swap", "cash"];
const SETTLEMENT_METHODS: [&str; 3] = ["shieldedZec", "swap", "cash"];

fn type_error(what: &str) -> SplitError {
    SplitError::new(code::BILL_TYPE_ERROR, format!("Expected {what}"))
}

fn string(value: &Value) -> Result<String> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| type_error("a string"))
}

fn integer(value: &Value) -> Result<i64> {
    // A float is not an integer, and `as_i64` already refuses one.
    value.as_i64().ok_or_else(|| type_error("an integer"))
}

fn array<'a>(value: &'a Value, field: &str) -> Result<&'a Vec<Value>> {
    match value.get(field) {
        None | Some(Value::Null) => Ok(EMPTY.get_or_init(Vec::new)),
        Some(Value::Array(items)) => Ok(items),
        Some(_) => Err(type_error("a list")),
    }
}

static EMPTY: std::sync::OnceLock<Vec<Value>> = std::sync::OnceLock::new();

/// Every payload carrying an amount states its own currency, so it decodes on
/// its own. A reader falls back to the enclosing bill's when absent (§9.1).
fn currency_of(payload: &Value, fallback: &str) -> Result<String> {
    match payload.get("currency") {
        None => Ok(fallback.to_owned()),
        Some(value) => {
            let own = value.as_str().ok_or_else(|| {
                SplitError::new(code::BILL_BAD_CURRENCY, "A currency is a string")
            })?;
            check_currency(own)?;
            if own != fallback {
                return Err(SplitError::new(
                    code::CURRENCY_MISMATCH,
                    format!("An amount in {own} is on a bill denominated in {fallback}"),
                ));
            }
            Ok(own.to_owned())
        }
    }
}

/// Decodes one exchange rate (§7).
///
/// Shared with the fold, which applies it to each `setRate` before the rate
/// reaches a bill document: a member the decoder would refuse sets its entry
/// aside (§10.3) rather than making the whole document undecodable.
///
/// §7 makes only `source` optional, so `at` is required here. A rate with no
/// instant prices an expense at a moment nobody stated.
pub fn decode_rate(value: &Value) -> Result<ExchangeRate> {
    if !value.is_object() {
        return Err(type_error("an object"));
    }
    let currency = value
        .get("currency")
        .and_then(Value::as_str)
        .ok_or_else(|| SplitError::new(code::BILL_BAD_CURRENCY, "A rate states a currency"))?;
    check_currency(currency)?;
    let per = integer(value.get("minorUnitsPerZec").unwrap_or(&Value::Null))?;
    if per <= 0 {
        return Err(SplitError::new(
            code::RATE_NOT_POSITIVE,
            format!("A rate of {per} is not positive"),
        ));
    }
    Ok(ExchangeRate {
        currency: currency.to_owned(),
        minor_units_per_zec: per,
        at: canonical_instant(
            value
                .get("at")
                .and_then(Value::as_str)
                .ok_or_else(|| type_error("an instant"))?,
        )?,
        source: match value.get("source") {
            None => None,
            Some(v) => Some(string(v)?),
        },
    })
}

fn payout_of(value: &Value) -> Result<Payout> {
    let kind = value.get("type").and_then(Value::as_str);
    // Refused rather than skipped: skipping settles to the next preference
    // down, which is a different address.
    let kind = match kind {
        Some(k) if PAYOUT_TYPES.contains(&k) => k,
        other => {
            return Err(SplitError::new(
                code::BILL_UNKNOWN_PAYOUT_METHOD,
                format!("No such payout method: {other:?}"),
            ))
        }
    };
    // §9: a field of the wrong type is refused, optional or not. These name
    // the address money is sent to.
    let member = |name: &str| -> Result<Option<String>> {
        match value.get(name) {
            None => Ok(None),
            Some(v) => Ok(Some(string(v)?)),
        }
    };
    Ok(Payout {
        kind: kind.to_owned(),
        address: member("address")?,
        asset: member("asset")?,
        chain: member("chain")?,
    })
}

/// Decodes a bill document.
/// Decodes one participant payload (§9.1).
///
/// Shared with the fold, which applies it to each `joinBill` before the
/// participant reaches a bill document: a member the decoder would refuse must
/// set its entry aside (§10.3), never make the whole document undecodable.
pub fn decode_participant(raw: &Value) -> Result<Participant> {
    if !raw.is_object() {
        return Err(type_error("an object"));
    }
    let id = string(raw.get("id").unwrap_or(&Value::Null))?;
    // §9.1. An empty id is not a name anyone can be settled to.
    if id.is_empty() {
        return Err(SplitError::new(
            code::BILL_BAD_PARTICIPANT_ID,
            "A participant states an id",
        ));
    }
    let mut payouts = Vec::new();
    for raw_payout in array(raw, "payouts")? {
        payouts.push(payout_of(raw_payout)?);
    }
    Ok(Participant {
        id,
        name: match raw.get("name") {
            None => String::new(),
            Some(v) => string(v)?,
        },
        pay_to: match raw.get("payTo") {
            None => None,
            Some(v) => Some(string(v)?),
        },
        identity_key: match raw.get("identityKey") {
            None => None,
            Some(v) => Some(string(v)?),
        },
        payouts,
    })
}

/// Decodes one expense payload (§9.1), against the ids already on the bill.
pub fn decode_expense(raw: &Value, currency: &str, ids: &BTreeSet<String>) -> Result<Expense> {
    if !raw.is_object() {
        return Err(type_error("an object"));
    }
    let own = currency_of(raw, currency)?;
    let paid_by = raw.get("paidBy").and_then(Value::as_str);
    let paid_by = match paid_by {
        Some(id) if ids.contains(id) => id.to_owned(),
        other => {
            return Err(SplitError::new(
                code::UNKNOWN_PARTICIPANT,
                format!("An expense is paid by {other:?}, who is not on this bill"),
            ))
        }
    };
    let split = raw.get("split").cloned().unwrap_or(Value::Null);
    if !split.is_object() {
        return Err(type_error("a split"));
    }
    // Every id a split names must be on the bill, not just `paidBy`. Without
    // this an expense splitting to a stranger is decoded, folded and kept, and
    // the refusal surfaces from `net_balances` on a bill that already looks
    // whole - section 10.8 rests on the fold being unable to apply such an
    // entry.
    for id in crate::split::split_participants(&split) {
        if !ids.contains(&id) {
            return Err(SplitError::new(
                code::UNKNOWN_PARTICIPANT,
                format!("An expense splits to {id}, who is not on this bill"),
            ));
        }
    }
    Ok(Expense {
        id: string(raw.get("id").unwrap_or(&Value::Null))?,
        description: match raw.get("description") {
            None => String::new(),
            Some(v) => string(v)?,
        },
        paid_by,
        amount: integer(raw.get("amount").unwrap_or(&Value::Null))?,
        currency: own,
        at: canonical_instant(
            raw.get("at")
                .and_then(Value::as_str)
                .ok_or_else(|| type_error("an instant"))?,
        )?,
        split,
    })
}

/// Decodes one payment payload (§9.2), against the ids already on the bill.
pub fn decode_payment(
    raw: &Value,
    currency: &str,
    ids: &BTreeSet<String>,
) -> Result<PaymentRecord> {
    if !raw.is_object() {
        return Err(type_error("an object"));
    }
    let own = currency_of(raw, currency)?;
    let from = raw.get("from").and_then(Value::as_str);
    let to = raw.get("to").and_then(Value::as_str);
    let (from, to) = match (from, to) {
        (Some(f), Some(t)) if ids.contains(f) && ids.contains(t) => (f, t),
        _ => {
            return Err(SplitError::new(
                code::UNKNOWN_PARTICIPANT,
                "A payment names somebody who is not on this bill",
            ))
        }
    };
    if from == to {
        return Err(SplitError::new(
            code::SELF_PAYMENT,
            format!("{from} cannot pay themselves"),
        ));
    }
    let method = match raw.get("method").and_then(Value::as_str) {
        Some(m) if SETTLEMENT_METHODS.contains(&m) => m.to_owned(),
        other => {
            return Err(SplitError::new(
                code::BILL_UNKNOWN_SETTLEMENT_METHOD,
                format!("No such settlement method: {other:?}"),
            ))
        }
    };
    let amount = integer(raw.get("amount").unwrap_or(&Value::Null))?;
    if amount < 0 {
        return Err(SplitError::new(
            code::NEGATIVE_AMOUNT,
            format!("A payment of {amount} is negative"),
        ));
    }

    let zatoshi = match raw.get("zatoshi") {
        None => None,
        Some(v) => {
            let z = integer(v)?;
            if z <= 0 {
                return Err(SplitError::new(
                    code::NEGATIVE_AMOUNT,
                    format!("A payment sends more than nothing, got {z}"),
                ));
            }
            Some(z)
        }
    };

    let paid_at_rate = match raw.get("paidAtRate") {
        None => None,
        Some(v) => {
            if !v.is_object() {
                return Err(type_error("a rate object"));
            }
            let r = decode_rate(v)?;
            // Checked against the currency the payment states, never one
            // it inherited: a rate in another currency restates the debt
            // at an unrelated number rather than pricing it.
            if r.currency != own {
                return Err(SplitError::new(
                    code::RATE_CURRENCY_MISMATCH,
                    format!("A rate in {} does not price a payment in {own}", r.currency),
                ));
            }
            Some(r)
        }
    };

    Ok(PaymentRecord {
        id: string(raw.get("id").unwrap_or(&Value::Null))?,
        from: from.to_owned(),
        to: to.to_owned(),
        amount,
        currency: own,
        method,
        at: canonical_instant(
            raw.get("at")
                .and_then(Value::as_str)
                .ok_or_else(|| type_error("an instant"))?,
        )?,
        zatoshi,
        paid_at_rate,
        reference: match raw.get("reference") {
            None => None,
            Some(v) => Some(string(v)?),
        },
        note: match raw.get("note") {
            None => None,
            Some(v) => Some(string(v)?),
        },
    })
}

pub fn decode_bill(doc: &Value) -> Result<Bill> {
    if !doc.is_object() {
        return Err(type_error("an object"));
    }

    // A version written as a string is not a version: a reader that skips the
    // check when the type is wrong lets a future format present itself as this
    // one.
    let version = match doc.get("v") {
        None => {
            return Err(SplitError::new(
                code::BILL_MISSING_VERSION,
                "A bill states its format version",
            ))
        }
        Some(v) => v.as_i64().ok_or_else(|| {
            SplitError::new(code::BILL_MISSING_VERSION, "A version is an integer")
        })?,
    };
    if version < 1 {
        return Err(SplitError::new(
            code::BILL_TYPE_ERROR,
            format!("A version is at least 1, got {version}"),
        ));
    }
    if version > BILL_VERSION {
        return Err(SplitError::new(
            code::BILL_FUTURE_VERSION,
            format!("This bill is version {version}; this reader implements {BILL_VERSION}"),
        ));
    }

    let currency = match doc.get("currency") {
        None | Some(Value::Null) => {
            return Err(SplitError::new(
                code::BILL_MISSING_CURRENCY,
                "A bill states its currency",
            ))
        }
        Some(Value::String(s)) if s.is_empty() => {
            return Err(SplitError::new(
                code::BILL_MISSING_CURRENCY,
                "A bill states its currency",
            ))
        }
        Some(v) => {
            let s = v.as_str().ok_or_else(|| {
                SplitError::new(code::BILL_BAD_CURRENCY, "A currency is a string")
            })?;
            check_currency(s)?;
            s.to_owned()
        }
    };

    // §9.1. An optional scalar does not read `null` as absent.
    let split_mode = match doc.get("splitMode") {
        None => "equal".to_owned(),
        Some(v) => {
            let s = v.as_str().ok_or_else(|| type_error("a string"))?;
            if !SPLIT_MODES.contains(&s) {
                return Err(SplitError::new(
                    code::BILL_UNKNOWN_SPLIT_MODE,
                    format!("No such split mode: \"{s}\""),
                ));
            }
            s.to_owned()
        }
    };

    let mut participants = Vec::new();
    let mut ids: BTreeSet<String> = BTreeSet::new();
    for raw in array(doc, "participants")? {
        let participant = decode_participant(raw)?;
        if !ids.insert(participant.id.clone()) {
            return Err(SplitError::new(
                code::DUPLICATE_PARTICIPANT,
                format!("Two participants share the id \"{}\"", participant.id),
            ));
        }
        participants.push(participant);
    }

    let mut expenses = Vec::new();
    for raw in array(doc, "expenses")? {
        expenses.push(decode_expense(raw, &currency, &ids)?);
    }

    let mut payments = Vec::new();
    for raw in array(doc, "payments")? {
        payments.push(decode_payment(raw, &currency, &ids)?);
    }

    let rate = match doc.get("rate") {
        None => None,
        Some(v) => {
            if !v.is_object() {
                return Err(type_error("a rate object"));
            }
            Some(decode_rate(v)?)
        }
    };

    let mut confirmed_payments = BTreeSet::new();
    match doc.get("confirmedPayments") {
        None | Some(Value::Null) => {}
        Some(Value::Array(ids)) => {
            for id in ids {
                match id.as_str() {
                    Some(id) => {
                        confirmed_payments.insert(id.to_owned());
                    }
                    None => return Err(type_error("a confirmed payment id")),
                }
            }
        }
        Some(_) => return Err(type_error("a list of confirmed payment ids")),
    }

    Ok(Bill {
        id: match doc.get("id") {
            None => String::new(),
            Some(v) => string(v)?,
        },
        name: match doc.get("name") {
            None => String::new(),
            Some(v) => string(v)?,
        },
        currency,
        split_mode,
        participants,
        expenses,
        payments,
        // §9.1. Absent means nothing is confirmed, never everything:
        // reading it the other way settles a debt on the debtor's own
        // unconfirmed claim.
        confirmed_payments,
        rate,
    })
}
