//! Swaps this device sent and has not seen finish.
//!
//! A payment record carries the swap's `reference` and nothing else about the
//! provider (§9.2) — and that is deliberate: a deposit address is one
//! provider's routing detail for one swap, not something every participant
//! should carry on the bill forever.
//!
//! Following one up still needs it, so it is kept here instead: **local to the
//! device that sent the swap**, never sealed, never synced, and deleted once
//! the swap is done. Nobody else needs it and nobody else is told it.

use serde_json::{json, Value};

use crate::error::Result;
use crate::swaps::{SwapQuote, TradableAsset};
use crate::transport::component_encode;
use crate::wallet::BillStorage;

/// What this device needs to ask a provider how a swap went.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwapWatch {
    /// The bill the swap settles a debt on.
    pub bill_id: String,
    /// The payment record's `reference`, which is how the bill names it.
    pub reference: String,
    /// Who was owed.
    pub to: String,
    /// Where the ZEC was sent. The provider keys a status by this.
    pub deposit_address: String,
    /// Some chains lose a deposit sent without its memo, and a status query
    /// needs it too.
    pub deposit_memo: Option<String>,
    pub asset_symbol: String,
    pub asset_chain: String,
}

impl SwapWatch {
    pub fn to_json(&self) -> Value {
        let mut out = json!({
            "billId": self.bill_id,
            "reference": self.reference,
            "to": self.to,
            "depositAddress": self.deposit_address,
            "assetSymbol": self.asset_symbol,
            "assetChain": self.asset_chain,
        });
        if let Some(memo) = &self.deposit_memo {
            out["depositMemo"] = Value::from(memo.clone());
        }
        out
    }

    pub fn from_json(raw: &Value) -> Option<Self> {
        let text = |key: &str| raw.get(key).and_then(Value::as_str).map(str::to_owned);
        Some(Self {
            bill_id: text("billId")?,
            reference: text("reference")?,
            to: text("to")?,
            deposit_address: text("depositAddress")?,
            deposit_memo: text("depositMemo"),
            asset_symbol: text("assetSymbol").unwrap_or_default(),
            asset_chain: text("assetChain").unwrap_or_default(),
        })
    }

    /// The quote shape [`crate::wallet::SwapProvider::status_of`] asks for.
    ///
    /// Only the fields a status query reads are real; the amounts are not
    /// kept, because the bill already holds what was owed and what was sent
    /// and a second copy here would be a second thing to keep right.
    pub fn as_quote(&self) -> SwapQuote {
        SwapQuote {
            recipient: None,
            deposit_address: self.deposit_address.clone(),
            deposit_memo: self.deposit_memo.clone(),
            amount_in_zatoshi: 0,
            amount_out: String::new(),
            min_amount_out: None,
            asset: TradableAsset {
                asset_id: String::new(),
                symbol: self.asset_symbol.clone(),
                chain: self.asset_chain.clone(),
                decimals: 0,
            },
            // A watch is not a quote: nothing here is honoured, so the
            // deadline is the earliest instant and every reader sees it as
            // past.
            deadline: "0000-01-01T00:00:00.000Z".to_owned(),
            reference: Some(self.reference.clone()),
        }
    }
}

/// Keeps the swaps a device is still waiting on.
pub struct SwapWatchList<'a> {
    storage: &'a dyn BillStorage,
}

/// Namespaced away from bills so a sweep of one never reaches the other.
const PREFIX: &str = "swapwatch/";

impl<'a> SwapWatchList<'a> {
    pub fn new(storage: &'a dyn BillStorage) -> Self {
        Self { storage }
    }

    fn key(reference: &str) -> String {
        format!("{PREFIX}{}", component_encode(reference))
    }

    pub fn add(&self, watch: &SwapWatch) -> Result<()> {
        self.storage
            .write(&Self::key(&watch.reference), &watch.to_json().to_string())
    }

    /// Stops following `reference`. Called when a swap is confirmed or given
    /// up on: a list that only grows is one nobody reads.
    pub fn forget(&self, reference: &str) -> Result<()> {
        self.storage.delete(&Self::key(reference))
    }

    /// Everything still being followed, for `bill_id` when given.
    pub fn held(&self, bill_id: Option<&str>) -> Result<Vec<SwapWatch>> {
        let mut watches = Vec::new();
        for key in self.storage.keys(PREFIX)? {
            let Some(raw) = self.storage.read(&key)? else {
                continue;
            };
            // A damaged entry is skipped rather than refused on: it costs a
            // follow-up, not a bill.
            let Ok(decoded) = serde_json::from_str::<Value>(&raw) else {
                continue;
            };
            let Some(watch) = SwapWatch::from_json(&decoded) else {
                continue;
            };
            if bill_id.is_some_and(|id| watch.bill_id != id) {
                continue;
            }
            watches.push(watch);
        }
        Ok(watches)
    }
}
