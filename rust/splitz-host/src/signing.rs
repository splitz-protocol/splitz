//! Ed25519 over the message SPEC.md §10.6 fixes.
//!
//! The protocol fixes what a signature covers and everything around the
//! answer; the curve operation is the host's (§13). This is that operation.
//!
//! Why entries are signed at all: a bill's contents are sealed under a key
//! every participant holds, so the seal proves only that a blob came from
//! *someone* who saw the invite. A signature proves *which* participant wrote
//! an entry, which is what stops a peer redirecting somebody else's payout or
//! writing an expense in their name.

use std::cell::RefCell;
use std::collections::{BTreeSet, HashMap};

use ed25519_dalek::{Signature, Signer as _, SigningKey, Verifier as _, VerifyingKey};
use serde_json::Value;
use splitz_core::signing_message;
use splitz_core::zip321::{base64url, unbase64url};

/// An Ed25519 seed is 32 bytes, and so is the public key it yields — which is
/// also the length §9.4 requires of a `creatorKey`.
pub const SEED_BYTES: usize = 32;

/// Unpadded base64url, as §9.4 writes a key.
pub fn base64url_encode(bytes: &[u8]) -> String {
    base64url(bytes)
}

/// Reads unpadded base64url, and tolerates trailing `=` padding.
///
/// §9.4 writes keys unpadded and §11.1 parses an invite's `k` unpadded, but a
/// key handed in from outside may carry padding from whatever produced it.
/// Accepting it here is what keeps a padded key a stored key rather than a
/// failure surfacing later from inside a decryption loop.
pub fn base64url_decode(value: &str) -> Option<Vec<u8>> {
    unbase64url(value.trim_end_matches('='))
}

/// Signs a bill's entries, and answers whether somebody else's verify.
#[derive(Debug, Clone, Copy, Default)]
pub struct Signer;

impl Signer {
    /// The base64url public key for `seed`, in the encoding §9.4 wants.
    pub fn public_key_from_seed(&self, seed: &[u8]) -> Option<String> {
        let seed: [u8; SEED_BYTES] = seed.try_into().ok()?;
        Some(base64url_encode(
            SigningKey::from_bytes(&seed).verifying_key().as_bytes(),
        ))
    }

    /// Signs `message` with `seed`, base64url encoded.
    ///
    /// Ed25519 is deterministic, so re-signing an unchanged entry yields the
    /// same bytes. That is what keeps a sealed blob idempotent: the same entry
    /// pushed twice is one blob, not two.
    pub fn sign(&self, seed: &[u8], message: &[u8]) -> Option<String> {
        let seed: [u8; SEED_BYTES] = seed.try_into().ok()?;
        Some(base64url_encode(
            &SigningKey::from_bytes(&seed).sign(message).to_bytes(),
        ))
    }

    /// Whether `entry`'s signature, made on the bill `bill_id`, verifies
    /// against `public_key`. A signature made on any other bill does not
    /// (§10.6).
    ///
    /// False when the entry is unsigned, when either input is malformed, and
    /// when the signature simply does not match — all of which mean the same
    /// thing to §10.7: this entry was not written by the holder of that key.
    pub fn verify_entry(&self, entry: &Value, public_key: &str, bill_id: &str) -> bool {
        let Some(signature) = entry.get("sig").and_then(Value::as_str) else {
            return false;
        };
        let (Some(signature_bytes), Some(key_bytes)) =
            (base64url_decode(signature), base64url_decode(public_key))
        else {
            return false;
        };
        let Ok(key_bytes) = <[u8; SEED_BYTES]>::try_from(key_bytes.as_slice()) else {
            return false;
        };
        let Ok(signature) = Signature::from_slice(&signature_bytes) else {
            return false;
        };
        let Ok(key) = VerifyingKey::from_bytes(&key_bytes) else {
            return false;
        };
        // §10.6's message, from the protocol rather than from this encoder's
        // key order, so a signature made here verifies in another
        // implementation and one made there verifies here.
        let Ok(message) = signing_message(entry, bill_id) else {
            return false;
        };
        key.verify(message.as_bytes(), &signature).is_ok()
    }

    /// Answers every signature question a fold of `entries` on the bill
    /// `bill_id` can ask, in advance.
    ///
    /// The fold takes a verifier it may call in any order and must be able to
    /// call more than once for one pair; answering ahead of it is what §10.7
    /// requires, since a verifier that answered differently on two devices
    /// would fold two different bills from one log.
    ///
    /// The pairs are enumerable without restating §10.7. The protocol asks
    /// whether an entry verifies against a key that same entry states —
    /// `creatorKey` on a create, `participant.identityKey` on a join — and,
    /// once its author is bound, against the author's key (§10.3), which is
    /// one of the keys the author states in a create or a join of their own.
    /// Any other pair is a question this build did not expect, and
    /// [`VerifiedLog::unanswered`] records it rather than letting a `false`
    /// pass for an answer.
    pub fn prepare<'a, I>(&self, entries: I, bill_id: &str) -> VerifiedLog
    where
        I: IntoIterator<Item = &'a Value>,
    {
        let all: Vec<&Value> = entries.into_iter().collect();
        let mut keys_of: HashMap<String, BTreeSet<String>> = HashMap::new();
        for entry in &all {
            let Some(author) = entry.get("author").and_then(Value::as_str) else {
                continue;
            };
            let key = match entry.get("kind").and_then(Value::as_str) {
                Some("createBill") => entry.get("creatorKey").and_then(Value::as_str),
                Some("joinBill") => entry
                    .get("participant")
                    .filter(|p| p.get("id").and_then(Value::as_str) == Some(author))
                    .and_then(|p| p.get("identityKey"))
                    .and_then(Value::as_str),
                _ => None,
            };
            if let Some(key) = key {
                keys_of
                    .entry(author.to_owned())
                    .or_default()
                    .insert(key.to_owned());
            }
        }
        let mut answers = HashMap::new();
        for entry in all {
            let mut keys: BTreeSet<String> = keys_stated_by(entry).into_iter().collect();
            if let Some(author) = entry.get("author").and_then(Value::as_str) {
                if let Some(own) = keys_of.get(author) {
                    keys.extend(own.iter().cloned());
                }
            }
            for key in keys {
                answers.insert(pair(entry, &key), self.verify_entry(entry, &key, bill_id));
            }
        }
        VerifiedLog {
            answers,
            unanswered: RefCell::new(Vec::new()),
        }
    }
}

/// The keys an entry states about itself.
fn keys_stated_by(entry: &Value) -> Vec<String> {
    let mut out = Vec::new();
    if let Some(key) = entry.get("creatorKey").and_then(Value::as_str) {
        out.push(key.to_owned());
    }
    if let Some(key) = entry
        .get("participant")
        .and_then(|p| p.get("identityKey"))
        .and_then(Value::as_str)
    {
        out.push(key.to_owned());
    }
    out
}

/// Identifies one (entry, key) question.
///
/// `sig` is part of it: §9.5's digest excludes the signature, so a signed
/// entry and its unsigned twin share an id while giving opposite answers.
fn pair(entry: &Value, key: &str) -> String {
    let id = entry.get("id").and_then(Value::as_str).unwrap_or_default();
    let sig = entry.get("sig").and_then(Value::as_str).unwrap_or_default();
    format!("{id}\u{0}{sig}\u{0}{key}")
}

/// Signature answers for one log, ready for a fold.
#[derive(Debug)]
pub struct VerifiedLog {
    answers: HashMap<String, bool>,
    unanswered: RefCell<Vec<String>>,
}

impl VerifiedLog {
    /// Answers one (entry, key) question the fold asks.
    ///
    /// A pair this build did not anticipate is recorded rather than silently
    /// answered: `false` here reads as "that signature is invalid", which is a
    /// different and much quieter claim than "nobody asked".
    pub fn verify(&self, entry: &Value, key: &str) -> bool {
        let question = pair(entry, key);
        match self.answers.get(&question) {
            Some(answer) => *answer,
            None => {
                self.unanswered.borrow_mut().push(question);
                false
            }
        }
    }

    /// Pairs the fold asked about that were never verified.
    ///
    /// Empty on every fold this build understands. Anything here means §10.7
    /// now asks a question [`Signer::prepare`] does not anticipate, and the
    /// identities it reported are not to be trusted.
    pub fn unanswered(&self) -> Vec<String> {
        self.unanswered.borrow().clone()
    }
}
