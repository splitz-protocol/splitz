//! What this layer refuses, and why.
//!
//! Separate from `splitz_core::SplitError`: a §12 code names a protocol
//! refusal that every implementation must reproduce. A host failure is local —
//! a malformed key, a store that would not write — and carries a sentence
//! rather than a code; a protocol refusal passing through keeps its code.

use std::fmt;

/// A refusal from this layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostError {
    /// An input this layer will not accept: a key of the wrong length, a
    /// value that is not the encoding it claims.
    Malformed(String),
    /// The wallet's own store would not read, write or delete.
    Storage(String),
    /// A value is stored under this name and does not decode. Not absent: a
    /// merge that took it for an empty log would write over it.
    Unreadable(String),
    /// A blob could not be sealed or opened (§11.3).
    Sealing(String),
    /// The relay could not be reached, refused, or answered with something
    /// that is not a channel (§15.5).
    ///
    /// `transient` is whether retrying later could plausibly succeed. It is
    /// false for a relay this build cannot use at all. Reported, never
    /// swallowed: a bill works with no relay, so a transport that quietly does
    /// nothing looks exactly like one that is working.
    Relay { message: String, transient: bool },
    /// A bill cannot be synced. `kind` is why, for a wallet to put in its own
    /// words; `message` names the bill, for a developer.
    Sync { kind: SyncFailure, message: String },
    /// An invite carries a different key for the named bill than the one this
    /// device already holds.
    KeyConflict(String),
    /// A swap could not be arranged (§15.7).
    ///
    /// `transient` is true when retrying the same request could succeed — a
    /// timeout, a 5xx. A quote the provider refused on its merits is not.
    Swap { message: String, transient: bool },
    /// A swap leg `combined_send` will not put in a transaction, and why: the
    /// refusal `swap_send_refusal` gives a deposit sent alone.
    SwapRefused(crate::swaps::SwapSendRefusal),
    /// A price source answered with something that is not a price, or
    /// could not be reached. Not `None`: that means the source cannot price
    /// the currency, and an answer that did not read is a different state.
    Price(String),
    /// A send from this bill is written down and not resolved, or is under
    /// way in this process (§14.3). `pending` is the note, when one is
    /// written; `None` while the earlier send is still under way.
    SendInFlight {
        bill_id: String,
        pending: Option<Box<crate::pending_sends::PendingSend>>,
    },
    /// The bill's own create commits to a key other than the one held for it
    /// (§9.4, `invite_key_mismatch`): what opened under the held key is a
    /// version of the bill somebody else made.
    ForeignKey(String),
    /// A protocol refusal met on the way through this layer, with its §12
    /// code: a caller branches on the code whichever function raised it.
    Protocol(splitz_core::SplitError),
}

/// Why a bill could not be synced. A key that is not the bill's own is
/// [`HostError::ForeignKey`], which carries its §12 code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncFailure {
    /// This device holds no key for the bill.
    NoKey,
    /// The keychain refused to read the key, as it does while locked.
    KeyLocked,
    /// The bill was forgotten on this device while the sync ran.
    Forgotten,
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HostError::Malformed(why) => write!(f, "{why}"),
            HostError::Storage(why) => write!(f, "{why}"),
            HostError::Unreadable(name) => write!(f, "{name} is stored and does not decode"),
            HostError::Sealing(why) => write!(f, "{why}"),
            HostError::Relay { message, .. } => write!(f, "{message}"),
            HostError::Sync { message, .. } => write!(f, "{message}"),
            HostError::KeyConflict(bill) => {
                write!(f, "this device already holds a different key for {bill}")
            }
            HostError::Swap { message, .. } => write!(f, "{message}"),
            HostError::SwapRefused(why) => write!(f, "the swap is refused: {why:?}"),
            HostError::Price(why) => write!(f, "{why}"),
            HostError::SendInFlight { bill_id, .. } => {
                write!(f, "an earlier send from {bill_id} is not resolved")
            }
            HostError::ForeignKey(bill) => write!(
                f,
                "invite_key_mismatch: the key held for {bill} is not the one it was made with"
            ),
            HostError::Protocol(e) => write!(f, "{}: {}", e.code, e.message),
        }
    }
}

impl std::error::Error for HostError {}

pub type Result<T> = core::result::Result<T, HostError>;
