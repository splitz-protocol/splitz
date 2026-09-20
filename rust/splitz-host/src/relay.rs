//! The optional transport: a store of ciphertext blobs, grouped per bill.

use serde_json::{json, Value};
use std::sync::Mutex;

use crate::error::HostError;
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

/// The two calls [`HttpSplitsRelay`] makes.
///
/// Injected rather than made by this crate, so bill sync takes the same
/// network route as the rest of the wallet. On a build that routes through Tor
/// it goes over Tor and fails closed while Tor is starting or broken, instead
/// of being the one path that quietly leaves in the clear.
pub trait RelayTransport {
    /// Posts `body` to `url` and returns the response body, or why not.
    fn post(&self, url: &str, body: &str) -> Result<String, String>;
    /// Fetches `url` and returns the response body, or why not.
    fn get(&self, url: &str) -> Result<String, String>;
}

/// A relay backed by an HTTP blob store.
///
/// Two routes under `origin`: `POST /c/<channel>` with `{"blobs":[…]}` adds
/// blobs, `GET /c/<channel>` returns them. The server stores opaque bytes
/// keyed by the channel hash — never the bill id, never plaintext.
pub struct HttpSplitsRelay<'a> {
    origin: String,
    transport: &'a dyn RelayTransport,
}

impl<'a> HttpSplitsRelay<'a> {
    /// A blob longer than this is refused rather than sent. Mirrored by the
    /// server: a bound only one side keeps is not a bound.
    pub const MAX_BLOB_CHARS: usize = 64 * 1024;

    /// `origin` is a scheme, a host and an optional path. A query or a
    /// fragment is refused: the channel is appended to the path, and an origin
    /// carrying either would put it after them, addressing something else.
    pub fn new(origin: &str, transport: &'a dyn RelayTransport) -> Result<Self, HostError> {
        if origin.contains('?') || origin.contains('#') {
            return Err(relay_error(
                "A relay origin carries no query and no fragment",
                false,
            ));
        }
        Ok(Self {
            origin: origin.to_owned(),
            transport,
        })
    }

    fn channel_url(&self, channel: &str) -> String {
        format!("{}/c/{channel}", self.origin)
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
        if blobs.is_empty() {
            return Ok(());
        }
        for blob in blobs {
            if blob.chars().count() > Self::MAX_BLOB_CHARS {
                return Err(relay_error(
                    format!(
                        "A blob of {} characters is over the {} the relay accepts",
                        blob.chars().count(),
                        Self::MAX_BLOB_CHARS
                    ),
                    false,
                ));
            }
        }
        let body = self
            .transport
            .post(
                &self.channel_url(channel),
                &json!({ "blobs": blobs }).to_string(),
            )
            .map_err(|e| relay_error(format!("Could not reach the relay: {e}"), true))?;
        if Self::decode(&body)?.get("ok") != Some(&Value::Bool(true)) {
            return Err(relay_error(
                format!("The relay refused the push: {body}"),
                true,
            ));
        }
        Ok(())
    }

    fn fetch(&self, channel: &str) -> Result<Vec<String>, HostError> {
        let body = self
            .transport
            .get(&self.channel_url(channel))
            .map_err(|e| relay_error(format!("Could not reach the relay: {e}"), true))?;
        let Some(Value::Array(blobs)) = Self::decode(&body)?.get("blobs").cloned() else {
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
}
