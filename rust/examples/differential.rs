//! Answers the differential lane's operation list.
//!
//! Reads one JSON operation per line on stdin and writes one JSON answer per
//! line on stdout. Every implementation answers the same list, and
//! `tools/differential/compare.py` diffs the answers against each other rather
//! than against anybody's expectation.

use serde_json::{json, Value};
use splitz::error::Result;
use std::io::{self, BufRead, Write};

/// Runs `body` and returns its value, or the refusal code that stopped it.
fn attempt(body: impl FnOnce() -> Result<Value>) -> Value {
    match body() {
        Ok(value) => json!({ "ok": value }),
        Err(e) => json!({ "refused": e.code }),
    }
}

fn answer(op: &Value) -> Value {
    match op["op"].as_str().unwrap_or("") {
        "allocate" => attempt(|| {
            let weights: Vec<i64> = op["weights"]
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_i64).collect())
                .unwrap_or_default();
            Ok(json!(splitz::allocate(
                op["total"].as_i64().unwrap_or(0),
                &weights
            )?))
        }),

        "split" => attempt(|| {
            Ok(json!(splitz::split_expense(
                op["total"].as_i64().unwrap_or(0),
                &op["split"]
            )?))
        }),

        "rate" => attempt(|| {
            let rate = splitz::ExchangeRate {
                currency: op["currency"].as_str().unwrap_or("").to_owned(),
                minor_units_per_zec: op["minorUnitsPerZec"].as_i64().unwrap_or(0),
                at: "2026-10-28T19:30:00.000Z".to_owned(),
                source: None,
            };
            let rounding = match op["rounding"].as_str() {
                Some("down") => splitz::RateRounding::Down,
                Some("nearest") => splitz::RateRounding::Nearest,
                _ => splitz::RateRounding::Up,
            };
            Ok(json!(splitz::fiat_to_zatoshi(
                op["minorUnits"].as_i64().unwrap_or(0),
                &rate,
                None,
                rounding
            )?))
        }),

        "amount" => attempt(|| {
            Ok(json!(splitz::render_amount(
                op["zatoshi"].as_i64().unwrap_or(0)
            )?))
        }),

        "qchar" => attempt(|| {
            Ok(json!(splitz::zip321::qchar(
                op["text"].as_str().unwrap_or("")
            )))
        }),

        "instant" => attempt(|| {
            Ok(json!(splitz::instant::canonical_instant(
                op["text"].as_str().unwrap_or("")
            )?))
        }),

        "invite" => attempt(|| {
            let i = splitz::parse_invite(op["uri"].as_str().unwrap_or(""))?;
            Ok(json!({"billId": i.bill_id, "key": i.key, "name": i.name}))
        }),

        "canonical" => attempt(|| Ok(json!(splitz::canonical_json(&op["value"])?))),

        "billid" => attempt(|| Ok(json!(splitz::derive_bill_id(&op["entry"])?))),

        _ => json!({"refused": "unknown_operation"}),
    }
}

fn main() {
    let stdin = io::stdin();
    let mut out = io::BufWriter::new(io::stdout());
    for line in stdin.lock().lines() {
        let line = line.expect("stdin is readable");
        if line.trim().is_empty() {
            continue;
        }
        let op: Value = serde_json::from_str(&line).expect("an operation is JSON");
        let mut result = answer(&op);
        result
            .as_object_mut()
            .expect("an answer is an object")
            .insert("id".into(), op["id"].clone());
        writeln!(
            out,
            "{}",
            serde_json::to_string(&result).expect("an answer encodes")
        )
        .expect("stdout is writable");
    }
}
