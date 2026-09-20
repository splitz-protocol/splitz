//! Answers `tools/differential/host_generate.py`'s operations.
//!
//! One JSON object per line in, one per line out. The Dart host answers the
//! same list, and the runner diffs them: nothing in `vectors/` can cover a
//! curve operation, because a vector cannot carry a private key.

use std::io::{self, BufRead, Write};

use serde_json::{json, Value};
use splitz_core::{sha256, signing_message};
use splitz_host::{
    base64url_decode, base64url_encode, is_well_formed_key, Signer, IDENTITY_DOMAIN,
};

fn answer(op: &Value) -> Value {
    let name = op.get("op").and_then(Value::as_str).unwrap_or("");
    match name {
        "public_key" => {
            let seed = base64url_decode(op["seed"].as_str().unwrap()).unwrap_or_default();
            json!(Signer.public_key_from_seed(&seed))
        }
        "sign_entry" => {
            let seed = base64url_decode(op["seed"].as_str().unwrap()).unwrap_or_default();
            match signing_message(&op["entry"]) {
                Err(e) => json!({ "error": e.code }),
                Ok(message) => json!({
                    "message": message,
                    "sig": Signer.sign(&seed, message.as_bytes()),
                }),
            }
        }
        "verify" => {
            let sign_with = base64url_decode(op["signWith"].as_str().unwrap()).unwrap_or_default();
            let Ok(message) = signing_message(&op["entry"]) else {
                return json!({ "signable": false });
            };
            let Some(mut sig) = Signer.sign(&sign_with, message.as_bytes()) else {
                return json!({ "signable": false });
            };
            if op["tamper"].as_bool().unwrap_or(false) {
                sig = tamper(&sig);
            }
            let mut entry = op["entry"].clone();
            entry["sig"] = Value::from(sig);
            let true_key = Signer.public_key_from_seed(&sign_with).unwrap_or_default();
            json!({
                "signable": true,
                "againstGivenKey": Signer.verify_entry(&entry, op["key"].as_str().unwrap()),
                "againstTrueKey": Signer.verify_entry(&entry, &true_key),
            })
        }
        "identity_seed" => {
            let viewing_key = op["viewingKey"].as_str().unwrap_or("");
            if viewing_key.is_empty() {
                // Random, on both sides, so there is nothing to compare.
                json!(Value::Null)
            } else {
                json!(base64url_encode(&sha256(
                    format!("{IDENTITY_DOMAIN}:{viewing_key}").as_bytes()
                )))
            }
        }
        "well_formed_key" => json!(is_well_formed_key(op["key"].as_str().unwrap())),
        "b64_round_trip" => match base64url_decode(op["text"].as_str().unwrap()) {
            None => json!(Value::Null),
            Some(raw) => json!({ "bytes": raw.len(), "reencoded": base64url_encode(&raw) }),
        },
        other => json!({ "error": format!("unknown op {other}") }),
    }
}

/// Changes one character of a signature, so it is well formed and wrong.
fn tamper(sig: &str) -> String {
    let mut chars: Vec<char> = sig.chars().collect();
    let first = chars[0];
    chars[0] = if first == 'A' { 'B' } else { 'A' };
    chars.into_iter().collect()
}

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let op: Value = serde_json::from_str(&line).expect("an operation is one JSON object");
        writeln!(out, "{}", serde_json::to_string(&answer(&op)).unwrap())?;
    }
    Ok(())
}
