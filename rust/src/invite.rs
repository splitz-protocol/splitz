//! Invite URIs, scanned payloads and sealed frames (SPEC.md §11).
//!
//! The invite grammar is exact and is never delegated to a general URI
//! library. A general parser brings its own answers to questions this format
//! has to fix itself, and two libraries answer them differently.

use serde_json::Value;
use std::collections::BTreeSet;

use crate::canonical_json::canonical_json;
use crate::error::{code, Result, SplitError};
use crate::zip321::base64url;

const PREFIX: &str = "splitz://join";

/// The invite format version this crate writes and the highest it reads.
pub const INVITE_VERSION: i64 = 1;

/// A bill id in an invite holds at most this many base64url characters. A
/// derived id is 22; the cap is generous rather than tight.
pub const MAX_INVITE_BILL_ID: usize = 128;

/// Exactly the code points §11.1 calls scan padding.
///
/// Not a general trim: Rust's `str::trim()` strips Unicode whitespace and
/// leaves U+FEFF, Dart's `String.trim()` strips U+FEFF too. One QR code with a
/// leading byte order mark would then be an invite to one reader and not the
/// other.
pub const SCAN_PADDING: [char; 5] = ['\u{0009}', '\u{000A}', '\u{000D}', '\u{0020}', '\u{FEFF}'];

const UNRESERVED: &str = "-._~";
const INVITE_EXTRA: &str = "!*'()";

/// What an invite carries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Invite {
    pub bill_id: String,
    /// The symmetric key the bill's contents are encrypted under. This protocol
    /// carries the key; it does not encrypt.
    pub key: String,
    pub name: String,
    pub expiry: Option<i64>,
}

/// Strips scan padding from both ends of `text`, and nothing else.
///
/// The same characters anywhere inside are content.
pub fn strip_scan_padding(text: &str) -> &str {
    text.trim_matches(|c| SCAN_PADDING.contains(&c))
}

fn is_b64url(value: &str) -> bool {
    value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// Percent-decodes one parameter value.
///
/// `+` is a literal plus, never a space: that convention belongs to HTML form
/// encoding, and a `+` inside a bill id or a key must survive the round trip. A
/// malformed escape is left literal.
fn percent_decode(value: &str) -> String {
    let raw = value.as_bytes();
    let mut out: Vec<u8> = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        if raw[i] == b'%' && i + 2 < raw.len() {
            let hex = std::str::from_utf8(&raw[i + 1..i + 3]).ok();
            if let Some(byte) = hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(raw[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn escape(text: &str) -> String {
    let mut out = String::new();
    for byte in text.as_bytes() {
        let ch = *byte as char;
        if byte.is_ascii_alphanumeric() || UNRESERVED.contains(ch) || INVITE_EXTRA.contains(ch) {
            out.push(ch);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Parses an invite.
pub fn parse_invite(text: &str) -> Result<Invite> {
    let s = strip_scan_padding(text);

    // The scheme and host are matched case-sensitively.
    let rest = s.strip_prefix(PREFIX).ok_or_else(|| {
        SplitError::new(
            code::INVITE_NOT_AN_INVITE,
            format!("Not an invite: \"{text}\""),
        )
    })?;
    // Nothing may follow `join` but an optional query.
    if !rest.is_empty() && !rest.starts_with('?') {
        return Err(SplitError::new(
            code::INVITE_NOT_AN_INVITE,
            format!("Not an invite: \"{text}\""),
        ));
    }
    let query = rest.strip_prefix('?').unwrap_or("");

    // Where a parameter appears more than once, the first occurrence wins.
    let mut fields: Vec<(String, String)> = Vec::new();
    if !query.is_empty() {
        for pair in query.split('&') {
            let (key, value) = match pair.split_once('=') {
                Some((k, v)) => (k, v),
                None => (pair, ""),
            };
            if !key.is_empty() && !fields.iter().any(|(k, _)| k == key) {
                fields.push((key.to_owned(), percent_decode(value)));
            }
        }
    }
    let field = |name: &str| {
        fields
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    };

    let raw_version = field("v").unwrap_or("");
    if raw_version.is_empty() || !raw_version.bytes().all(|b| b.is_ascii_digit()) {
        return Err(SplitError::new(
            code::INVITE_MISSING_VERSION,
            "An invite states its version",
        ));
    }
    // Bounded before it is compared (§11.1).
    let version: i64 = raw_version.parse().map_err(|_| {
        SplitError::new(
            code::INVITE_MISSING_VERSION,
            format!("A version fits in a signed 64-bit integer, got \"{raw_version}\""),
        )
    })?;
    // No sign, no padding, no whitespace.
    if raw_version != version.to_string() || version < 1 {
        return Err(SplitError::new(
            code::INVITE_MISSING_VERSION,
            format!("A version is a bare decimal integer, got \"{raw_version}\""),
        ));
    }
    if version > INVITE_VERSION {
        return Err(SplitError::new(
            code::INVITE_FUTURE_VERSION,
            format!("This invite is version {version}; this reader implements {INVITE_VERSION}"),
        ));
    }

    let bill_id = field("b").unwrap_or("");
    if bill_id.is_empty() {
        return Err(SplitError::new(
            code::INVITE_MISSING_BILL_ID,
            "An invite names a bill",
        ));
    }
    // A derived id is base64url, so nothing outside that alphabet came from a
    // derivation. This refuses a literal `+`, which percent-decoding leaves
    // intact.
    if bill_id.chars().count() > MAX_INVITE_BILL_ID || !is_b64url(bill_id) {
        return Err(SplitError::new(
            code::INVITE_BAD_BILL_ID,
            format!("Not a bill id: \"{bill_id}\""),
        ));
    }

    let key = field("k").unwrap_or("");
    if key.is_empty() || !is_b64url(key) {
        return Err(SplitError::new(
            code::INVITE_MISSING_KEY,
            "An invite carries a base64url key",
        ));
    }

    let expiry = match field("x") {
        None => None,
        Some(raw) => {
            if raw.is_empty() || !raw.bytes().all(|b| b.is_ascii_digit()) {
                return Err(SplitError::new(
                    code::INVITE_BAD_EXPIRY,
                    format!("Not an expiry: \"{raw}\""),
                ));
            }
            let value = raw.parse::<i64>().map_err(|_| {
                SplitError::new(
                    code::INVITE_BAD_EXPIRY,
                    format!("An expiry is a bare decimal integer within 64 bits, got \"{raw}\""),
                )
            })?;
            // A bare decimal integer, as `v` is: no padding.
            if raw != value.to_string() {
                return Err(SplitError::new(
                    code::INVITE_BAD_EXPIRY,
                    format!("An expiry is a bare decimal integer within 64 bits, got \"{raw}\""),
                ));
            }
            Some(value)
        }
    };

    Ok(Invite {
        bill_id: bill_id.to_owned(),
        key: key.to_owned(),
        name: field("n").unwrap_or("").to_owned(),
        expiry,
    })
}

/// Renders an invite.
///
/// An encoder refuses what [`parse_invite`] refuses: a bound enforced only on
/// decode lets a caller build a URI no reader accepts, and the caller learns
/// of it from somebody else's scanner.
pub fn render_invite(invite: &Invite) -> Result<String> {
    if let Some(x) = invite.expiry {
        if x < 0 {
            return Err(SplitError::new(
                code::INVITE_BAD_EXPIRY,
                format!("An expiry is not negative, got {x}"),
            ));
        }
    }
    if invite.bill_id.is_empty()
        || invite.bill_id.chars().count() > MAX_INVITE_BILL_ID
        || !is_b64url(&invite.bill_id)
    {
        return Err(SplitError::new(
            code::INVITE_BAD_BILL_ID,
            format!("Not a bill id: \"{}\"", invite.bill_id),
        ));
    }
    if invite.key.is_empty() || !is_b64url(&invite.key) {
        return Err(SplitError::new(
            code::INVITE_MISSING_KEY,
            "An invite carries a base64url key",
        ));
    }
    let mut parts = vec![
        format!("v={INVITE_VERSION}"),
        format!("b={}", escape(&invite.bill_id)),
        format!("k={}", escape(&invite.key)),
    ];
    if !invite.name.is_empty() {
        parts.push(format!("n={}", escape(&invite.name)));
    }
    if let Some(x) = invite.expiry {
        parts.push(format!("x={x}"));
    }
    Ok(format!("{PREFIX}?{}", parts.join("&")))
}

// --- §11.2 scanned payloads -------------------------------------------------

/// What a version-40 QR code holds in byte mode at error-correction level M.
///
/// Enforced on decode as well as encode: a cap applied only when writing
/// bounds what an implementation emits rather than what it accepts, which is
/// the wrong direction for a trust boundary.
pub const PAYLOAD_CAP: usize = 2331;

/// The payload format version this crate writes and the highest it reads.
pub const PAYLOAD_VERSION: i64 = 1;

/// How deep a document may nest before a reader refuses it (§10.1, §11.2).
///
/// Stated rather than inherited from a JSON library: one reader's parser gives
/// up at its own depth and another does not, and the cap is no defence because
/// a level of nesting costs two bytes. The deepest a conforming document
/// reaches is the `sharedBy` array inside an itemised split, at eight.
pub const MAX_DOCUMENT_DEPTH: usize = 64;

pub fn within_depth(value: &Value, limit: usize) -> bool {
    let mut stack = vec![(value, 1usize)];
    while let Some((node, d)) = stack.pop() {
        if d > limit {
            return false;
        }
        match node {
            Value::Object(o) => stack.extend(o.values().map(|v| (v, d + 1))),
            Value::Array(a) => stack.extend(a.iter().map(|v| (v, d + 1))),
            _ => {}
        }
    }
    true
}

/// The prefix a whole bill's payload carries (section 11.2).
pub const BILL_PREFIX: &str = "splitz1:";

/// The prefix a delta carries. A delta never carries an invite.
pub const DELTA_PREFIX: &str = "splitzd1:";

/// A decoded payload.
#[derive(Debug, Clone, PartialEq)]
pub struct ScannedPayload {
    pub prefix: String,
    pub version: i64,
    pub log: Vec<Value>,
    pub invite: Option<Value>,
}

fn unbase64url(text: &str) -> Option<Vec<u8>> {
    if !is_b64url(text) {
        return None;
    }
    const fn index(b: u8) -> Option<u32> {
        match b {
            b'A'..=b'Z' => Some((b - b'A') as u32),
            b'a'..=b'z' => Some((b - b'a') as u32 + 26),
            b'0'..=b'9' => Some((b - b'0') as u32 + 52),
            b'-' => Some(62),
            b'_' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::new();
    for chunk in text.as_bytes().chunks(4) {
        if chunk.len() == 1 {
            return None;
        }
        let mut acc: u32 = 0;
        for (i, b) in chunk.iter().enumerate() {
            acc |= index(*b)? << (18 - 6 * i);
        }
        out.push((acc >> 16) as u8);
        if chunk.len() > 2 {
            out.push((acc >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(acc as u8);
        }
    }
    Some(out)
}

/// Encodes `body` under `prefix`.
pub fn encode_payload(prefix: &str, body: &Value) -> Result<String> {
    if prefix != BILL_PREFIX && prefix != DELTA_PREFIX {
        return Err(SplitError::new(
            code::PAYLOAD_NOT_A_PAYLOAD,
            format!("No such payload prefix: \"{prefix}\""),
        ));
    }
    let encoded = base64url(canonical_json(body)?.as_bytes());
    if encoded.len() > PAYLOAD_CAP {
        return Err(SplitError::new(
            code::PAYLOAD_TOO_LARGE,
            format!(
                "A payload of {} characters exceeds {PAYLOAD_CAP}",
                encoded.len()
            ),
        ));
    }
    Ok(format!("{prefix}{encoded}"))
}

/// Decodes a scanned payload.
pub fn decode_payload(text: &str) -> Result<ScannedPayload> {
    // Padding is stripped before the prefix is matched and before the size is
    // measured, so padding does not count toward the cap.
    let s = strip_scan_padding(text);
    let prefix = if s.starts_with(BILL_PREFIX) {
        BILL_PREFIX
    } else if s.starts_with(DELTA_PREFIX) {
        DELTA_PREFIX
    } else {
        return Err(SplitError::new(
            code::PAYLOAD_NOT_A_PAYLOAD,
            format!("Not a payload: \"{text}\""),
        ));
    };

    let encoded = &s[prefix.len()..];
    if encoded.len() > PAYLOAD_CAP {
        return Err(SplitError::new(
            code::PAYLOAD_TOO_LARGE,
            format!(
                "A payload of {} characters exceeds {PAYLOAD_CAP}",
                encoded.len()
            ),
        ));
    }

    let damaged = || {
        SplitError::new(
            code::PAYLOAD_DAMAGED,
            "The body is not a canonical document",
        )
    };
    let raw = unbase64url(encoded).ok_or_else(damaged)?;
    let text = String::from_utf8(raw).map_err(|_| damaged())?;
    let body: Value = serde_json::from_str(&text).map_err(|_| damaged())?;
    if !body.is_object() {
        return Err(damaged());
    }
    if !within_depth(&body, MAX_DOCUMENT_DEPTH) {
        return Err(damaged());
    }

    let version = body.get("v").and_then(Value::as_i64).ok_or_else(damaged)?;
    if version < 1 {
        return Err(damaged());
    }
    if version > PAYLOAD_VERSION {
        return Err(SplitError::new(
            code::PAYLOAD_FUTURE_VERSION,
            format!("This payload is version {version}; this reader implements {PAYLOAD_VERSION}"),
        ));
    }

    let log = body
        .get("log")
        .and_then(Value::as_array)
        .ok_or_else(|| SplitError::new(code::PAYLOAD_MISSING_BODY, "A payload carries a log"))?;

    Ok(ScannedPayload {
        prefix: prefix.to_owned(),
        version,
        log: log.clone(),
        // §11.2. Only the bill prefix carries an invite, and only an object
        // is one: a delta's reader already holds a key, and a second one
        // arriving from a peer names a bill and a key that reader never chose.
        invite: if prefix == BILL_PREFIX {
            body.get("invite").filter(|v| v.is_object()).cloned()
        } else {
            None
        },
    })
}

// --- §11.3 sealing ----------------------------------------------------------

/// The sealed frame version.
pub const SEALED_VERSION: u8 = 1;

/// XChaCha20-Poly1305 nonce length, in bytes.
pub const NONCE_BYTES: usize = 24;

/// Poly1305 tag length, in bytes.
pub const TAG_BYTES: usize = 16;

/// What a sealed frame states before the cipher runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SealedFrame {
    pub version: u8,
    /// Derived from the plaintext, so one entry always seals to one blob.
    pub nonce: String,
    pub body_bytes: usize,
}

/// Parses a sealed frame: a version byte, a 24-byte nonce, then the cipher's
/// output.
pub fn parse_sealed_frame(text: &str) -> Result<SealedFrame> {
    let s = strip_scan_padding(text);
    let malformed = |why: &str| SplitError::new(code::SEALED_MALFORMED, why.to_owned());
    if s.is_empty() {
        return Err(malformed("An empty frame"));
    }
    let raw = unbase64url(s).ok_or_else(|| malformed("A frame is base64url"))?;
    if raw.len() < 1 + NONCE_BYTES + TAG_BYTES {
        return Err(malformed("A frame cannot hold a nonce and a tag"));
    }
    let version = raw[0];
    // A version of zero is a malformed frame and not a future format: telling
    // somebody their app is too old sends them to an update that will not
    // help.
    if version < 1 {
        return Err(malformed("A frame states a version of at least 1"));
    }
    if version > SEALED_VERSION {
        return Err(SplitError::new(
            code::SEALED_FUTURE_VERSION,
            format!("This frame is version {version}; this reader implements {SEALED_VERSION}"),
        ));
    }
    Ok(SealedFrame {
        version,
        nonce: base64url(&raw[1..1 + NONCE_BYTES]),
        body_bytes: raw.len() - 1 - NONCE_BYTES,
    })
}

/// What a peer has not seen, and whether it fits one square (section 14.5).
///
/// Three answers, not two. A peer holding everything and a peer holding none
/// of a log too long to encode are opposite states, and one value for both
/// tells somebody their bill is up to date while entries on it have never
/// reached them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Delta {
    /// The peer holds every entry this device does.
    NothingMissing,
    /// The entries the peer has not seen, as one square.
    Square { uri: String, entry_count: usize },
    /// Behind by more than one square can carry. What this needs is a relay.
    TooBig {
        entry_count: usize,
        /// The refusal section 11.2 gave, `payload_too_large` when it is the
        /// cap.
        code: &'static str,
    },
}

/// Computes what `they_have` is missing from `entries` (section 14.5).
///
/// A delta carries no invite: its reader already holds the key (section 11.2).
pub fn delta_for(entries: &[Value], they_have: &BTreeSet<String>) -> Delta {
    let mut ordered: Vec<Value> = entries.to_vec();
    crate::log::order_entries(&mut ordered);
    let missing: Vec<Value> = ordered
        .into_iter()
        .filter(|e| {
            !e.get("id")
                .and_then(Value::as_str)
                .is_some_and(|id| they_have.contains(id))
        })
        .collect();
    if missing.is_empty() {
        return Delta::NothingMissing;
    }
    let entry_count = missing.len();
    match encode_payload(DELTA_PREFIX, &serde_json::json!({ "v": 1, "log": missing })) {
        Ok(uri) => Delta::Square { uri, entry_count },
        Err(e) => Delta::TooBig {
            entry_count,
            code: e.code,
        },
    }
}
