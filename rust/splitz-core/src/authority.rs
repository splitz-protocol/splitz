//! What a signature covers, and who a participant is (SPEC.md §10.6, §10.7).

use serde_json::{Map, Value};
use std::collections::BTreeMap;

use crate::canonical_json::canonical_json;
use crate::error::Result;
use crate::sha256::sha256;
use crate::zip321::{base64url, unbase64url};

/// The domain separator an entry's signature covers.
pub const ENTRY_SIGNING_DOMAIN: &str = "splitz-entry-v2";

/// The bytes an entry's signature covers on the bill `bill_id` (§10.6).
///
/// The bill is part of the message because an entry does not name it: a
/// participant's id and key are the same on every bill, so without it a
/// signature made on one bill verifies on any other.
///
/// `sig` is excluded because it is the output, and `v` because an entry does
/// not carry its own format version through an implementation's object model:
/// a reader re-encodes with the version it writes, so a signature covering `v`
/// would stop verifying for every existing entry the day it changed.
///
/// Returned as text. A test asserting that a signature verified would pass in
/// two implementations that disagree about the bytes, each checking its own.
pub fn signing_message(entry: &Value, bill_id: &str) -> Result<String> {
    let mut body = Map::new();
    if let Some(obj) = entry.as_object() {
        for (k, v) in obj {
            if k != "sig" && k != "v" {
                body.insert(k.clone(), v.clone());
            }
        }
    }
    let mut message = Map::new();
    message.insert("bill".to_owned(), Value::from(bill_id));
    message.insert("entry".to_owned(), Value::Object(body));
    Ok(format!(
        "{ENTRY_SIGNING_DOMAIN}{}",
        canonical_json(&Value::Object(message))?
    ))
}

/// The domain separator a participant id's digest covers (§10.7).
pub const PARTICIPANT_ID_DOMAIN: &str = "splitz-participant-v1";

/// The participant id a key speaks as (§10.7), or `None` for a text that is
/// not a canonical 32-byte key:
/// `base64url( SHA-256( "splitz-participant-v1" || key bytes )[0..16] )`.
///
/// Two keys cannot derive one id, so no second key can claim a participant
/// this binds.
pub fn participant_id(key: &str) -> Option<String> {
    let raw = unbase64url(key).filter(|raw| raw.len() == 32)?;
    let mut message = PARTICIPANT_ID_DOMAIN.as_bytes().to_vec();
    message.extend_from_slice(&raw);
    Some(base64url(&sha256(&message)[..16]))
}

/// Which key, if any, speaks for each participant (§10.7).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Identities {
    /// Participant id to the key bound to it.
    pub bound: BTreeMap<String, String>,
}

/// Resolves identities from the entry set alone.
///
/// Never from arrival order, never from anything a device has seen before: two
/// devices resolving this differently fold a different bill from one log.
///
/// `verify` is the host's curve operation (§13); this fixes everything around
/// the answer, never the answer itself.
pub fn resolve_identities(
    entries: &[Value],
    create: &Value,
    verify: impl Fn(&Value, &str) -> bool,
) -> Identities {
    let creator = create
        .get("author")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_owned();
    let creator_key = create.get("creatorKey").and_then(Value::as_str);

    // The creator is bound by the invite, not by a join: the bill's id is the
    // digest of the entry that states their key, so it needs no prior
    // acquaintance. The signature requirement is not ornamental — absent it,
    // `creatorKey` is a number the author typed.
    let mut bound: BTreeMap<String, String> = BTreeMap::new();
    if let Some(key) = creator_key {
        if verify(create, key) {
            bound.insert(creator.clone(), key.to_owned());
        }
    }

    // A key is bound by a self-claim: a join whose author is the participant
    // it carries, whose id is the one that participant's key derives, and
    // whose signature verifies against that key. An entry naming somebody
    // else proves nothing about them, and a key cannot claim an id it does not
    // derive, so no second key can claim a bound participant.
    for entry in entries {
        if entry.get("kind").and_then(Value::as_str) != Some("joinBill") {
            continue;
        }
        let Some(p) = entry.get("participant") else {
            continue;
        };
        let (Some(id), Some(key)) = (
            p.get("id").and_then(Value::as_str),
            p.get("identityKey").and_then(Value::as_str),
        ) else {
            continue;
        };
        if entry.get("author").and_then(Value::as_str) != Some(id) || id == creator {
            continue;
        }
        if participant_id(key).as_deref() != Some(id) || !verify(entry, key) {
            continue;
        }
        bound.insert(id.to_owned(), key.to_owned());
    }

    Identities { bound }
}
