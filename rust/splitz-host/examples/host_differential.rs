//! Answers `tools/differential/host_generate.py`'s operations.
//!
//! One JSON object per line in, one per line out. The Dart host answers the
//! same list, and the runner diffs them: nothing in `vectors/` can cover a
//! curve operation, because a vector cannot carry a private key.

use std::io::{self, BufRead, Write};

use serde_json::{json, Value};
use splitz_core::{decode_bill, sha256, signing_message, SetAside};
use splitz_host::{
    activity_of, awaiting_confirmation_by, base64url_decode, base64url_encode, is_well_formed_key,
    BillEvent, BillStorage, BillStore, HostError, InMemoryBillStorage, Sealing, Signer,
    IDENTITY_DOMAIN,
};

/// One history line, as JSON, so the two implementations are compared line for
/// line rather than by a summary either could get wrong the same way.
fn event_json(e: &BillEvent) -> Value {
    json!({
        "entryId": e.entry_id,
        "kind": format!("{:?}", e.kind),
        "author": e.author,
        "at": e.at,
        "subject": e.subject,
        "amount": e.amount_minor_units,
        "description": e.description,
        "method": e.method,
        "reference": e.reference,
        "withdrawn": e.withdrawn,
        "refusedCode": e.refused_code,
        "confirmed": e.confirmed,
        "applied": e.applied(),
    })
}

/// The two implementations word their refusals differently; what has to match
/// is *which* refusal. Both drivers map to this vocabulary.
fn tag(e: &HostError) -> &'static str {
    let HostError::Sealing(why) = e else {
        return "other";
    };
    for (prefix, name) in [
        ("Empty blob", "empty"),
        ("Blob is format v", "version"),
        ("Blob is too short", "short"),
        ("Malformed base64url", "malformed_b64"),
        ("Key is ", "key_length"),
        ("Could not open", "auth"),
        ("Opened blob is not UTF-8", "not_utf8"),
        ("Opened blob is not JSON", "not_json"),
        ("Opened blob is not an entry", "not_entry"),
        ("An entry is not sealable", "not_sealable"),
    ] {
        if why.starts_with(prefix) {
            return name;
        }
    }
    "unclassified"
}

/// Flips a bit of a blob: 1 in the ciphertext, 2 in the version byte.
fn tamper_blob(blob: &str, how: i64) -> String {
    let Some(mut bytes) = base64url_decode(blob) else {
        return blob.to_owned();
    };
    match how {
        1 if bytes.len() > 3 => {
            let last = bytes.len() - 3;
            bytes[last] ^= 0x01;
        }
        2 if !bytes.is_empty() => bytes[0] = bytes[0].wrapping_add(1),
        _ => {}
    }
    base64url_encode(&bytes)
}

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
        "seal_open" => {
            let key = op["key"].as_str().unwrap();
            let blob = match Sealing.seal(&op["entry"], key) {
                Err(e) => return json!({ "sealed": false, "why": tag(&e) }),
                Ok(blob) => blob,
            };
            let presented = tamper_blob(&blob, op["tamper"].as_i64().unwrap_or(0));
            let open_with = op["openWith"].as_str().unwrap_or(key);
            match Sealing.open(&presented, open_with) {
                Ok(entry) => json!({ "sealed": true, "blob": blob, "opened": true,
                                     "entry": entry }),
                Err(e) => json!({ "sealed": true, "blob": blob, "opened": false,
                                  "why": tag(&e) }),
            }
        }
        "open_raw" => match Sealing.open(op["blob"].as_str().unwrap(), op["key"].as_str().unwrap())
        {
            Ok(entry) => json!({ "opened": true, "entry": entry }),
            Err(e) => json!({ "opened": false, "why": tag(&e) }),
        },
        "activity" => {
            let Ok(bill) = decode_bill(&op["bill"]) else {
                return json!({ "decoded": false });
            };
            let entries: Vec<Value> = op["entries"].as_array().cloned().unwrap_or_default();
            let set_aside: Vec<SetAside> = op["setAside"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .map(|s| SetAside {
                    id: s["id"].as_str().unwrap_or_default().to_owned(),
                    // The library's codes are constants; a driver reading one
                    // from a file has to hand it a `'static` and this process
                    // is one operation list long.
                    code: Box::leak(
                        s["code"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned()
                            .into_boxed_str(),
                    ),
                })
                .collect();
            let withdrawn: Vec<String> = op["withdrawn"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect();
            let history = activity_of(&entries, &bill, &set_aside, &withdrawn);
            let awaiting = awaiting_confirmation_by(&bill, op["me"].as_str().unwrap_or(""));
            json!({
                "decoded": true,
                "history": history.iter().map(event_json).collect::<Vec<_>>(),
                "awaiting": awaiting.iter().map(|p| json!({
                    "id": p.id, "from": p.from, "to": p.to,
                    "amount": p.amount, "at": p.at,
                })).collect::<Vec<_>>(),
            })
        }
        "store_read" => {
            let storage = InMemoryBillStorage::default();
            storage
                .write("splitz_bill_b1", op["stored"].as_str().unwrap())
                .unwrap();
            let entries = BillStore::new(&storage).read("b1").unwrap();
            json!({ "count": entries.len(), "entries": entries })
        }
        "store_merge" => {
            let storage = InMemoryBillStorage::default();
            let store = BillStore::new(&storage);
            let held: Vec<Value> = op["held"].as_array().cloned().unwrap_or_default();
            let incoming: Vec<Value> = op["incoming"].as_array().cloned().unwrap_or_default();
            match store
                .merge("b1", held)
                .and_then(|_| store.merge("b1", incoming))
            {
                Err(_) => json!({ "merged": false }),
                Ok(merged) => json!({
                    "merged": true,
                    "ids": merged.entries.iter()
                        .map(|e| e["id"].clone()).collect::<Vec<_>>(),
                    "refused": merged.refused.len(),
                    "readBack": store.read("b1").unwrap().len(),
                }),
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
