//! The splitz protocol and its wallet plumbing, across a foreign function
//! boundary.
//!
//! `splitz-core` decides what a bill is and what anyone owes; `splitz-host` is
//! the plumbing a wallet needs around it. This crate is the two of them as a
//! Kotlin, Swift, Dart, JavaScript or Python wallet reaches them.
//!
//! **Nothing here calls back.** SPEC.md §15 names seven interfaces a wallet
//! implements, and across a foreign boundary those are seven sets of
//! callbacks — the one thing every bindings generator gets wrong differently.
//! So a wallet passes the facts it owns and gets an answer; its storage, its
//! keychain, its relay and its send stay in its own language, where they
//! already are. `splitz-host` keeps the traits for a Rust caller, which has no
//! such problem.
//!
//! An entry crosses as the JSON §9.3 canonicalises, because an entry is the
//! protocol's own wire format and a wallet never inspects one. Everything a
//! person is shown crosses as a typed record, and a refusal crosses as its
//! §12 code.

uniffi::setup_scaffolding!();

pub mod convert;
pub mod error;
pub mod host_facts;
pub mod pure;
pub mod records;

pub use error::SplitzError;
pub use host_facts::HostFacts;
pub use pure::*;
pub use records::*;
