//! Everything this crate needs from the wallet it is embedded in (SPEC.md §15).
//!
//! Declared here as traits rather than imported from an application. A feature
//! written inside a wallet reaches for whatever is next to it and then cannot
//! leave; nothing under `src/` names a wallet, and the compiler enforces that
//! by having nothing to name.

use crate::error::Result;

/// How a wallet's send ended.
///
/// Four states, not two. `PendingBroadcast` is a transaction that was built
/// and signed but not handed to the network: it may still land, so it is
/// neither paid nor unpaid. Collapsing it into either loses money — recorded
/// as paid, a transaction that never lands leaves a real debt showing as
/// settled; recorded as nothing, one that does land is paid a second time.
///
/// `Failed` and `Aborted` are §14.3's second outcome either way — nothing was
/// spent — and differ only in what a person is told.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalletSendPhase {
    Succeeded,
    PendingBroadcast,
    Failed,
    Aborted,
}

/// What a broadcast attempt reported.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WalletSendOutcome {
    pub phase: WalletSendPhase,
    /// Present when and only when `phase` is `Succeeded`. It becomes the id of
    /// the payment entry, so the record of a payment and the transaction that
    /// made it carry one identifier.
    pub txid: Option<String>,
    /// What to put in front of a person while the send is unresolved.
    pub status_message: Option<String>,
    /// What to put in front of a person when it failed.
    pub error: Option<String>,
}

/// The wallet's own send path.
///
/// Specified in SPEC.md §15.2.
pub trait WalletSender {
    /// Builds and signs one transaction paying every output in
    /// `payment_request_uri`, then broadcasts it.
    ///
    /// One call takes the whole ZIP 321 URI, which is why a payer who owes
    /// four people signs once. One transaction per recipient would cost four
    /// fees and four proofs, and would let a person walk away after the
    /// second.
    fn send(&self, payment_request_uri: &str) -> WalletSendOutcome;

    /// The address this device is paid at, or `None` when it has none.
    ///
    /// A participant with no address is reported as unpayable under §8.4
    /// rather than dropped from a settlement, so `None` is a state to show and
    /// not an error.
    fn pay_to_address(&self) -> Option<String>;
}

/// Where this crate's secrets live.
///
/// Specified in SPEC.md §15.3.
pub trait SecretStore {
    fn read(&self, key: &str) -> Result<Option<String>>;
    fn write(&self, key: &str, value: &str) -> Result<()>;
    fn delete(&self, key: &str) -> Result<()>;
}

/// Secrets held in memory only.
///
/// For tests, and for nothing else: §15.3 requires a value written to outlive
/// the process that wrote it, and a bill key that does not cannot decrypt that
/// bill tomorrow.
#[derive(Debug, Default)]
pub struct InMemorySecretStore {
    values: std::sync::Mutex<std::collections::BTreeMap<String, String>>,
}

impl SecretStore for InMemorySecretStore {
    fn read(&self, key: &str) -> Result<Option<String>> {
        Ok(self.values.lock().unwrap().get(key).cloned())
    }

    fn write(&self, key: &str, value: &str) -> Result<()> {
        self.values
            .lock()
            .unwrap()
            .insert(key.to_owned(), value.to_owned());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<()> {
        self.values.lock().unwrap().remove(key);
        Ok(())
    }
}

/// Durable storage for this device's bills, as raw entries.
///
/// Entries, not folded bills. §10.2 merges by set union, so a device holds
/// entries and derives everything else; a stored summary is a second source of
/// truth that goes stale without saying so.
///
/// Specified in SPEC.md §15.4.
pub trait BillStorage {
    fn read(&self, key: &str) -> Result<Option<String>>;
    fn write(&self, key: &str, value: &str) -> Result<()>;
    fn delete(&self, key: &str) -> Result<()>;
    fn keys(&self, prefix: &str) -> Result<Vec<String>>;

    /// Removes whatever a write that did not finish left behind, and returns
    /// how many. Zero for a store that cannot leave anything.
    ///
    /// Called once when the feature loads. A leftover is already invisible to
    /// [`BillStorage::keys`]; this stops them accumulating across the crashes
    /// of a year.
    fn sweep_unfinished_writes(&self) -> Result<usize>;
}

/// Storage that does not outlive the process. For tests.
#[derive(Debug, Default)]
pub struct InMemoryBillStorage {
    values: std::sync::Mutex<std::collections::BTreeMap<String, String>>,
}

impl BillStorage for InMemoryBillStorage {
    fn read(&self, key: &str) -> Result<Option<String>> {
        Ok(self.values.lock().unwrap().get(key).cloned())
    }

    fn write(&self, key: &str, value: &str) -> Result<()> {
        self.values
            .lock()
            .unwrap()
            .insert(key.to_owned(), value.to_owned());
        Ok(())
    }

    fn delete(&self, key: &str) -> Result<()> {
        self.values.lock().unwrap().remove(key);
        Ok(())
    }

    fn keys(&self, prefix: &str) -> Result<Vec<String>> {
        Ok(self
            .values
            .lock()
            .unwrap()
            .keys()
            .filter(|k| k.starts_with(prefix))
            .cloned()
            .collect())
    }

    /// Nothing to sweep: a map cannot be half written.
    fn sweep_unfinished_writes(&self) -> Result<usize> {
        Ok(0)
    }
}

/// Which wallet account is speaking, and what makes its identity recoverable.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WalletAccount {
    /// The participant id this device speaks as on every bill. Every entry it
    /// writes is authored by this id, and §10.4 decides what that authorises.
    pub id: String,
    /// A unified full viewing key, when the wallet can supply one.
    ///
    /// It is what makes this account's signing identity survive a reinstall: a
    /// viewing key is derived from the wallet seed, so the same mnemonic
    /// yields the same identity on a new device. An account identifier the
    /// wallet's own database assigns does not — it is handed out at import
    /// time — so an identity filed only under that is a stranger to every bill
    /// naming it after a restore.
    ///
    /// `None` means the identity is random and unrecoverable. It still signs
    /// correctly.
    pub viewing_key: Option<String>,
}

impl WalletAccount {
    /// Whether this account's identity would survive a restore from its
    /// mnemonic.
    ///
    /// False when the seed was drawn at random for want of a viewing key. The
    /// difference is invisible in every signature it makes and decisive the
    /// day the device is replaced, so it is reported rather than inferred.
    pub fn identity_is_recoverable(&self) -> bool {
        self.viewing_key.as_deref().is_some_and(|k| !k.is_empty())
    }
}

/// The wallet, as this crate needs it.
///
/// One trait rather than loose callbacks, so a host implements one thing and a
/// test fakes one thing.
///
/// Specified in SPEC.md §15.1.
pub trait SplitsWallet {
    fn account(&self) -> &WalletAccount;
    fn sender(&self) -> &dyn WalletSender;
    fn secrets(&self) -> &dyn SecretStore;

    /// A moment, taken from the wallet rather than from a clock this crate
    /// reads. §9.3 instants order a log, and a log that reorders between runs
    /// cannot be asserted.
    fn now(&self) -> String;

    /// Bytes nobody can predict. See [`Randomness`].
    fn random_bytes(&self, byte_count: usize) -> Vec<u8>;
}

/// Bytes nobody can predict.
///
/// §9.4 derives a bill's id from a nonce, so two bills created in the same
/// second by the same person are the same bill unless this is unpredictable.
/// The wallet supplies it because the wallet knows what secure randomness
/// means on its platform.
pub trait Randomness {
    fn bytes(&self, count: usize) -> Vec<u8>;
}

/// The platform's own entropy.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemRandomness;

impl Randomness for SystemRandomness {
    fn bytes(&self, count: usize) -> Vec<u8> {
        let mut out = vec![0u8; count];
        getrandom::fill(&mut out).expect("the platform has no entropy source");
        out
    }
}
