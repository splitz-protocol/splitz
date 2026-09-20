//! SPEC.md §15's interfaces, as a foreign caller implements them.
//!
//! One trait per §15 subsection, each exported so the wallet — Kotlin, Swift
//! or anything else uniffi reaches — supplies it. Every call is synchronous: a
//! foreign callback is, and nothing in this layer waits on two things at once.
//!
//! The traits are named exactly as §15 names them. The adapters below turn
//! each into the `splitz-host` trait of the same name, which differs only in
//! taking borrowed strings and returning this crate's error.

use std::sync::Arc;

use splitz_host::HostError;

use crate::error::SplitzError;
use crate::records::{SwapQuote, SwapStatus, TradableAsset, WalletSendOutcome};

fn host_error(e: SplitzError) -> HostError {
    match e {
        SplitzError::Protocol { code, detail } => HostError::Malformed(format!("{code}: {detail}")),
        SplitzError::Host { detail, transient } => HostError::Relay {
            message: detail,
            transient,
        },
    }
}

/// Specified in SPEC.md §15.3.
#[uniffi::export(foreign)]
pub trait SecretStore: Send + Sync {
    fn read(&self, key: String) -> Result<Option<String>, SplitzError>;
    fn write(&self, key: String, value: String) -> Result<(), SplitzError>;
    fn delete(&self, key: String) -> Result<(), SplitzError>;
}

/// Specified in SPEC.md §15.4.
#[uniffi::export(foreign)]
pub trait BillStorage: Send + Sync {
    fn read(&self, key: String) -> Result<Option<String>, SplitzError>;
    fn write(&self, key: String, value: String) -> Result<(), SplitzError>;
    fn delete(&self, key: String) -> Result<(), SplitzError>;
    fn keys(&self, prefix: String) -> Result<Vec<String>, SplitzError>;
    fn sweep_unfinished_writes(&self) -> Result<u32, SplitzError>;
}

/// Specified in SPEC.md §15.5.
#[uniffi::export(foreign)]
pub trait SplitsRelay: Send + Sync {
    fn push(&self, channel: String, blobs: Vec<String>) -> Result<(), SplitzError>;
    fn fetch(&self, channel: String) -> Result<Vec<String>, SplitzError>;
}

/// Specified in SPEC.md §15.2.
#[uniffi::export(foreign)]
pub trait WalletSender: Send + Sync {
    fn send(&self, payment_request_uri: String) -> WalletSendOutcome;
    fn pay_to_address(&self) -> Option<String>;
}

/// Specified in SPEC.md §15.6.
#[uniffi::export(foreign)]
pub trait ZecPrices: Send + Sync {
    fn minor_units_per_zec(&self, currency: String) -> Result<Option<i64>, SplitzError>;
}

/// Specified in SPEC.md §15.7.
#[uniffi::export(foreign)]
pub trait SwapProvider: Send + Sync {
    fn tradable_assets(&self) -> Result<Vec<TradableAsset>, SplitzError>;
    fn quote(
        &self,
        asset: TradableAsset,
        amount_in_zatoshi: i64,
        recipient: String,
        refund_to: String,
    ) -> Result<SwapQuote, SplitzError>;
    fn status_of(&self, quote: SwapQuote) -> Result<SwapStatus, SplitzError>;
}

/// Specified in SPEC.md §15.1.
///
/// `account_id` and `viewing_key` are read once, when a session is opened:
/// §15.1 requires the id to be stable for the life of an installed wallet, and
/// one fold must see one answer.
#[uniffi::export(foreign)]
pub trait SplitsWallet: Send + Sync {
    fn account_id(&self) -> String;
    fn viewing_key(&self) -> Option<String>;
    /// A §9.3 instant.
    fn now(&self) -> String;
    fn random_bytes(&self, byte_count: u32) -> Vec<u8>;
}

// --- the same interfaces, as `splitz-host` asks for them --------------------

pub(crate) struct SecretStoreAdapter(pub Arc<dyn SecretStore>);

impl splitz_host::SecretStore for SecretStoreAdapter {
    fn read(&self, key: &str) -> splitz_host::Result<Option<String>> {
        self.0.read(key.to_owned()).map_err(host_error)
    }

    fn write(&self, key: &str, value: &str) -> splitz_host::Result<()> {
        self.0
            .write(key.to_owned(), value.to_owned())
            .map_err(host_error)
    }

    fn delete(&self, key: &str) -> splitz_host::Result<()> {
        self.0.delete(key.to_owned()).map_err(host_error)
    }
}

pub(crate) struct BillStorageAdapter(pub Arc<dyn BillStorage>);

impl splitz_host::BillStorage for BillStorageAdapter {
    fn read(&self, key: &str) -> splitz_host::Result<Option<String>> {
        self.0.read(key.to_owned()).map_err(host_error)
    }

    fn write(&self, key: &str, value: &str) -> splitz_host::Result<()> {
        self.0
            .write(key.to_owned(), value.to_owned())
            .map_err(host_error)
    }

    fn delete(&self, key: &str) -> splitz_host::Result<()> {
        self.0.delete(key.to_owned()).map_err(host_error)
    }

    fn keys(&self, prefix: &str) -> splitz_host::Result<Vec<String>> {
        self.0.keys(prefix.to_owned()).map_err(host_error)
    }

    fn sweep_unfinished_writes(&self) -> splitz_host::Result<usize> {
        self.0
            .sweep_unfinished_writes()
            .map(|n| n as usize)
            .map_err(host_error)
    }
}

pub(crate) struct RelayAdapter(pub Arc<dyn SplitsRelay>);

impl splitz_host::SplitsRelay for RelayAdapter {
    fn push(&self, channel: &str, blobs: &[String]) -> splitz_host::Result<()> {
        self.0
            .push(channel.to_owned(), blobs.to_vec())
            .map_err(host_error)
    }

    fn fetch(&self, channel: &str) -> splitz_host::Result<Vec<String>> {
        self.0.fetch(channel.to_owned()).map_err(host_error)
    }
}

pub(crate) struct SenderAdapter(pub Arc<dyn WalletSender>);

impl splitz_host::WalletSender for SenderAdapter {
    fn send(&self, payment_request_uri: &str) -> splitz_host::WalletSendOutcome {
        self.0.send(payment_request_uri.to_owned()).into()
    }

    fn pay_to_address(&self) -> Option<String> {
        self.0.pay_to_address()
    }
}

/// The wallet, with the two values §15.1 fixes for a session read once.
pub(crate) struct WalletAdapter {
    pub wallet: Arc<dyn SplitsWallet>,
    pub account: splitz_host::WalletAccount,
    pub sender: SenderAdapter,
    pub secrets: SecretStoreAdapter,
}

impl splitz_host::SplitsWallet for WalletAdapter {
    fn account(&self) -> &splitz_host::WalletAccount {
        &self.account
    }

    fn sender(&self) -> &dyn splitz_host::WalletSender {
        &self.sender
    }

    fn secrets(&self) -> &dyn splitz_host::SecretStore {
        &self.secrets
    }

    fn now(&self) -> String {
        self.wallet.now()
    }

    fn random_bytes(&self, byte_count: usize) -> Vec<u8> {
        self.wallet.random_bytes(byte_count as u32)
    }
}

impl splitz_host::Randomness for WalletAdapter {
    fn bytes(&self, count: usize) -> Vec<u8> {
        self.wallet.random_bytes(count as u32)
    }
}
