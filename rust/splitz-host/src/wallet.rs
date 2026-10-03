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
    /// Present when `phase` is `Succeeded`: it becomes the reference of the
    /// payment entry, so the record of a payment and the transaction that made
    /// it carry one identifier.
    ///
    /// When `phase` is `PendingBroadcast`, the transaction the wallet built and
    /// may still broadcast, when it knows it. Nothing is recorded from it; a
    /// person looks it up to learn which way the send went.
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

/// `inner`, scoped to one wallet account: every name is `<name>@<account>`.
///
/// A bill key is named by its bill alone, so on a store several accounts
/// share, one account forgetting a bill deletes the key every other account
/// opens it with (§15.3). One of these per account keeps them apart.
pub struct AccountSecretStore<'a> {
    inner: &'a dyn SecretStore,
    account: String,
}

impl<'a> AccountSecretStore<'a> {
    /// Refuses an empty account, which would scope nothing.
    pub fn new(inner: &'a dyn SecretStore, account: &str) -> Result<Self> {
        if account.is_empty() {
            return Err(crate::error::HostError::Malformed(
                "an account is named".to_owned(),
            ));
        }
        Ok(Self {
            inner,
            account: account.to_owned(),
        })
    }

    fn scoped(&self, key: &str) -> String {
        format!("{key}@{}", self.account)
    }
}

impl SecretStore for AccountSecretStore<'_> {
    fn read(&self, key: &str) -> Result<Option<String>> {
        self.inner.read(&self.scoped(key))
    }
    fn write(&self, key: &str, value: &str) -> Result<()> {
        self.inner.write(&self.scoped(key), value)
    }
    fn delete(&self, key: &str) -> Result<()> {
        self.inner.delete(&self.scoped(key))
    }
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

/// A dumb store of ciphertext blobs, grouped into per-bill channels.
///
/// It moves bytes for the participant who left before dessert and cannot be
/// handed a code across the table. It holds no key, so every blob is opaque.
/// What it can observe is deliberately the minimum — that a channel has some
/// blobs, how large they are, and when they last changed — and never who owes
/// whom.
///
/// Optional, and meant to stay so: a bill works with no relay at all.
///
/// Specified in SPEC.md §15.5.
pub trait SplitsRelay {
    /// Adds `blobs` to `channel`. Pushing a blob already present is a no-op,
    /// so a retry after a dropped connection cannot create duplicates.
    fn push(&self, channel: &str, blobs: &[String]) -> Result<()>;

    /// Every blob currently held for `channel`.
    ///
    /// The caller opens and merges by entry id, and merging is idempotent, so
    /// returning blobs it already holds is harmless — the relay is not asked
    /// to track per-caller state it has no identity to key on.
    fn fetch(&self, channel: &str) -> Result<Vec<String>>;
}

/// A source of ZEC prices.
///
/// Injected, like everything else a wallet already has: a wallet showing
/// balances in a currency has a price feed, and a second one here would be a
/// second answer on one screen.
///
/// Specified in SPEC.md §15.6.
pub trait ZecPrices {
    /// Minor units of `currency` that one ZEC costs, or `None` when this
    /// source cannot price it.
    ///
    /// Minor units, not a decimal. §7 snapshots the figure onto the bill as an
    /// integer, so rounding it happens once, here, where the source's
    /// precision is known — rather than at every place that reads it.
    ///
    /// `None` is an ordinary answer. A bill with no rate is an ordinary bill:
    /// there is no §12 code for unpriced, and nothing should invent a price to
    /// avoid showing that state.
    fn minor_units_per_zec(&self, currency: &str) -> Result<Option<i64>>;
}

/// Arranges swaps off this chain.
///
/// Specified in SPEC.md §15.7.
pub trait SwapProvider {
    /// Every asset this provider will deliver.
    ///
    /// Read before quoting, so a payout naming an asset the provider does not
    /// carry is refused before a person is asked to send anything.
    fn tradable_assets(&self) -> Result<Vec<crate::swaps::TradableAsset>>;

    /// Quotes sending `amount_in_zatoshi` of ZEC so that `recipient` is paid
    /// in `asset`.
    ///
    /// `refund_to` is where the ZEC goes back to if the swap fails, and is the
    /// payer's own address. A quote with no refund address risks the deposit.
    fn quote(
        &self,
        asset: &crate::swaps::TradableAsset,
        amount_in_zatoshi: i64,
        recipient: &str,
        refund_to: &str,
    ) -> Result<crate::swaps::SwapQuote>;

    /// What has happened to the swap `quote` arranged.
    fn status_of(&self, quote: &crate::swaps::SwapQuote) -> Result<crate::swaps::SwapStatus>;
}

/// Which wallet account is speaking, and what makes its identity recoverable.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct WalletAccount {
    /// The participant id this device speaks as on every bill. Every entry it
    /// writes is authored by this id, and §10.4 decides what that authorises.
    pub id: String,
    /// Bytes derived from the account's spending secret, when the wallet
    /// holds one — for a software wallet, its mnemonic and passphrase.
    ///
    /// It makes this account's signing identity survive a reinstall: the same
    /// mnemonic yields the same identity on a new device. An account
    /// identifier the wallet's own database assigns does not — it is handed
    /// out at import time — so an identity filed only under that is a
    /// stranger to every bill naming it after a restore.
    ///
    /// It MUST NOT be anything the wallet shows or shares: whoever holds it
    /// holds the identity, and can write entries that bind as this
    /// participant and redirect what they are paid. A viewing key is exactly
    /// such a thing.
    ///
    /// `None` — a hardware account keeps no secret on the phone — means the
    /// identity is random and unrecoverable. It still signs correctly.
    pub identity_secret: Option<Vec<u8>>,
}

impl WalletAccount {
    /// Whether this account's identity would survive a restore from its
    /// mnemonic.
    ///
    /// False when the seed was drawn at random for want of a secret. The
    /// difference is invisible in every signature it makes and decisive the
    /// day the device is replaced, so it is reported rather than inferred.
    pub fn identity_is_recoverable(&self) -> bool {
        self.identity_secret
            .as_deref()
            .is_some_and(|s| !s.is_empty())
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
