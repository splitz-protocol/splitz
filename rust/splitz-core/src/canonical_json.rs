//! Canonical JSON (SPEC.md §9.3).
//!
//! Object keys in ascending UTF-8 byte order, no insignificant whitespace.
//! This is the encoding used wherever a document's bytes are compared, hashed
//! or signed, which is the one place key order stops being presentational and
//! becomes part of the message.

use serde_json::Value;

use crate::error::{code, Result, SplitError};

/// Encodes `value` canonically.
///
/// A floating point number anywhere in the document is refused with
/// `canonical_json_float` rather than truncated: §2 puts every amount in minor
/// units as an integer, so a document carrying one was not written by a
/// conforming writer.
pub fn canonical_json(value: &Value) -> Result<String> {
    let mut out = String::new();
    write(value, &mut out)?;
    Ok(out)
}

fn write(value: &Value, out: &mut String) -> Result<()> {
    match value {
        Value::Null => out.push_str("null"),
        Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                out.push_str(&i.to_string());
            } else if let Some(u) = n.as_u64() {
                out.push_str(&u.to_string());
            } else {
                return Err(SplitError::new(
                    code::CANONICAL_JSON_FLOAT,
                    format!("A canonical document carries integer minor units, got {n}"),
                ));
            }
        }
        Value::String(s) => out.push_str(&serde_json::to_string(s).expect("a string encodes")),
        Value::Array(items) => {
            out.push('[');
            for (i, item) in items.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                write(item, out)?;
            }
            out.push(']');
        }
        Value::Object(map) => {
            let mut keys: Vec<&String> = map.keys().collect();
            keys.sort_by(|a, b| a.as_bytes().cmp(b.as_bytes()));
            out.push('{');
            for (i, key) in keys.iter().enumerate() {
                if i > 0 {
                    out.push(',');
                }
                out.push_str(&serde_json::to_string(key).expect("a key encodes"));
                out.push(':');
                write(&map[*key], out)?;
            }
            out.push('}');
        }
    }
    Ok(())
}
