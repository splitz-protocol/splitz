//! What a signature covers, and who a participant is (SPEC.md §10.6, §10.7).

use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

use crate::canonical_json::canonical_json;
use crate::error::Result;

/// The domain separator an entry's signature covers.
pub const ENTRY_SIGNING_DOMAIN: &str = "splitz-entry-v1";

/// The bytes an entry's signature covers (§10.6).
///
/// `sig` is excluded because it is the output, and `v` because an entry does
/// not carry its own format version through an implementation's object model:
/// a reader re-encodes with the version it writes, so a signature covering `v`
/// would stop verifying for every existing entry the day it changed.
///
/// Returned as text. A test asserting that a signature verified would pass in
/// two implementations that disagree about the bytes, each checking its own.
pub fn signing_message(entry: &Value) -> Result<String> {
    let mut body = Map::new();
    if let Some(obj) = entry.as_object() {
        for (k, v) in obj {
            if k != "sig" && k != "v" {
                body.insert(k.clone(), v.clone());
            }
        }
    }
    Ok(format!(
        "{ENTRY_SIGNING_DOMAIN}{}",
        canonical_json(&Value::Object(body))?
    ))
}

/// Which key, if any, speaks for each participant (§10.7).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Identities {
    /// Participant id to the key bound to it.
    pub bound: BTreeMap<String, String>,
    /// Ids two keys each claim. Neither is bound: nothing inside the log says
    /// which is the person, and `at` is whatever its author wrote, so
    /// resolving by time hands the identity to whoever backdates furthest.
    pub contested: BTreeSet<String>,
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
    // it carries, stating a key, whose signature verifies against that key. An
    // entry naming somebody else proves nothing about them, whoever signed it.
    let mut claims: BTreeMap<String, BTreeSet<String>> = BTreeMap::new();
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
        if entry.get("author").and_then(Value::as_str) != Some(id) {
            continue;
        }
        if !verify(entry, key) {
            continue;
        }
        claims
            .entry(id.to_owned())
            .or_default()
            .insert(key.to_owned());
    }

    let mut contested: BTreeSet<String> = BTreeSet::new();
    for (id, keys) in &claims {
        // A join claiming the creator's id is not a rival claim; §10.7 refuses
        // it rather than contesting the one identity the invite proves.
        if *id == creator {
            continue;
        }
        if keys.len() > 1 {
            contested.insert(id.clone());
        } else if let Some(key) = keys.iter().next() {
            bound.insert(id.clone(), key.clone());
        }
    }

    Identities { bound, contested }
}
