//! Sealing what leaves the device, so a relay only ever holds ciphertext.

use chacha20poly1305::aead::Aead;
use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
use serde_json::Value;
use splitz_core::{
    frame_sealed, pad_sealed, sealed_nonce, sealed_plaintext, unpad_sealed, NONCE_BYTES, TAG_BYTES,
};

use crate::error::HostError;
use crate::keys::KEY_LENGTH_BYTES;
use crate::signing::base64url_decode;

/// Blob layout version this writes, the first byte of every blob, so a later
/// change to the framing or the cipher is recognised rather than fed to the
/// wrong decoder. [`Sealing::open`] reads every version from 1 to this one.
///
/// §11.3 fixes it; it is not restated here.
pub const BLOB_VERSION: u8 = splitz_core::SEALED_VERSION;

/// Seals a bill's entries under its key.
///
/// An entry crossing a wire is the graph of who ate with whom and who owes
/// whom — the one thing no server is meant to see. So an entry is sealed under
/// the bill's symmetric key before it is handed to any transport. A relay
/// holds the blob, its size, and when it changed; it cannot read a name, an
/// amount, or an address.
///
/// XChaCha20-Poly1305: its 192-bit nonce is long enough to be chosen freely
/// for every blob without the birthday-bound concern a 96-bit nonce carries,
/// so there is no per-key nonce counter to persist and keep consistent across
/// devices that never coordinate.
#[derive(Debug, Clone, Copy, Default)]
pub struct Sealing;

impl Sealing {
    /// Seals one entry into a base64url string safe for a URL, a scanned
    /// payload or a JSON field.
    ///
    /// The nonce is derived from the sealed bytes (§11.3), so the same entry
    /// always seals to the same blob while two different entries never share a
    /// nonce — the one condition the cipher requires. Idempotence is what
    /// keeps a channel finite: a relay keyed by blob content stores an entry
    /// once however often it is pushed. Refused for an entry no bucket holds
    /// (§11.3, `sealed_too_large`).
    pub fn seal(&self, entry: &Value, bill_key: &str) -> Result<String, HostError> {
        let cipher = self.cipher(bill_key)?;
        // Canonical, not this encoder's key order. The nonce comes from these
        // bytes, so a device ordering keys differently would seal one entry
        // into a second blob that every device opens perfectly and none can
        // recognise as the same entry.
        // Padded to a bucket (§11.3), so the blob's size says which bucket the
        // entry fell in and not how long it is.
        let clear = sealed_plaintext(entry)
            .and_then(|plain| pad_sealed(&plain))
            .map_err(|e| HostError::Sealing(format!("An entry is not sealable: {e}")))?;
        let nonce = sealed_nonce(&clear);
        let body = cipher
            .encrypt(XNonce::from_slice(&nonce), clear.as_slice())
            .map_err(|_| HostError::Sealing("The cipher refused the entry".to_owned()))?;
        frame_sealed(&nonce, &body)
            .map_err(|e| HostError::Sealing(format!("The frame is malformed: {e}")))
    }

    /// Opens a blob back into an entry.
    ///
    /// A blob sealed under another key, or altered in transit, fails the
    /// Poly1305 check and is refused rather than returning a wrong plaintext.
    pub fn open(&self, blob: &str, bill_key: &str) -> Result<Value, HostError> {
        let bytes = decode(blob, "blob")?;
        if bytes.is_empty() {
            return Err(HostError::Sealing("Empty blob".to_owned()));
        }
        let version = bytes[0];
        if !(1..=BLOB_VERSION).contains(&version) {
            return Err(HostError::Sealing(format!(
                "Blob is format v{version}; this build reads v1 to v{BLOB_VERSION}"
            )));
        }
        let body = &bytes[1..];
        if body.len() < NONCE_BYTES + TAG_BYTES {
            return Err(HostError::Sealing(
                "Blob is too short to be a sealed entry".to_owned(),
            ));
        }
        let (nonce, sealed) = body.split_at(NONCE_BYTES);
        let clear = self
            .cipher(bill_key)?
            .decrypt(XNonce::from_slice(nonce), sealed)
            // Wrong key or tampered bytes — indistinguishable, and one thing
            // to a caller: this blob is not for this bill.
            .map_err(|_| {
                HostError::Sealing("Could not open: wrong key, or the blob was altered".to_owned())
            })?;

        // A blob that authenticates but does not hold an entry — genuinely
        // corrupt, or crafted by a key-holder to break decoding — is reported
        // the same way, so a sync skips it like any other unopenable blob
        // instead of the refusal aborting the whole pull.
        let plain = if version == 1 {
            clear
        } else {
            unpad_sealed(&clear)
                .map_err(|e| {
                    HostError::Sealing(format!("Opened blob is not padded as §11.3 pads: {e}"))
                })?
                .to_vec()
        };
        let text = String::from_utf8(plain)
            .map_err(|_| HostError::Sealing("Opened blob is not UTF-8".to_owned()))?;
        let decoded: Value = splitz_core::parse_json(&text)
            .map_err(|e| HostError::Sealing(format!("Opened blob is not JSON: {e}")))?;
        if !decoded.is_object() {
            return Err(HostError::Sealing("Opened blob is not an entry".to_owned()));
        }
        Ok(decoded)
    }

    fn cipher(&self, bill_key: &str) -> Result<XChaCha20Poly1305, HostError> {
        let bytes = decode(bill_key, "key")?;
        if bytes.len() != KEY_LENGTH_BYTES {
            return Err(HostError::Sealing(format!(
                "Key is {} bytes; expected {KEY_LENGTH_BYTES}",
                bytes.len()
            )));
        }
        Ok(XChaCha20Poly1305::new(bytes.as_slice().into()))
    }
}

fn decode(value: &str, what: &str) -> Result<Vec<u8>, HostError> {
    base64url_decode(value).ok_or_else(|| HostError::Sealing(format!("Malformed base64url {what}")))
}
