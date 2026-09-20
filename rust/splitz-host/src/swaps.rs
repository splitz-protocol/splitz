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

use std::cell::RefCell;

use serde_json::{json, Value};

use crate::error::HostError;
use crate::transport::{query_encode, HttpTransport};
use crate::wallet::SwapProvider;

fn swap_error(message: impl Into<String>, transient: bool) -> HostError {
    HostError::Swap {
        message: message.into(),
        transient,
    }
}

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

/// A [`SwapProvider`] speaking the 1Click request shape.
///
/// Four endpoints, relative to `origin`: `GET /v0/tokens`, `POST /v0/quote`,
/// `GET /v0/status`, `POST /v0/deposit/submit`. `origin` is the wallet's — a
/// provider's own host, or a proxy the wallet runs so no credential ships in
/// the app.
///
/// No credential is held here. A deployment needing one puts it behind its own
/// origin, which is why `origin` is required and has no default.
pub struct OneClickSwaps<'a> {
    origin: String,
    /// How the provider names ZEC. Read from its own token list rather than
    /// guessed; [`OneClickSwaps::tradable_assets`] is what a caller matches
    /// against.
    zec_asset_id: String,
    /// Identifies the integrator to the provider, where it asks for one.
    referral: Option<String>,
    transport: &'a dyn HttpTransport,
    /// How long the caller asks a quote to stand for, as a §9.3 instant.
    ///
    /// Supplied rather than computed: a clock and a calendar are the wallet's
    /// (§15.1), and this crate carries neither.
    deadline: &'a dyn Fn() -> String,
    tokens: RefCell<Option<Vec<TradableAsset>>>,
}

impl<'a> OneClickSwaps<'a> {
    /// `origin` is a scheme, a host and an optional path. A query or a
    /// fragment is refused: a path and a query are appended to it, and an
    /// origin carrying either would address something else.
    pub fn new(
        origin: &str,
        zec_asset_id: &str,
        transport: &'a dyn HttpTransport,
        deadline: &'a dyn Fn() -> String,
        referral: Option<&str>,
    ) -> Result<Self, HostError> {
        if origin.contains('?') || origin.contains('#') {
            return Err(swap_error(
                "A swap origin carries no query and no fragment",
                false,
            ));
        }
        Ok(Self {
            origin: origin.to_owned(),
            zec_asset_id: zec_asset_id.to_owned(),
            referral: referral.map(str::to_owned),
            transport,
            deadline,
            tokens: RefCell::new(None),
        })
    }

    fn url(&self, path: &str, query: &[(&str, &str)]) -> String {
        let mut url = format!("{}{path}", self.origin);
        for (i, (key, value)) in query.iter().enumerate() {
            url.push(if i == 0 { '?' } else { '&' });
            url.push_str(&query_encode(key));
            url.push('=');
            url.push_str(&query_encode(value));
        }
        url
    }

    /// Reads a response body, or says why the provider could not be reached.
    fn read(&self, body: Result<String, String>, what: &str) -> Result<Value, HostError> {
        // The transport is the wallet's, so its failures are its own. A
        // network fault is retryable; nothing here can tell which, so the
        // caller is told it may be.
        let text = body.map_err(|e| {
            swap_error(format!("The swap provider could not be reached: {e}"), true)
        })?;
        serde_json::from_str(&text)
            .map_err(|_| swap_error(format!("The {what} response is not JSON"), false))
    }
}

fn required(object: &Value, key: &str) -> Result<String, HostError> {
    match object.get(key).and_then(Value::as_str) {
        Some(value) if !value.is_empty() => Ok(value.to_owned()),
        _ => Err(swap_error(format!("The provider omitted {key}"), false)),
    }
}

fn optional(object: &Value, key: &str) -> Option<String> {
    object
        .get(key)
        .and_then(Value::as_str)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

fn asset_from(token: &Value) -> Result<TradableAsset, HostError> {
    Ok(TradableAsset {
        asset_id: required(token, "assetId")?,
        symbol: required(token, "symbol")?,
        chain: required(token, "blockchain")?,
        decimals: token.get("decimals").and_then(Value::as_i64).unwrap_or(0) as i32,
    })
}

/// The provider's own vocabulary, mapped onto §9.2's answers.
///
/// **An unrecognised status is [`SwapState::Processing`], never
/// [`SwapState::Delivered`].** Reading an unknown word as success would tell a
/// payer their debt is settled on the strength of a string nobody here has
/// defined.
fn state_of(status: &str) -> SwapState {
    match status {
        "PENDING_DEPOSIT" | "KNOWN_DEPOSIT_TX" => SwapState::AwaitingDeposit,
        "SUCCESS" => SwapState::Delivered,
        "FAILED" | "REFUNDED" | "EXPIRED" => SwapState::Failed,
        _ => SwapState::Processing,
    }
}

impl SwapProvider for OneClickSwaps<'_> {
    fn tradable_assets(&self) -> Result<Vec<TradableAsset>, HostError> {
        if let Some(cached) = self.tokens.borrow().as_ref() {
            return Ok(cached.clone());
        }
        let body = self.read(self.transport.get(&self.url("/v0/tokens", &[])), "tokens")?;
        let raw = if body.is_array() {
            body.clone()
        } else {
            body.get("tokens").cloned().unwrap_or(Value::Null)
        };
        let Some(list) = raw.as_array() else {
            return Err(swap_error("The provider listed no tokens", false));
        };
        let tokens: Vec<TradableAsset> = list
            .iter()
            .filter(|t| t.is_object())
            .map(asset_from)
            .collect::<Result<_, _>>()?;
        *self.tokens.borrow_mut() = Some(tokens.clone());
        Ok(tokens)
    }

    fn quote(
        &self,
        asset: &TradableAsset,
        amount_in_zatoshi: i64,
        recipient: &str,
        refund_to: &str,
    ) -> Result<SwapQuote, HostError> {
        if amount_in_zatoshi <= 0 {
            return Err(swap_error("A swap sends more than nothing", false));
        }
        if recipient.is_empty() || refund_to.is_empty() {
            // A quote with no refund address risks the whole deposit if the
            // swap fails, which is the one failure the payer cannot recover
            // from.
            return Err(swap_error(
                "A swap states both who receives it and where a refund goes",
                false,
            ));
        }
        let deadline = (self.deadline)();
        let mut request = json!({
            "dry": false,
            "swapType": "EXACT_INPUT",
            "originAsset": self.zec_asset_id,
            "depositType": "ORIGIN_CHAIN",
            "destinationAsset": asset.asset_id,
            "amount": amount_in_zatoshi.to_string(),
            "refundTo": refund_to,
            "refundType": "ORIGIN_CHAIN",
            "recipient": recipient,
            "recipientType": "DESTINATION_CHAIN",
            "deadline": deadline,
            "depositMode": "SIMPLE",
        });
        if let Some(referral) = self.referral.as_deref().filter(|r| !r.is_empty()) {
            request["referral"] = Value::from(referral);
        }

        let body = self.read(
            self.transport
                .post(&self.url("/v0/quote", &[]), &request.to_string()),
            "quote",
        )?;
        if !body.is_object() {
            return Err(swap_error("Malformed quote response", false));
        }
        let Some(quote) = body.get("quote").filter(|q| q.is_object()) else {
            return Err(swap_error("A quote response carries a quote", false));
        };
        Ok(SwapQuote {
            deposit_address: required(quote, "depositAddress")?,
            deposit_memo: optional(quote, "depositMemo"),
            amount_in_zatoshi,
            amount_out: required(quote, "amountOut")?,
            asset: asset.clone(),
            // The provider's own deadline where it states one: honouring a
            // longer one of ours would quote a price it has stopped holding.
            deadline: optional(quote, "deadline")
                .and_then(|raw| splitz_core::canonical_instant(&raw).ok())
                .unwrap_or(deadline),
            reference: optional(&body, "correlationId"),
        })
    }

    fn status_of(&self, quote: &SwapQuote) -> Result<SwapStatus, HostError> {
        let mut query: Vec<(&str, &str)> = vec![("depositAddress", &quote.deposit_address)];
        if let Some(memo) = quote.deposit_memo.as_deref().filter(|m| !m.is_empty()) {
            query.push(("depositMemo", memo));
        }
        let body = self.read(
            self.transport.get(&self.url("/v0/status", &query)),
            "status",
        )?;
        if !body.is_object() {
            return Err(swap_error("Malformed status response", false));
        }
        let status = body
            .get("status")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_uppercase();
        Ok(SwapStatus {
            state: state_of(&status),
            destination_tx_hash: optional(&body, "destinationTxHash")
                .or_else(|| optional(&body, "destinationChainTxHash")),
            detail: optional(&body, "message"),
        })
    }
}
