//! The layer between the splitz protocol and a wallet's screens.
//!
//! `splitz-core` decides what a bill is, what anyone owes and which payment
//! request settles it. This crate is what a wallet needs around that: the seam
//! it plugs into (SPEC.md §15), Ed25519 entry signing, sealing entries for a
//! transport, the log a device keeps, and the sync that moves a bill between
//! devices.
//!
//! It names no wallet and depends on none. Everything a wallet supplies is a
//! trait, and every one of them is synchronous: a foreign binding carries a
//! synchronous callback into Kotlin and Swift, and nothing here needs to wait
//! on two things at once.

pub mod error;
pub mod fold;
pub mod keys;
pub mod signing;
pub mod wallet;
pub mod wallet_bill_host;

pub use error::{HostError, Result};
pub use fold::{fold_unverified, fold_verified, FoldFailure};
pub use keys::{is_well_formed_key, SplitsKeys, IDENTITY_DOMAIN, KEY_LENGTH_BYTES};
pub use signing::{base64url_decode, base64url_encode, Signer, VerifiedLog, SEED_BYTES};
pub use wallet::{
    InMemorySecretStore, Randomness, SecretStore, SplitsWallet, SystemRandomness, WalletAccount,
    WalletSendOutcome, WalletSendPhase, WalletSender,
};
pub use wallet_bill_host::WalletBillHost;
