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

/// Parses JSON text a peer wrote, reading its numbers as §2.2 and §9.3 do.
///
/// Two things a plain parse gets wrong, and nothing else changes: `-0` is the
/// integer 0, as every other reader takes it, and a number no double holds
/// (`1e400`) is past the 64-bit range rather than a failed parse, so it
/// reaches the refusal every reader gives it instead of taking the whole text
/// down. Done on the text, not with serde_json's `arbitrary_precision`, which
/// Cargo would switch on for every crate a wallet builds with this one.
pub fn parse_json(text: &str) -> serde_json::Result<Value> {
    serde_json::from_str(&read_numbers(text, true))
}

/// [`parse_json`] for a whole payload body (§11.2): `-0` is 0, and a number
/// no double holds fails the parse, which is `payload_damaged` for the code
/// as every reader answers it. An entry is refused on its own; a body that
/// will not decode is refused whole.
pub fn parse_json_body(text: &str) -> serde_json::Result<Value> {
    serde_json::from_str(&read_numbers(text, false))
}

/// The pass over `text`'s number tokens; strings are copied as they are.
/// With `bound_the_unbounded`, a number no double holds is read as one past
/// the 64-bit range.
fn read_numbers(text: &str, bound_the_unbounded: bool) -> std::borrow::Cow<'_, str> {
    let bytes = text.as_bytes();
    let mut out: Option<String> = None;
    let mut copied = 0;
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'"' => {
                i += 1;
                while i < bytes.len() && bytes[i] != b'"' {
                    i += if bytes[i] == b'\\' { 2 } else { 1 };
                }
                i += 1;
            }
            b'-' | b'0'..=b'9' => {
                let start = i;
                i += 1;
                while i < bytes.len()
                    && matches!(bytes[i], b'0'..=b'9' | b'.' | b'e' | b'E' | b'+' | b'-')
                {
                    i += 1;
                }
                let token = &text[start..i];
                let read = if token == "-0" {
                    Some("0")
                } else if bound_the_unbounded && token.parse::<f64>().is_ok_and(f64::is_infinite) {
                    Some(if token.starts_with('-') {
                        "-1e300"
                    } else {
                        "1e300"
                    })
                } else {
                    None
                };
                if let Some(read) = read {
                    let o = out.get_or_insert_with(String::new);
                    o.push_str(&text[copied..start]);
                    o.push_str(read);
                    copied = i;
                }
            }
            _ => i += 1,
        }
    }
    match out {
        None => std::borrow::Cow::Borrowed(text),
        Some(mut o) => {
            o.push_str(&text[copied..]);
            std::borrow::Cow::Owned(o)
        }
    }
}
