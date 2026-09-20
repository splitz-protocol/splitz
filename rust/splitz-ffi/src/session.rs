//! One device's view of its bills, as a foreign caller holds it.
//!
//! A session owns what SPEC.md §15 says a wallet supplies and nothing else. It
//! signs every entry this device writes with the account's own identity, so
//! §10.7 can bind a key to this participant on every other device.

use std::collections::BTreeSet;
use std::sync::Arc;

use serde_json::Value;
use splitz_core::host::{
    accept_scan, add_expense, amend_entry, confirm_payment, create_bill, has_joined, invite_for,
    join_bill, obligation_for, record_payment, set_rate, settle, shareable_bill, sign_entry,
    void_entry, BillHost, BillLog, Scanned, SignEntry, VerifyEntry,
};
use splitz_host::{
    activity_of, fold_verified, BillStore, Signer, SplitsKeys, SplitsSync, SwapWatch,
    SwapWatchList, WalletAccount, WalletBillHost,
};

use crate::convert;
use crate::error::{Result, SplitzError};
use crate::records as ffi;
use crate::seam::{
    BillStorage, BillStorageAdapter, RelayAdapter, SecretStore, SecretStoreAdapter, SenderAdapter,
    SplitsRelay, SplitsWallet, SwapProvider, WalletAdapter, WalletSender, ZecPrices,
};

/// One device's bills.
#[derive(uniffi::Object)]
pub struct SplitzSession {
    wallet: WalletAdapter,
    storage: BillStorageAdapter,
    relay: RelayAdapter,
    prices: Arc<dyn ZecPrices>,
    swaps: Arc<dyn SwapProvider>,
    /// This account's Ed25519 seed, read or minted when the session opened.
    /// §15.1: an identity that changed between runs would make this device a
    /// new participant on every bill it has already touched.
    seed: Vec<u8>,
}

#[uniffi::export]
impl SplitzSession {
    /// Opens a session, deriving or reading this account's signing identity.
    ///
    /// Named `new` so every binding spells it the way that language spells a
    /// constructor. A name of its own generates a static function instead,
    /// and `open` is a soft keyword in Kotlin.
    #[uniffi::constructor]
    pub fn new(
        wallet: Arc<dyn SplitsWallet>,
        secrets: Arc<dyn SecretStore>,
        storage: Arc<dyn BillStorage>,
        sender: Arc<dyn WalletSender>,
        relay: Arc<dyn SplitsRelay>,
        prices: Arc<dyn ZecPrices>,
        swaps: Arc<dyn SwapProvider>,
    ) -> Result<Arc<Self>> {
        let adapter = WalletAdapter {
            account: WalletAccount {
                id: wallet.account_id(),
                viewing_key: wallet.viewing_key(),
            },
            sender: SenderAdapter(sender),
            secrets: SecretStoreAdapter(secrets),
            wallet,
        };
        let seed =
            SplitsKeys::new(&adapter.secrets, &adapter).ensure_identity_seed(&adapter.account)?;
        Ok(Arc::new(Self {
            wallet: adapter,
            storage: BillStorageAdapter(storage),
            relay: RelayAdapter(relay),
            prices,
            swaps,
            seed,
        }))
    }

    /// The participant id every entry this device writes is authored by.
    pub fn account_id(&self) -> String {
        self.wallet.account.id.clone()
    }

    /// Whether this account's identity would survive a restore from its
    /// mnemonic. False when the seed was drawn at random for want of a viewing
    /// key — invisible in every signature it makes and decisive the day the
    /// device is replaced.
    pub fn identity_is_recoverable(&self) -> bool {
        self.wallet.account.identity_is_recoverable()
    }

    /// The public half other participants pin under §10.7.
    pub fn identity_key(&self) -> Result<String> {
        Signer
            .public_key_from_seed(&self.seed)
            .ok_or_else(|| SplitzError::Host {
                detail: "an identity seed is 32 bytes".to_owned(),
                transient: false,
            })
    }

    /// Every bill this device holds entries for.
    pub fn bill_ids(&self) -> Result<Vec<String>> {
        Ok(self.store().bill_ids()?)
    }

    /// Opens a bill, returning its §9.4 id.
    ///
    /// The bill key is minted here and never derived from the id: an id is
    /// what every invite hands out, and a key derived from it would be
    /// reproducible by everyone who ever saw one.
    pub fn create_bill(
        &self,
        name: String,
        currency: String,
        split_mode: String,
    ) -> Result<String> {
        let keys = self.keys();
        let placeholder = keys.generate_key();
        let entry = self.signed(|host| {
            create_bill(host, &name, &currency, &split_mode, &self.identity_key()?)
                .map_err(SplitzError::from)
        })?;
        let bill_id = entry_id(&entry)?;
        keys.store_bill_key(&bill_id, &placeholder)?;
        self.append(&bill_id, vec![entry])?;
        Ok(bill_id)
    }

    /// Joins `bill_id` under this account's own id and key.
    pub fn join_bill(&self, bill_id: String, name: String, pay_to: Option<String>) -> Result<()> {
        let key = self.identity_key()?;
        let entry = self.signed(|host| {
            join_bill(host, Some(&name), pay_to.as_deref(), Some(&key), None)
                .map_err(SplitzError::from)
        })?;
        self.append(&bill_id, vec![entry])
    }

    pub fn add_expense(
        &self,
        bill_id: String,
        expense_id: String,
        paid_by: String,
        amount: i64,
        split_json: String,
        description: Option<String>,
    ) -> Result<()> {
        let split = parse(&split_json, "a split")?;
        let entry = self.signed(|host| {
            add_expense(
                host,
                &expense_id,
                &paid_by,
                amount,
                split.clone(),
                description.as_deref(),
            )
            .map_err(SplitzError::from)
        })?;
        self.append(&bill_id, vec![entry])
    }

    /// Replaces an entry this device wrote, wholesale (§10.3).
    ///
    /// `member` names the payload's own member — `expense`, `participant`,
    /// `payment` — and must be the target's own kind: an amendment carrying
    /// another kind's payload would delete what it claims to correct.
    pub fn amend_entry(
        &self,
        bill_id: String,
        target_id: String,
        member: String,
        payload_json: String,
    ) -> Result<()> {
        let payload = parse(&payload_json, "an amendment payload")?;
        let entry = self.signed(|host| {
            amend_entry(host, &target_id, &member, payload.clone()).map_err(SplitzError::from)
        })?;
        self.append(&bill_id, vec![entry])
    }

    pub fn void_entry(&self, bill_id: String, target_id: String) -> Result<()> {
        let entry = self.signed(|host| void_entry(host, &target_id).map_err(SplitzError::from))?;
        self.append(&bill_id, vec![entry])
    }

    /// Records a claim that a debt was discharged. **A record is a claim**:
    /// §10.5 moves the balance only when the payee confirms.
    #[allow(clippy::too_many_arguments)]
    pub fn record_payment(
        &self,
        bill_id: String,
        payment_id: String,
        to: String,
        amount: i64,
        method: String,
        reference: Option<String>,
        zatoshi: Option<i64>,
        note: Option<String>,
    ) -> Result<()> {
        let entry = self.signed(|host| {
            record_payment(
                host,
                &payment_id,
                &to,
                amount,
                &method,
                reference.as_deref(),
                zatoshi,
                None,
                note.as_deref(),
            )
            .map_err(SplitzError::from)
        })?;
        self.append(&bill_id, vec![entry])
    }

    /// Confirms a payment to this device. **Only the payee confirms** — a
    /// payer who could confirm their own would settle a debt by asserting
    /// twice that they paid it.
    pub fn confirm_payment(
        &self,
        bill_id: String,
        payment_id: String,
        method: String,
        reference: Option<String>,
    ) -> Result<()> {
        let entry = self.signed(|host| {
            confirm_payment(host, &payment_id, &method, reference.as_deref())
                .map_err(SplitzError::from)
        })?;
        self.append(&bill_id, vec![entry])
    }

    /// Snapshots a rate onto the bill (§7). Every device then prices from the
    /// same figure rather than from whatever its own feed said.
    pub fn set_rate(
        &self,
        bill_id: String,
        currency: String,
        minor_units_per_zec: i64,
        source: Option<String>,
    ) -> Result<()> {
        let entry = self.signed(|host| {
            set_rate(host, &currency, minor_units_per_zec, source.as_deref())
                .map_err(SplitzError::from)
        })?;
        self.append(&bill_id, vec![entry])
    }

    /// The bill as it stands, with what the fold refused and who is contested.
    pub fn fold(&self, bill_id: String) -> Result<ffi::FoldedBill> {
        let entries = self.store().read(&bill_id)?;
        let folded = fold_verified(&self.wallet, &entries, Some(&self.seed)).map_err(|e| {
            SplitzError::Host {
                detail: e.to_string(),
                transient: false,
            }
        })?;
        Ok(convert::folded(&folded))
    }

    /// The log read as a history, newest first.
    pub fn history(&self, bill_id: String) -> Result<Vec<ffi::BillEvent>> {
        let entries = self.store().read(&bill_id)?;
        let folded = fold_verified(&self.wallet, &entries, Some(&self.seed)).map_err(|e| {
            SplitzError::Host {
                detail: e.to_string(),
                transient: false,
            }
        })?;
        Ok(
            activity_of(&entries, &folded.bill, &folded.set_aside, &folded.withdrawn)
                .iter()
                .map(convert::event)
                .collect(),
        )
    }

    /// What this device owes, and the §8 request that carries it.
    ///
    /// `None` when the bill carries no rate: an unpriced bill is an ordinary
    /// bill and nothing invents a price to avoid showing that.
    ///
    /// `pay_anyway` names the contested participants the payer has been shown
    /// and has chosen to pay regardless (§10.7).
    pub fn obligation(
        &self,
        bill_id: String,
        pay_anyway: Vec<String>,
    ) -> Result<Option<ffi::PayerObligation>> {
        let entries = self.store().read(&bill_id)?;
        let folded = fold_verified(&self.wallet, &entries, Some(&self.seed)).map_err(|e| {
            SplitzError::Host {
                detail: e.to_string(),
                transient: false,
            }
        })?;
        let host = self.host();
        let chosen: BTreeSet<String> = pay_anyway.into_iter().collect();
        Ok(obligation_for(&host, &folded, &chosen)?.map(|o| convert::obligation(&o)))
    }

    /// Whether this device has joined `bill_id` and may act on it.
    pub fn has_joined(&self, bill_id: String) -> Result<bool> {
        let entries = self.store().read(&bill_id)?;
        let folded = fold_verified(&self.wallet, &entries, Some(&self.seed)).map_err(|e| {
            SplitzError::Host {
                detail: e.to_string(),
                transient: false,
            }
        })?;
        Ok(has_joined(&self.host(), &folded.bill))
    }

    /// The invite URI for `bill_id` (§11.1).
    pub fn invite_for(&self, bill_id: String, name: Option<String>) -> Result<String> {
        let entries = self.store().read(&bill_id)?;
        let folded = fold_verified(&self.wallet, &entries, Some(&self.seed)).map_err(|e| {
            SplitzError::Host {
                detail: e.to_string(),
                transient: false,
            }
        })?;
        Ok(invite_for(
            &folded.bill,
            &self.require_key(&bill_id)?,
            name.as_deref(),
            None,
        )?)
    }

    /// The whole bill as one scanned payload (§11.2), or `None` when it will
    /// not fit in one. A caller shown `None` shares by relay instead.
    pub fn shareable_bill(&self, bill_id: String) -> Result<Option<String>> {
        let entries = self.store().read(&bill_id)?;
        let host = self.host();
        let log = BillLog::with_entries(&host, entries);
        let folded = log.fold()?;
        Ok(shareable_bill(
            &log,
            &self.require_key(&bill_id)?,
            &folded.bill,
        ))
    }

    /// Takes a scanned invite or payload into this device's own log.
    ///
    /// Returns the bill id it opened or added to.
    pub fn accept_scan(&self, text: String) -> Result<String> {
        match splitz_core::host::read_scan(&text) {
            // A §12 code, never a sentence: §1 leaves the wording to the
            // wallet.
            Scanned::Refused(code) => Err(SplitzError::Protocol {
                code: code.to_owned(),
                detail: "That code is not a bill or an invite".to_owned(),
            }),
            Scanned::Invite(invite) => {
                self.keys().store_bill_key(&invite.bill_id, &invite.key)?;
                Ok(invite.bill_id)
            }
            Scanned::Bill(scan) => {
                let invite = scan.invite.clone().ok_or_else(|| SplitzError::Host {
                    detail: "That payload carries no key, so it cannot be opened".to_owned(),
                    transient: false,
                })?;
                self.keys().store_bill_key(&invite.bill_id, &invite.key)?;
                let host = self.host();
                let mut log = BillLog::with_entries(&host, self.store().read(&invite.bill_id)?);
                accept_scan(&mut log, scan)?;
                self.append(&invite.bill_id, log.entries())?;
                Ok(invite.bill_id)
            }
        }
    }

    /// Pushes what this device holds, then pulls what it does not.
    pub fn sync(&self, bill_id: String) -> Result<ffi::SyncResult> {
        let storage = &self.storage;
        let store = BillStore::new(storage);
        let secrets = &self.wallet.secrets;
        let keys = SplitsKeys::new(secrets, &self.wallet);
        let sync = SplitsSync::new(&store, &keys, &self.relay);
        let result = sync.sync(&bill_id, Some(&self.seed), Some(&self.wallet.account.id))?;
        Ok(ffi::SyncResult {
            entry_count: result.entries.len() as u32,
            refused: result.refused.iter().map(convert::set_aside).collect(),
            unopenable: result.unopenable as u32,
        })
    }

    /// Sends this device's obligation and records what §14.3 says may be
    /// recorded.
    pub fn settle(&self, bill_id: String, pay_anyway: Vec<String>) -> Result<ffi::SettleResult> {
        let entries = self.store().read(&bill_id)?;
        let folded = fold_verified(&self.wallet, &entries, Some(&self.seed)).map_err(|e| {
            SplitzError::Host {
                detail: e.to_string(),
                transient: false,
            }
        })?;
        let host = self.host();
        let chosen: BTreeSet<String> = pay_anyway.into_iter().collect();
        let Some(obligation) = obligation_for(&host, &folded, &chosen)? else {
            return Err(SplitzError::Host {
                detail: "This bill carries no rate, so nothing can be sent".to_owned(),
                transient: false,
            });
        };
        let mut log = BillLog::with_entries(&host, entries);
        let settled = settle(&host, &mut log, &obligation)?;
        if !settled.records.is_empty() {
            self.append(&bill_id, settled.records.clone())?;
        }
        Ok(ffi::SettleResult {
            sent: matches!(settled.result, splitz_core::host::SendResult::Sent),
            pending: matches!(settled.result, splitz_core::host::SendResult::Pending),
            txid: settled.txid,
            detail: settled.detail,
            recorded: settled.records.len() as u32,
        })
    }

    /// The raw log, as the JSON §9.3 canonicalises. For a wallet that moves
    /// entries itself rather than through the relay.
    pub fn entries_json(&self, bill_id: String) -> Result<Vec<String>> {
        Ok(self
            .store()
            .read(&bill_id)?
            .iter()
            .map(Value::to_string)
            .collect())
    }

    /// Takes entries a peer handed over, reporting what §10.1 refused.
    pub fn add_entries(
        &self,
        bill_id: String,
        entries_json: Vec<String>,
    ) -> Result<Vec<ffi::SetAside>> {
        let mut entries = Vec::with_capacity(entries_json.len());
        for text in &entries_json {
            entries.push(parse(text, "an entry")?);
        }
        let merged = self.store().merge(&bill_id, entries)?;
        Ok(merged.refused.iter().map(convert::set_aside).collect())
    }

    /// What one ZEC costs, in the minor units of `currency`, or `None` when
    /// this build's source cannot price it.
    ///
    /// An unpriced bill is an ordinary bill: there is no §12 code for one, and
    /// nothing here invents a figure to avoid showing that state.
    pub fn price_of(&self, currency: String) -> Result<Option<i64>> {
        self.prices.minor_units_per_zec(currency)
    }

    /// Every asset this build's provider will deliver (§15.7).
    ///
    /// Read before quoting, so a payout naming an asset the provider does not
    /// carry is refused before a person is asked to send anything.
    pub fn swap_assets(&self) -> Result<Vec<ffi::TradableAsset>> {
        self.swaps.tradable_assets()
    }

    /// Quotes sending ZEC so that `recipient` is paid in `asset`.
    ///
    /// `refund_to` is the payer's own address and is where the ZEC goes back
    /// to if the swap fails. A quote arranged without one risks the deposit.
    pub fn swap_quote(
        &self,
        asset: ffi::TradableAsset,
        amount_in_zatoshi: i64,
        recipient: String,
        refund_to: String,
    ) -> Result<ffi::SwapQuote> {
        self.swaps
            .quote(asset, amount_in_zatoshi, recipient, refund_to)
    }

    /// What has happened to a swap.
    ///
    /// **Not a confirmation.** §10.5 says only the recipient settles a debt,
    /// and the transaction this reports is on the destination chain.
    pub fn swap_status(&self, quote: ffi::SwapQuote) -> Result<ffi::SwapStatus> {
        self.swaps.status_of(quote)
    }

    /// Keeps a swap this device sent, so its status can be asked for later.
    ///
    /// Local to this device, never sealed and never synced: a deposit address
    /// is one provider's routing detail for one swap, not something every
    /// participant should carry on the bill forever.
    pub fn watch_swap(&self, bill_id: String, to: String, quote: ffi::SwapQuote) -> Result<()> {
        SwapWatchList::new(&self.storage).add(&SwapWatch {
            bill_id,
            reference: quote
                .reference
                .clone()
                .unwrap_or_else(|| quote.deposit_address.clone()),
            to,
            deposit_address: quote.deposit_address.clone(),
            deposit_memo: quote.deposit_memo.clone(),
            asset_symbol: quote.asset.symbol.clone(),
            asset_chain: quote.asset.chain.clone(),
        })?;
        Ok(())
    }

    /// The swaps still being followed, for `bill_id` when given.
    pub fn watched_swaps(&self, bill_id: Option<String>) -> Result<Vec<ffi::SwapQuote>> {
        Ok(SwapWatchList::new(&self.storage)
            .held(bill_id.as_deref())?
            .iter()
            .map(|w| convert::quote(&w.as_quote()))
            .collect())
    }

    /// Stops following a swap. A list that only grows is one nobody reads.
    pub fn forget_swap(&self, reference: String) -> Result<()> {
        SwapWatchList::new(&self.storage).forget(&reference)?;
        Ok(())
    }

    /// Forgets a bill and the key it was sealed under.
    pub fn forget(&self, bill_id: String) -> Result<()> {
        self.store().forget(&bill_id)?;
        self.keys().forget_bill(&bill_id)?;
        Ok(())
    }
}

impl SplitzSession {
    fn store(&self) -> BillStore<'_> {
        BillStore::new(&self.storage)
    }

    fn keys(&self) -> SplitsKeys<'_> {
        SplitsKeys::new(&self.wallet.secrets, &self.wallet)
    }

    fn require_key(&self, bill_id: &str) -> Result<String> {
        match self.keys().read_bill_key(bill_id)? {
            Some(key) if !key.is_empty() => Ok(key),
            _ => Err(SplitzError::Host {
                detail: format!("No key for {bill_id}; it cannot be shared"),
                transient: false,
            }),
        }
    }

    fn host(&self) -> WalletBillHost<'_> {
        WalletBillHost::new(&self.wallet)
    }

    /// Builds an entry through a host that signs, then signs it.
    fn signed(&self, build: impl Fn(&dyn BillHost) -> Result<Value>) -> Result<Value> {
        let sign = |message: &[u8]| {
            Signer
                .sign(&self.seed, message)
                .expect("an identity seed is 32 bytes")
        };
        let sign_ref: SignEntry<'_> = &sign;
        let verify = |_: &Value, _: &str| false;
        let verify_ref: VerifyEntry<'_> = &verify;
        let host = WalletBillHost::new(&self.wallet)
            .signing_with(sign_ref)
            .verifying_with(verify_ref);
        let entry = build(&host)?;
        Ok(sign_entry(&host, &entry)?)
    }

    fn append(&self, bill_id: &str, entries: Vec<Value>) -> Result<()> {
        self.store().merge(bill_id, entries)?;
        Ok(())
    }
}

fn entry_id(entry: &Value) -> Result<String> {
    entry
        .get("id")
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| SplitzError::Host {
            detail: "an entry this device wrote carries no id".to_owned(),
            transient: false,
        })
}

fn parse(text: &str, what: &str) -> Result<Value> {
    serde_json::from_str(text).map_err(|e| SplitzError::Host {
        detail: format!("{what} is not JSON: {e}"),
        transient: false,
    })
}
