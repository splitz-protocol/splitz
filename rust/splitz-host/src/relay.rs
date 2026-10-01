//! The optional transport: a store of ciphertext blobs, grouped per bill.

use serde_json::{json, Value};
use std::sync::Mutex;

use crate::error::HostError;
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

/// A relay backed by an HTTP blob store.
///
/// Two routes under `origin`: `POST /c/<channel>` with `{"blobs":[…]}` adds
/// blobs, `GET /c/<channel>` returns them. The server stores opaque bytes
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

    /// The body `POST <origin>/c/<channel>` carries for `blobs`:
    /// `{"blobs":[…]}`, or `None` when there is nothing to push and no request
    /// is made. A blob over `MAX_BLOB_CHARS` is refused, not retryable, before
    /// anything is sent.
    pub fn push_body(blobs: &[String]) -> Result<Option<String>, HostError> {
        if blobs.is_empty() {
            return Ok(None);
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
        Ok(Some(json!({ "blobs": blobs }).to_string()))
    }

    /// `blobs` as the push bodies that carry them, in order, each at most
    /// `MAX_BODY_BYTES`. Refused as [`Self::push_body`] refuses; every blob is
    /// then at most `MAX_BLOB_CHARS`, so every body holds at least one.
    pub fn push_bodies(blobs: &[String]) -> Result<Vec<String>, HostError> {
        const OPEN: &str = "{\"blobs\":[";
        const CLOSE: &str = "]}";
        let empty = OPEN.len() + CLOSE.len();
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

    /// Reads the relay's answer to a fetch, whatever its HTTP status: the
    /// blobs of `{"blobs":[…]}`. An answer without that list is refused, and
    /// retryable.
    pub fn fetch_answer(body: &str) -> Result<Vec<String>, HostError> {
        let Some(Value::Array(blobs)) = Self::decode(body)?.get("blobs").cloned() else {
            return Err(relay_error("The relay answered without a channel", true));
        };
        // Everything here was written by somebody else and is opened under the
        // bill key afterwards. A non-string is dropped rather than refused: it
        // is reached by talking to a server nobody here runs.
        Ok(blobs
            .into_iter()
            .filter_map(|b| b.as_str().map(str::to_owned))
            .collect())
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
            let answer = self
                .transport
                .post(&url, &body)
                .map_err(|e| Self::not_reached(&e))?;
            Self::push_answer(&answer)?;
        }
        Ok(())
    }

    fn fetch(&self, channel: &str) -> Result<Vec<String>, HostError> {
        let answer = self
            .transport
            .get(&Self::channel_url(&self.origin, channel)?)
            .map_err(|e| Self::not_reached(&e))?;
        Self::fetch_answer(&answer)
    }
}
