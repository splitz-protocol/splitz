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
}

impl fmt::Display for HostError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HostError::Malformed(why) => write!(f, "{why}"),
            HostError::Storage(why) => write!(f, "{why}"),
            HostError::Sealing(why) => write!(f, "{why}"),
        }
    }
}

impl std::error::Error for HostError {}

pub type Result<T> = core::result::Result<T, HostError>;
