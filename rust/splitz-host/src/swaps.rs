//! Settling a debt in an asset that is not ZEC (SPEC.md §9.2, `swap`).
//!
//! A recipient whose first payout preference is a `swap` cannot be an output
//! of a ZIP 321 request: §8.5 leaves them out and reports them. This is the
//! other half — a deposit address to send ZEC to, and a provider that delivers
//! the asset they asked for on the chain they named.
//!
//! **Nothing here names a provider, a host or a URL.** The endpoint, the
//! transport and the referral are the embedding wallet's, injected like the
//! relay's.
//!
//! **A swap is verifiable only in half** (§9.2). What leaves the payer's
//! wallet is ZEC and is recorded in `zatoshi`; what the recipient was owed
//! arrives as another asset on another chain, which the bill cannot see. A
//! caller MUST NOT present a swap as confirmed on the strength of the ZEC leg
//! alone — only the recipient can say they were paid (§10.5).

use crate::error::HostError;
use crate::wallet::SwapProvider;

/// An asset a provider will deliver, named by both halves.
///
/// **Asset and chain are read together, never separately.** One symbol exists
/// on many chains, and a swap matched on the symbol alone delivers the right
/// token to the wrong network, where the recipient cannot reach it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TradableAsset {
    /// The provider's own identifier, passed back verbatim.
    pub asset_id: String,
    /// What a person calls it — `USDC`. Not unique across chains.
    pub symbol: String,
    /// The network it is delivered on — `base`, `arb`. Not unique across
    /// symbols.
    pub chain: String,
    /// How many base units make one whole token, as a power of ten.
    pub decimals: i32,
}

impl TradableAsset {
    /// Whether this is what `symbol` on `chain` asked for, ignoring case.
    ///
    /// Both halves must match. A payout naming `USDC` on `base` is not
    /// satisfied by `USDC` on any other chain.
    pub fn answers(&self, wanted_symbol: &str, wanted_chain: &str) -> bool {
        self.symbol.eq_ignore_ascii_case(wanted_symbol)
            && self.chain.eq_ignore_ascii_case(wanted_chain)
    }
}

/// Where to send ZEC, and what the recipient gets for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwapQuote {
    /// The address the payer's ZEC goes to. **Not the recipient's address** —
    /// the provider's, for this one swap.
    pub deposit_address: String,
    /// Some chains need a memo alongside the address; sending without it loses
    /// the deposit.
    pub deposit_memo: Option<String>,
    /// What leaves the payer's wallet. This is the figure a payment record's
    /// `zatoshi` carries (§9.2).
    pub amount_in_zatoshi: i64,
    /// What the recipient receives, in the asset's base units.
    pub amount_out: String,
    pub asset: TradableAsset,
    /// After this the quote is not honoured and a new one is needed. A §9.3
    /// instant, so two devices read one moment.
    pub deadline: String,
    /// The provider's own identifier for this swap.
    ///
    /// This is what a payment record's `reference` carries (§9.2) — **not a
    /// Zcash txid**, and a reader that renders it as one is wrong for every
    /// swap. Absent when the provider names the swap only by its deposit
    /// address, in which case that is the reference.
    pub reference: Option<String>,
}

impl SwapQuote {
    /// Whether `now` is at or past the deadline.
    ///
    /// Both are §9.3 instants, which sort as text, so this is the same
    /// comparison §10.2 makes over a log.
    pub fn has_expired(&self, now: &str) -> bool {
        now >= self.deadline.as_str()
    }

    /// What a payment record should carry as its `reference` (§9.2).
    pub fn payment_reference(&self) -> &str {
        self.reference.as_deref().unwrap_or(&self.deposit_address)
    }
}

/// Where a swap has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapState {
    /// The provider has not seen the deposit.
    AwaitingDeposit,
    /// The deposit landed; the asset has not been delivered.
    Processing,
    /// The provider reports the recipient was paid. **Still not a
    /// confirmation**: §10.5 says only the recipient settles a debt.
    Delivered,
    /// The swap will not complete. The ZEC may have been refunded.
    Failed,
}

/// What a provider says about a swap in flight.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SwapStatus {
    pub state: SwapState,
    /// The transaction on the destination chain, when there is one. Named by
    /// the chain of the `swap` payout being settled, never by this one, so it
    /// MUST NOT be recorded as the §10.5 payment for a debt settled here.
    pub destination_tx_hash: Option<String>,
    /// What to put in front of a person. Present on [`SwapState::Failed`].
    pub detail: Option<String>,
}

/// A provider that arranges nothing, for a build configured with none.
///
/// Every call fails and says why. The alternative is a swap button that
/// silently does nothing.
#[derive(Debug, Clone, Copy, Default)]
pub struct UnconfiguredSwaps;

impl UnconfiguredSwaps {
    const WHY: &'static str = "This build has no swap provider configured, so a debt owed in \
                               another asset cannot be settled here.";
}

impl SwapProvider for UnconfiguredSwaps {
    fn tradable_assets(&self) -> Result<Vec<TradableAsset>, HostError> {
        Ok(Vec::new())
    }

    fn quote(
        &self,
        _asset: &TradableAsset,
        _amount_in_zatoshi: i64,
        _recipient: &str,
        _refund_to: &str,
    ) -> Result<SwapQuote, HostError> {
        Err(HostError::Swap {
            message: Self::WHY.to_owned(),
            transient: false,
        })
    }

    fn status_of(&self, _quote: &SwapQuote) -> Result<SwapStatus, HostError> {
        Err(HostError::Swap {
            message: Self::WHY.to_owned(),
            transient: false,
        })
    }
}
