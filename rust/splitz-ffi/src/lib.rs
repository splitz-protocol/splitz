//! The splitz protocol and its wallet plumbing, across a foreign function
//! boundary.
//!
//! `splitz-core` decides what a bill is and what anyone owes; `splitz-host` is
//! the plumbing a wallet needs around it. This crate is the two of them as a
//! Kotlin, Swift or Python wallet reaches them: SPEC.md §15's seven interfaces
//! as foreign-implemented traits, and one session object that holds them.
//!
//! An entry crosses as the JSON §9.3 canonicalises, because an entry is the
//! protocol's own wire format and a wallet never inspects one. Everything a
//! person is shown crosses as a typed record.

uniffi::setup_scaffolding!();

pub mod convert;
pub mod error;
pub mod records;
pub mod seam;
pub mod session;

pub use error::SplitzError;
pub use records::*;
pub use seam::*;
pub use session::SplitzSession;
