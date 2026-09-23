//! What this layer refuses, and why.
//!
//! Separate from `splitz_core::SplitError`: a §12 code names a protocol
//! refusal that every implementation must reproduce, and nothing here is one.
//! A host failure is local — a malformed key, a store that would not write —
//! and carries a sentence rather than a code.

use std::fmt;

/// A refusal from this layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HostError {
    /// An input this layer will not accept: a key of the wrong length, a
    /// value that is not the encoding it claims.
    Malformed(String),
    /// The wallet's own store would not read, write or delete.
    Storage(String),
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
    /// A bill cannot be synced.
    Sync(String),
    /// An invite carries a different key for the named bill than the one this
    /// device already holds.
    KeyConflict(String),
    /// A swap could not be arranged (§15.7).
    ///
    /// `transient` is true when retrying the same request could succeed — a
    /// timeout, a 5xx. A quote the provider refused on its merits is not.
    Swap { message: String, transient: bool },
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HostError::Malformed(why) => write!(f, "{why}"),
            HostError::Storage(why) => write!(f, "{why}"),
            HostError::Sealing(why) => write!(f, "{why}"),
            HostError::Relay { message, .. } => write!(f, "{message}"),
            HostError::Sync(why) => write!(f, "{why}"),
            HostError::KeyConflict(bill) => {
                write!(f, "this device already holds a different key for {bill}")
            }
            HostError::Swap { message, .. } => write!(f, "{message}"),
        }
    }
}

impl std::error::Error for HostError {}

pub type Result<T> = core::result::Result<T, HostError>;
