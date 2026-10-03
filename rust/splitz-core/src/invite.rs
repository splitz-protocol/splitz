//! Invite URIs, scanned payloads and sealed frames (SPEC.md §11).
//!
//! The invite grammar is exact and is never delegated to a general URI
//! library. A general parser brings its own answers to questions this format
//! has to fix itself, and two libraries answer them differently.

use serde_json::Value;
use std::collections::BTreeSet;

use crate::canonical_json::canonical_json;
use crate::error::{code, Result, SplitError};
use crate::sha256::{sha256, sha256_hex};
use crate::zip321::{base64url, is_b64url, unbase64url};

const PREFIX: &str = "splitz://join";
const LINK_SCHEME: &str = "https://";

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
        // Section 11.1: `%` then exactly two hexadecimal digits. Anything else
        // stays literal; `from_str_radix` alone accepts a leading `+`.
        if raw[i] == b'%'
            && i + 2 < raw.len()
            && raw[i + 1].is_ascii_hexdigit()
            && raw[i + 2].is_ascii_hexdigit()
        {
            let hex = std::str::from_utf8(&raw[i + 1..i + 3]).expect("ascii");
            out.push(u8::from_str_radix(hex, 16).expect("two hex digits"));
            i += 3;
            continue;
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
    let mut s = strip_scan_padding(text);

    // An https link carries the invite as its fragment, whole (§11.1).
    if s.starts_with(LINK_SCHEME) {
        let hash = s.find('#').ok_or_else(|| {
            SplitError::new(
                code::INVITE_NOT_AN_INVITE,
                format!("A link with no invite: \"{text}\""),
            )
        })?;
        s = &s[hash + 1..];
    }

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
    // §11.1: `k` decodes as unpadded base64url, not merely draws from its
    // alphabet, to the 32 bytes of a bill key (§11.3): a key of any other
    // length passes every check until the first one that seals with it.
    if key.is_empty() || unbase64url(key).is_none_or(|raw| raw.len() != 32) {
        return Err(SplitError::new(
            code::INVITE_MISSING_KEY,
            "An invite carries a 32-byte base64url key",
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
/// Renders `invite` as an https link: `base`, a `#`, and the invite URI
/// (§11.1).
///
/// The invite rides in the fragment, which a browser never sends to `base`'s
/// host, and a chat app shows an https link as one a person can tap. `base` is
/// `https://`, then at least one character that is not `/`, and nothing but
/// printable ASCII — no space and no `#` — or it is refused with
/// `invite_bad_link`.
pub fn render_invite_link(invite: &Invite, base: &str) -> Result<String> {
    let valid = base.strip_prefix(LINK_SCHEME).is_some_and(|rest| {
        !rest.is_empty()
            && !rest.starts_with('/')
            && rest
                .bytes()
                .all(|b| (b'!'..=b'~').contains(&b) && b != b'#')
    });
    if !valid {
        return Err(SplitError::new(
            code::INVITE_BAD_LINK,
            format!("Not a base for an invite link: \"{base}\""),
        ));
    }
    Ok(format!("{base}#{}", render_invite(invite)?))
}

/// Whether `invite` is past the expiry its sender wrote, at
/// `now_unix_seconds`.
///
/// A hint to show, not a refusal (§11.1): `x` is unauthenticated, and removing
/// it yields an invite to the same bill. Compared in whole seconds, because
/// `x` may be any of nineteen digits and scaling it to milliseconds would
/// overflow. An invite with no expiry never expires.
pub fn is_invite_expired(invite: &Invite, now_unix_seconds: i64) -> bool {
    invite.expiry.is_some_and(|x| x < now_unix_seconds)
}

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
    if invite.key.is_empty() || unbase64url(&invite.key).is_none_or(|raw| raw.len() != 32) {
        return Err(SplitError::new(
            code::INVITE_MISSING_KEY,
            "An invite carries a 32-byte base64url key",
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

/// The most characters of encoded body a payload carries: 2331, what a
/// version-40 QR code holds in byte mode at error-correction level M, less the
/// 9 characters of the longer prefix, `splitzd1:`, so the whole scanned string
/// fits a code at that level whichever prefix it carries.
///
/// Enforced on decode as well as encode: a cap applied only when writing
/// bounds what an implementation emits rather than what it accepts, which is
/// the wrong direction for a trust boundary.
pub const PAYLOAD_CAP: usize = 2322;

/// The payload format version this crate writes and the highest it reads.
pub const PAYLOAD_VERSION: i64 = 1;

/// How deep a document may nest before a reader refuses it (§10.1, §11.2).
///
/// Stated rather than inherited from a JSON library: one reader's parser gives
/// up at its own depth and another does not, and the cap is no defence because
/// a level of nesting costs two bytes. The deepest a conforming document
/// reaches is the `sharedBy` array inside an itemised split, at eight.
pub const MAX_DOCUMENT_DEPTH: usize = 64;

/// How deep one entry may nest (section 10.1): `MAX_DOCUMENT_DEPTH` less the
/// two levels a payload wraps it in, the body and its `log`.
///
/// An entry admitted at the document limit would sit at 66 inside the payload
/// that carries it, which every reader refuses, so one entry would leave the
/// bill unsendable by code for good.
pub const MAX_ENTRY_DEPTH: usize = MAX_DOCUMENT_DEPTH - 2;

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
/// True when every number in `value` is one a double holds.
fn all_numbers_finite(value: &Value) -> bool {
    match value {
        Value::Number(n) => n.as_f64().is_some_and(f64::is_finite),
        Value::Array(items) => items.iter().all(all_numbers_finite),
        Value::Object(map) => map.values().all(all_numbers_finite),
        _ => true,
    }
}

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
    let body: Value = crate::canonical_json::parse_json_body(&text).map_err(|_| damaged())?;
    if !body.is_object() {
        return Err(damaged());
    }
    if !within_depth(&body, MAX_DOCUMENT_DEPTH) {
        return Err(damaged());
    }
    // §2.3 and §11.2, over the whole body before any entry is read. The parser
    // refuses a string that is not Unicode scalar values; a number no double
    // holds (`1e400`) parses, and makes the document damaged as a whole: one
    // reader must not open a bill from a code another refuses.
    if !all_numbers_finite(&body) {
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

// --- §11.3, the producing half ---------------------------------------------
//
// The cipher itself is the host's: this crate depends on nothing that could
// hold a key. What it owns is every value §11.3 fixes — the plaintext, the
// nonce derived from it, the frame around the cipher's output, and the
// channel. Those are the parts two devices must agree on byte for byte, and a
// wallet that derives them a second time is a second place for them to drift.

/// The bytes an entry is sealed as (§11.3).
///
/// Canonical JSON (§9.3), UTF-8. The canonical form is what makes the seal
/// idempotent: two devices holding one entry produce one plaintext, therefore
/// one nonce, therefore one blob.
pub fn sealed_plaintext(entry: &Value) -> Result<Vec<u8>> {
    Ok(canonical_json(entry)?.into_bytes())
}

/// The nonce `plaintext` seals under: `SHA-256(plaintext)` truncated to
/// [`NONCE_BYTES`] (§11.3).
///
/// Derived rather than random, so the same entry always seals to the same
/// blob and a relay stores it once however many times it is pushed. Two
/// different plaintexts never share a nonce, which is the one condition the
/// cipher requires.
///
/// **Never derive this from the entry id.** Two payloads can carry one id, and
/// a stream cipher under a repeated (key, nonce) hands a relay the xor of two
/// plaintexts it holds no key for.
pub fn sealed_nonce(plaintext: &[u8]) -> [u8; NONCE_BYTES] {
    let digest = sha256(plaintext);
    let mut nonce = [0u8; NONCE_BYTES];
    nonce.copy_from_slice(&digest[..NONCE_BYTES]);
    nonce
}

/// Frames a sealed `body` for a transport (§11.3).
///
/// `body` is the cipher's own output — ciphertext followed by its tag — and
/// this crate never produces it. The frame is one version byte, the
/// [`NONCE_BYTES`]-byte `nonce`, then `body`, the whole unpadded base64url. A
/// reader knowing the fixed nonce and tag lengths splits it apart with no
/// length fields.
pub fn frame_sealed(nonce: &[u8], body: &[u8]) -> Result<String> {
    if nonce.len() != NONCE_BYTES {
        return Err(SplitError::new(
            code::SEALED_MALFORMED,
            format!("A nonce is {NONCE_BYTES} bytes, got {}", nonce.len()),
        ));
    }
    if body.len() < TAG_BYTES {
        return Err(SplitError::new(
            code::SEALED_MALFORMED,
            format!(
                "A body carries at least a {TAG_BYTES}-byte tag, got {}",
                body.len()
            ),
        ));
    }
    let mut raw = Vec::with_capacity(1 + nonce.len() + body.len());
    raw.push(SEALED_VERSION);
    raw.extend_from_slice(nonce);
    raw.extend_from_slice(body);
    Ok(base64url(&raw))
}

/// The channel a bill's blobs are pushed to and pulled from (§11.3).
///
/// The bill id's SHA-256, lower-case hex. A digest rather than the id itself,
/// because the id is a live address printed in every invite: every
/// participant knows it and computes the same channel, and a relay that only
/// ever sees traffic cannot run it backwards.
pub fn channel_for(bill_id: &str) -> String {
    sha256_hex(bill_id.as_bytes())
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

/// How a peer names one copy of `entry` it holds (section 14.5): the entry's
/// id, and `|` and its `sig` when it carries one.
///
/// Section 10.2's union keeps copies by id and signature, so a peer holding a
/// copy whose signature fails holds the id and still lacks the entry. An id is
/// section 9.5's digest and holds no `|`, so the key splits at its first one.
pub fn copy_key(entry: &Value) -> String {
    let id = entry.get("id").and_then(Value::as_str).unwrap_or_default();
    match entry.get("sig").and_then(Value::as_str) {
        Some(sig) => format!("{id}|{sig}"),
        None => id.to_owned(),
    }
}

/// Computes what `they_have` is missing from `entries` (section 14.5).
///
/// `they_have` names copies by [`copy_key`]. A delta carries no invite: its
/// reader already holds the key (section 11.2).
pub fn delta_for(entries: &[Value], they_have: &BTreeSet<String>) -> Delta {
    let mut ordered: Vec<Value> = entries.to_vec();
    crate::log::order_entries(&mut ordered);
    let missing: Vec<Value> = ordered
        .into_iter()
        .filter(|e| !they_have.contains(&copy_key(e)))
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

/// The domain [`bill_key_digest`] hashes under (section 9.4).
pub const BILL_KEY_DIGEST_DOMAIN: &str = "splitz-bill-key-v1";

/// What a `createBill` states as `keyDigest` for `key` (section 9.4): SHA-256
/// of [`BILL_KEY_DIGEST_DOMAIN`] then the key's 32 bytes, as unpadded
/// base64url. `None` when `key` is not a bill key.
///
/// The bill id is the digest of the create entry, so the key an invite
/// carries is checked against the bill it names: a key somebody else made,
/// handed over with a real bill's id, opens a version of the bill only its
/// holder sees.
pub fn bill_key_digest(key: &str) -> Option<String> {
    let bytes = unbase64url(key)?;
    if bytes.len() != 32 || base64url(&bytes) != key {
        return None;
    }
    let mut message = BILL_KEY_DIGEST_DOMAIN.as_bytes().to_vec();
    message.extend_from_slice(&bytes);
    Some(base64url(&sha256(&message)))
}

/// Whether `key` is the one `create` was made with: `None` when `create`
/// states no `keyDigest`, and so commits to none.
pub fn key_fits_bill(create: &Value, key: &str) -> Option<bool> {
    let stated = create.get("keyDigest")?.as_str()?;
    Some(bill_key_digest(key).as_deref() == Some(stated))
}

/// §9.4. Whether `entry` is `bill_id`'s own create and commits to a key other
/// than `key`.
///
/// Its own: a `createBill` whose id is `bill_id` **and derives from it**. The
/// id member is whatever its writer typed, so a create that merely states
/// `bill_id` proves nothing, and anybody holding the key could otherwise seal
/// one with another `keyDigest` and have every device refuse the real bill.
pub fn create_refuses_key(entry: &Value, bill_id: &str, key: &str) -> bool {
    entry.get("kind").and_then(Value::as_str) == Some("createBill")
        && entry.get("id").and_then(Value::as_str) == Some(bill_id)
        && crate::log::derive_bill_id(entry).is_ok_and(|derived| derived == bill_id)
        && key_fits_bill(entry, key) == Some(false)
}
