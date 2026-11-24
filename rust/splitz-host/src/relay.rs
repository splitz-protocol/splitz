//! The optional transport: a store of ciphertext blobs, grouped per bill.

use serde_json::{json, Value};
use std::sync::Mutex;

use crate::error::HostError;
use crate::relay_work::{relay_work_bits, relay_work_stamp, RELAY_WORK_BASE, RELAY_WORK_MAX_BASE};
use crate::transport::HttpTransport;
use crate::wallet::SplitsRelay;

/// Raised when the relay could not be reached, refused, or answered with
/// something that is not a channel.
fn relay_error(message: impl Into<String>, transient: bool) -> HostError {
    HostError::Relay {
        message: message.into(),
        transient,
    }
}

/// Derives the channel a bill syncs under.
///
/// The bill id's SHA-256, hex encoded — a hash rather than the id itself,
/// because the id is also live in every invite and every scanned code.
/// Participants all know the bill id and so all compute the same channel;
/// somebody who only sees relay traffic cannot run it backwards to the id, let
/// alone to the contents.
pub fn channel_for_bill(bill_id: &str) -> String {
    splitz_core::channel_for(bill_id)
}

/// The relay for a build that names none.
///
/// Every call fails, and says why. The alternative is a sync indicator that
/// never resolves.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnconfiguredSplitsRelay;

impl UnconfiguredSplitsRelay {
    pub const REASON: &'static str = "This build has no bill relay, so bills stay on this device. \
                                      Share them by QR code instead.";
}

impl SplitsRelay for UnconfiguredSplitsRelay {
    fn push(&self, _channel: &str, _blobs: &[String]) -> Result<(), HostError> {
        Err(relay_error(Self::REASON, false))
    }

    fn fetch(&self, _channel: &str) -> Result<Vec<String>, HostError> {
        Err(relay_error(Self::REASON, false))
    }
}

/// A relay that keeps blobs in memory.
///
/// For tests, and for wiring two in-process participants together. Not for
/// moving bytes between devices: what is pushed here does not outlive the
/// process and is visible to nobody else.
#[derive(Debug, Default)]
pub struct InMemorySplitsRelay {
    channels: Mutex<Vec<(String, Vec<String>)>>,
}

impl SplitsRelay for InMemorySplitsRelay {
    fn push(&self, channel: &str, blobs: &[String]) -> Result<(), HostError> {
        let mut held = self.channels.lock().unwrap();
        let slot = match held.iter().position(|(name, _)| name == channel) {
            Some(i) => &mut held[i].1,
            None => {
                held.push((channel.to_owned(), Vec::new()));
                &mut held.last_mut().unwrap().1
            }
        };
        // Insertion order, and no duplicates: pushing a blob a channel already
        // holds changes nothing, so a retry cannot create a second copy.
        for blob in blobs {
            if !slot.contains(blob) {
                slot.push(blob.clone());
            }
        }
        Ok(())
    }

    fn fetch(&self, channel: &str) -> Result<Vec<String>, HostError> {
        Ok(self
            .channels
            .lock()
            .unwrap()
            .iter()
            .find(|(name, _)| name == channel)
            .map(|(_, blobs)| blobs.clone())
            .unwrap_or_default())
    }
}

/// One page of a channel, as `GET <origin>/c/<channel>?from=<n>` answers it
/// (SPEC.md §15.5).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RelayPage {
    pub blobs: Vec<String>,
    /// The index the next page starts at, or `None` on the last page.
    pub next: Option<u64>,
}

/// A relay backed by an HTTP blob store.
///
/// Two routes under `origin`: `POST /c/<channel>` with
/// `{"blobs":[…],"work":"…"}` adds blobs, `GET /c/<channel>` returns them.
/// `work` is a stamp for those blobs ([`relay_work_stamp`]): what a push
/// carries in place of anything that names its sender. The server stores opaque bytes
/// keyed by the channel hash — never the bill id, never plaintext.
pub struct HttpSplitsRelay<'a> {
    origin: String,
    transport: &'a dyn HttpTransport,
}

impl<'a> HttpSplitsRelay<'a> {
    /// A blob longer than this is refused rather than sent. Mirrored by the
    /// server: a bound only one side keeps is not a bound.
    pub const MAX_BLOB_CHARS: usize = 64 * 1024;

    /// A push body longer than this, in UTF-8 bytes, is refused by the
    /// server, so a push is split into requests that each fit. Mirrored by the
    /// server.
    pub const MAX_BODY_BYTES: usize = 32 * 1024 * 1024;

    /// `origin` is a scheme, a host and an optional path. A query or a
    /// fragment is refused: the channel is appended to the path, and an origin
    /// carrying either would put it after them, addressing something else.
    pub fn new(origin: &str, transport: &'a dyn HttpTransport) -> Result<Self, HostError> {
        Self::check_origin(origin)?;
        Ok(Self {
            origin: origin.to_owned(),
            transport,
        })
    }

    fn check_origin(origin: &str) -> Result<(), HostError> {
        if origin.contains('?') || origin.contains('#') {
            return Err(relay_error(
                "A relay origin carries no query and no fragment",
                false,
            ));
        }
        Ok(())
    }

    /// The URL both routes address for `channel` under `origin`:
    /// `<origin>/c/<channel>`. Refused, not retryable, for an origin carrying
    /// a query or a fragment.
    pub fn channel_url(origin: &str, channel: &str) -> Result<String, HostError> {
        Self::check_origin(origin)?;
        Ok(format!("{origin}/c/{channel}"))
    }

    /// The bytes a body keeps free for its stamp: `,"work":"`, at most 20
    /// digits, and `"`.
    pub const WORK_ROOM: usize = 32;

    /// The most pages one fetch reads. A relay holds at most 512 MiB of blob
    /// characters, a page at least 8 MiB of characters-plus-3, and a blob is
    /// at least one character: 512 MiB × 4 ÷ 8 MiB. A relay still answering a
    /// `next` past it is refused rather than read without end.
    pub const MAX_PAGES: u32 = 256;

    /// `<origin>/c/<channel>?from=<from>`: the page of `channel` starting at
    /// blob `from`. Refused, not retryable, for an origin carrying a query or
    /// a fragment.
    pub fn page_url(origin: &str, channel: &str, from: u64) -> Result<String, HostError> {
        Ok(format!(
            "{}?from={from}",
            Self::channel_url(origin, channel)?
        ))
    }

    /// The body `POST <origin>/c/<channel>` carries for `blobs`:
    /// `{"blobs":[…],"work":"…"}`, stamped for a relay asking `base`, or
    /// `None` when there is nothing to push and no request is made. A blob
    /// over `MAX_BLOB_CHARS` is refused, not retryable, before anything is
    /// sent.
    pub fn push_body(
        channel: &str,
        blobs: &[String],
        base: u32,
    ) -> Result<Option<String>, HostError> {
        if blobs.is_empty() {
            return Ok(None);
        }
        if base > RELAY_WORK_MAX_BASE {
            return Err(relay_error(
                format!("A relay asks for at most {RELAY_WORK_MAX_BASE} bits, not {base}"),
                false,
            ));
        }
        for blob in blobs {
            let chars = blob.chars().count();
            if chars > Self::MAX_BLOB_CHARS {
                return Err(relay_error(
                    format!(
                        "A blob of {chars} characters is over the {} the relay accepts",
                        Self::MAX_BLOB_CHARS
                    ),
                    false,
                ));
            }
        }
        Ok(Some(Self::stamped(channel, blobs, base)))
    }

    /// `{"blobs":[…],"work":"…"}` for `blobs`, stamped at `base`.
    fn stamped(channel: &str, blobs: &[String], base: u32) -> String {
        let chars = blobs.iter().map(|b| b.chars().count()).sum();
        let nonce = relay_work_stamp(channel, blobs, relay_work_bits(chars, base));
        json!({ "blobs": blobs, "work": nonce }).to_string()
    }

    /// `blobs` as the push bodies that carry them, in order, each at most
    /// `MAX_BODY_BYTES` once stamped. Refused as [`Self::push_body`] refuses;
    /// every blob is then at most `MAX_BLOB_CHARS`, so every body holds at
    /// least one.
    pub fn push_bodies(blobs: &[String]) -> Result<Vec<String>, HostError> {
        const OPEN: &str = "{\"blobs\":[";
        const CLOSE: &str = "]}";
        let empty = OPEN.len() + CLOSE.len() + Self::WORK_ROOM;
        let mut bodies = Vec::new();
        let mut batch: Vec<String> = Vec::new();
        let mut size = empty;
        for blob in blobs {
            let chars = blob.chars().count();
            if chars > Self::MAX_BLOB_CHARS {
                return Err(relay_error(
                    format!(
                        "A blob of {chars} characters is over the {} the relay accepts",
                        Self::MAX_BLOB_CHARS
                    ),
                    false,
                ));
            }
            let encoded = Value::String(blob.clone()).to_string();
            // One more blob costs its bytes and, after the first, a comma.
            if !batch.is_empty() && size + 1 + encoded.len() > Self::MAX_BODY_BYTES {
                bodies.push(format!("{OPEN}{}{CLOSE}", batch.join(",")));
                batch.clear();
                size = empty;
            }
            size += usize::from(!batch.is_empty()) + encoded.len();
            batch.push(encoded);
        }
        if !batch.is_empty() {
            bodies.push(format!("{OPEN}{}{CLOSE}", batch.join(",")));
        }
        Ok(bodies)
    }

    /// Reads the relay's answer to a push, whatever its HTTP status. Anything
    /// but `{"ok":true}` is a refusal, and retryable: the relay answered, and
    /// what it refused — a full store, a dropped body — may pass later.
    pub fn push_answer(body: &str) -> Result<(), HostError> {
        if Self::decode(body)?.get("ok") != Some(&Value::Bool(true)) {
            return Err(relay_error(
                format!("The relay refused the push: {body}"),
                true,
            ));
        }
        Ok(())
    }

    /// Reads the relay's answer to the page starting at `from`, whatever its
    /// HTTP status. An answer without a list of blobs, or with a `next` that
    /// is not `from` plus the one or more blobs it carries, is refused, and
    /// retryable.
    pub fn fetch_page(body: &str, from: u64) -> Result<RelayPage, HostError> {
        let decoded = Self::decode(body)?;
        let Some(Value::Array(blobs)) = decoded.get("blobs").cloned() else {
            return Err(relay_error("The relay answered without a channel", true));
        };
        let next = match decoded.get("next") {
            None | Some(Value::Null) => None,
            Some(given) => {
                let follows = given.as_u64().filter(|&n| {
                    !blobs.is_empty() && Some(n) == from.checked_add(blobs.len() as u64)
                });
                let Some(n) = follows else {
                    return Err(relay_error(
                        format!(
                            "The relay answered a page from {from} with {} blob(s) and next {given}",
                            blobs.len()
                        ),
                        true,
                    ));
                };
                Some(n)
            }
        };
        // Everything here was written by somebody else and is opened under the
        // bill key afterwards. A non-string is dropped rather than refused: it
        // is reached by talking to a server nobody here runs.
        Ok(RelayPage {
            blobs: blobs
                .into_iter()
                .filter_map(|b| b.as_str().map(str::to_owned))
                .collect(),
            next,
        })
    }

    /// The base a relay that refused a push asks for now, when its answer
    /// names one above `done` and no higher than [`RELAY_WORK_MAX_BASE`]: the push is then
    /// stamped once more at that base. `None` for any other answer.
    pub fn work_asked(body: &str, done: u32) -> Option<u32> {
        let asked = Self::decode(body).ok()?.get("work")?.as_u64()?;
        let asked = u32::try_from(asked).ok()?;
        (asked > done && asked <= RELAY_WORK_MAX_BASE).then_some(asked)
    }

    /// Why a request never reached an answer, as §15.5 raises it: retryable.
    fn not_reached(cause: &str) -> HostError {
        relay_error(format!("Could not reach the relay: {cause}"), true)
    }

    fn decode(body: &str) -> Result<Value, HostError> {
        let decoded: Value = serde_json::from_str(body)
            .map_err(|_| relay_error("The relay answered with something that is not JSON", true))?;
        if !decoded.is_object() {
            return Err(relay_error(
                "The relay answered with something that is not a channel",
                true,
            ));
        }
        Ok(decoded)
    }
}

impl SplitsRelay for HttpSplitsRelay<'_> {
    fn push(&self, channel: &str, blobs: &[String]) -> Result<(), HostError> {
        let url = Self::channel_url(&self.origin, channel)?;
        for body in Self::push_bodies(blobs)? {
            let batch: Vec<String> = Self::decode(&body)?["blobs"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|b| b.as_str().map(str::to_owned))
                .collect();
            let mut base = RELAY_WORK_BASE;
            // A relay that asks for more work than was done answers what it
            // asks for now, and the push is stamped once more for that.
            loop {
                let answer = self
                    .transport
                    .post(&url, &Self::stamped(channel, &batch, base))
                    .map_err(|e| Self::not_reached(&e))?;
                match Self::push_answer(&answer) {
                    Ok(()) => break,
                    Err(refused) => match Self::work_asked(&answer, base) {
                        Some(asked) if base == RELAY_WORK_BASE => base = asked,
                        _ => return Err(refused),
                    },
                }
            }
        }
        Ok(())
    }

    fn fetch(&self, channel: &str) -> Result<Vec<String>, HostError> {
        let mut held = Vec::new();
        let mut from = 0;
        for _ in 0..Self::MAX_PAGES {
            let answer = self
                .transport
                .get(&Self::page_url(&self.origin, channel, from)?)
                .map_err(|e| Self::not_reached(&e))?;
            let page = Self::fetch_page(&answer, from)?;
            held.extend(page.blobs);
            match page.next {
                None => return Ok(held),
                Some(next) => from = next,
            }
        }
        Err(relay_error(
            format!("The relay answered more than {} pages", Self::MAX_PAGES),
            true,
        ))
    }
}
