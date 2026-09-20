//! Answers the differential lane's operation list.
//!
//! Reads one JSON operation per line on stdin and writes one JSON answer per
//! line on stdout. Every implementation answers the same list, and
//! `tools/differential/compare.py` diffs the answers against each other rather
//! than against anybody's expectation.

use serde_json::{json, Value};
use splitz_core::error::Result;
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
            Ok(json!(splitz_core::allocate(
                op["total"].as_i64().unwrap_or(0),
                &weights
            )?))
        }),

        "split" => attempt(|| {
            Ok(json!(splitz_core::split_expense(
                op["total"].as_i64().unwrap_or(0),
                &op["split"]
            )?))
        }),

        "rate" => attempt(|| {
            let rate = splitz_core::ExchangeRate {
                currency: op["currency"].as_str().unwrap_or("").to_owned(),
                minor_units_per_zec: op["minorUnitsPerZec"].as_i64().unwrap_or(0),
                at: "2026-10-28T19:30:00.000Z".to_owned(),
                source: None,
            };
            let rounding = match op["rounding"].as_str() {
                Some("down") => splitz_core::RateRounding::Down,
                Some("nearest") => splitz_core::RateRounding::Nearest,
                _ => splitz_core::RateRounding::Up,
            };
            Ok(json!(splitz_core::fiat_to_zatoshi(
                op["minorUnits"].as_i64().unwrap_or(0),
                &rate,
                None,
                rounding
            )?))
        }),

        "amount" => attempt(|| {
            Ok(json!(splitz_core::render_amount(
                op["zatoshi"].as_i64().unwrap_or(0)
            )?))
        }),

        "qchar" => attempt(|| {
            Ok(json!(splitz_core::zip321::qchar(
                op["text"].as_str().unwrap_or("")
            )))
        }),

        "instant" => attempt(|| {
            Ok(json!(splitz_core::instant::canonical_instant(
                op["text"].as_str().unwrap_or("")
            )?))
        }),

        "invite" => attempt(|| {
            let i = splitz_core::parse_invite(op["uri"].as_str().unwrap_or(""))?;
            Ok(json!({"billId": i.bill_id, "key": i.key, "name": i.name,
                      "expiry": i.expiry}))
        }),

        "canonical" => attempt(|| Ok(json!(splitz_core::canonical_json(&op["value"])?))),

        "request" => attempt(|| {
            let payments: Vec<splitz_core::Zip321Payment> = op["payments"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|p| splitz_core::Zip321Payment {
                            address: p["address"].as_str().unwrap_or("").to_owned(),
                            zatoshi: p["zatoshi"].as_i64().unwrap_or(0),
                            fiat: p["fiat"].as_array().map(|f| splitz_core::FiatPrice {
                                currency: f[0].as_str().unwrap_or("").to_owned(),
                                minor_units: f[1].as_i64().unwrap_or(0),
                            }),
                            memo: p["memo"].as_str().map(|m| m.as_bytes().to_vec()),
                            label: p["label"].as_str().map(str::to_owned),
                            message: p["message"].as_str().map(str::to_owned),
                        })
                        .collect()
                })
                .unwrap_or_default();
            Ok(json!(splitz_core::render_uri(
                &payments,
                op["includeFiat"].as_bool().unwrap_or(false)
            )?))
        }),

        "fold" => attempt(|| {
            let log = op["log"].as_array().cloned().unwrap_or_default();
            let r = splitz_core::fold_log(&log, None)?;
            // The bill goes through the decoder: a fold that returns a
            // document its own decoder refuses is the defect this op exists
            // to catch, and it must show as a divergence rather than a crash.
            splitz_core::decode_bill(&r.bill)?;
            Ok(json!({
                "bill": r.bill,
                "setAside": r.set_aside.iter()
                    .map(|a| json!({"id": a.id, "code": a.code}))
                    .collect::<Vec<_>>(),
                "withdrawn": r.withdrawn,
            }))
        }),

        "merge" => attempt(|| {
            let parts: Vec<Vec<Value>> = op["parts"]
                .as_array()
                .map(|a| {
                    a.iter()
                        .map(|p| p.as_array().cloned().unwrap_or_default())
                        .collect()
                })
                .unwrap_or_default();
            let r = splitz_core::merge_logs(&parts)?;
            Ok(json!({
                // The entries themselves: §10.2 rule 2 decides which copy
                // under one id survives, and an id list is the same either way.
                "merged": r.merged,
                "refused": r.refused.iter()
                    .map(|a| json!({"id": a.id, "code": a.code}))
                    .collect::<Vec<_>>(),
            }))
        }),

        "property" => json!({
            "ok": op["runs"]
                .as_array()
                .map(|runs| runs.iter().map(answer).collect::<Vec<_>>())
                .unwrap_or_default(),
        }),

        "billid" => attempt(|| Ok(json!(splitz_core::derive_bill_id(&op["entry"])?))),

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
