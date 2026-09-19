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

const SPLIT_MODES: [&str; 2] = ["equal", "percentage"];
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

fn rate_of(value: &Value) -> Result<ExchangeRate> {
    let currency = value
        .get("currency")
        .and_then(Value::as_str)
        .ok_or_else(|| SplitError::new(code::BILL_BAD_CURRENCY, "A rate states a currency"))?;
    check_currency(currency)?;
    Ok(ExchangeRate {
        currency: currency.to_owned(),
        minor_units_per_zec: integer(value.get("minorUnitsPerZec").unwrap_or(&Value::Null))?,
        at: value
            .get("at")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned(),
        source: value
            .get("source")
            .and_then(Value::as_str)
            .map(str::to_owned),
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
    Ok(Payout {
        kind: kind.to_owned(),
        address: value
            .get("address")
            .and_then(Value::as_str)
            .map(str::to_owned),
        asset: value
            .get("asset")
            .and_then(Value::as_str)
            .map(str::to_owned),
        chain: value
            .get("chain")
            .and_then(Value::as_str)
            .map(str::to_owned),
    })
}

/// Decodes a bill document.
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

    let split_mode = match doc.get("splitMode") {
        None | Some(Value::Null) => "equal".to_owned(),
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
        if !ids.insert(id.clone()) {
            return Err(SplitError::new(
                code::DUPLICATE_PARTICIPANT,
                format!("Two participants share the id \"{id}\""),
            ));
        }
        let mut payouts = Vec::new();
        for raw_payout in array(raw, "payouts")? {
            payouts.push(payout_of(raw_payout)?);
        }
        participants.push(Participant {
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
        });
    }

    let mut expenses = Vec::new();
    for raw in array(doc, "expenses")? {
        if !raw.is_object() {
            return Err(type_error("an object"));
        }
        let own = currency_of(raw, &currency)?;
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
        expenses.push(Expense {
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
        });
    }

    let mut payments = Vec::new();
    for raw in array(doc, "payments")? {
        if !raw.is_object() {
            return Err(type_error("an object"));
        }
        let own = currency_of(raw, &currency)?;
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
                let r = rate_of(v)?;
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

        payments.push(PaymentRecord {
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
        });
    }

    let rate = match doc.get("rate") {
        None => None,
        Some(v) => {
            if !v.is_object() {
                return Err(type_error("a rate object"));
            }
            Some(rate_of(v)?)
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
