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
use splitz_core::host::{FoldedBill, PayerObligation};
use splitz_core::{fiat_to_zatoshi, Bill, Payout, RateRounding};

use crate::error::HostError;
use crate::transport::{query_encode, HttpTransport};
use crate::wallet::SwapProvider;

/// The body of a swap provider's answer, refusing a status it uses to say no.
///
/// A 4xx or 5xx carries a body too, and decoding it as a quote would read an
/// error object as a price, so the status decides before the bytes are read.
/// The provider's own `message`, when it sends one, goes into the refusal: it
/// names what to change. A 5xx is transient; a 4xx is a refusal on the
/// merits. Bytes that are not UTF-8 are replaced rather than refused.
pub fn swap_answer(status: u16, body: &[u8]) -> Result<String, HostError> {
    let body = String::from_utf8_lossy(body).into_owned();
    if status >= 400 {
        let said = provider_message(&body);
        return Err(HostError::Swap {
            message: match said {
                Some(said) => format!("The swap provider answered {status}: {said}"),
                None => format!("The swap provider answered {status}"),
            },
            transient: status >= 500,
        });
    }
    Ok(body)
}

/// The `message` of an error body, or `None` when there is none to read: a
/// string, or a list of strings joined, cut at 200 characters so a verbose
/// provider cannot fill a screen.
fn provider_message(body: &str) -> Option<String> {
    let decoded: Value = serde_json::from_str(body).ok()?;
    let text = match decoded.as_object()?.get("message")? {
        Value::String(s) => s.clone(),
        Value::Array(items) => items
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join("; "),
        _ => String::new(),
    };
    let text = text.trim();
    if text.is_empty() {
        return None;
    }
    let cut: String = text.chars().take(200).collect();
    Some(if cut.len() < text.len() {
        format!("{cut}…")
    } else {
        cut
    })
}

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
    /// The payout address the provider delivers to: what this quote was
    /// taken for. A deposit is refused once the payee's payout no longer names
    /// it — otherwise the money goes to an address they replaced. `None` for
    /// a quote rebuilt to follow a swap already sent.
    pub recipient: Option<String>,
    /// Some chains need a memo alongside the address; sending without it loses
    /// the deposit.
    pub deposit_memo: Option<String>,
    /// What leaves the payer's wallet. This is the figure a payment record's
    /// `zatoshi` carries (§9.2).
    pub amount_in_zatoshi: i64,
    /// What the provider quotes the recipient receives, in the asset's base
    /// units. Up to the slippage less may arrive; `min_amount_out` is the
    /// floor.
    pub amount_out: String,
    /// The least the recipient receives once slippage is applied, in the
    /// asset's base units, or `None` when the provider states none.
    pub min_amount_out: Option<String>,
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

/// Where `payout` sits in `payouts`, or `None` when none of them is it.
///
/// A payout is matched on its type, address, asset and chain together, never
/// on its position: a payee who edits their list moves every position after
/// the edit, and an index kept across that pays an address nobody picked
/// (§14.8).
pub fn declared_payout_index(payouts: &[Payout], payout: &Payout) -> Option<usize> {
    payouts.iter().position(|p| p == payout)
}

/// Why a swap's deposit may not be sent, as [`swap_send_refusal`] reads it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SwapSendRefusal {
    /// The quote's deadline has passed.
    Expired,
    /// The deposit needs a memo, and a payment request carries none.
    NeedsMemo,
    /// The payee no longer declares the payout the quote was asked for.
    PayoutGone,
    /// A payment this payer sent and nobody has confirmed covers the debt
    /// (§14.4); `paid_to` must confirm it, in ascending id order.
    Held { paid_to: Vec<String> },
    /// The bill no longer says this payer owes the quoted amount to the payee
    /// as a debt a payment request leaves out.
    NotOwed,
    /// The payee's payout no longer names the address the quote delivers to.
    RecipientChanged,
    /// The payee's payout no longer names the asset and chain the quote buys.
    AssetChanged,
    /// The bill's rate no longer converts the debt to the quote's ZEC.
    RateChanged,
}

/// Whether `quote`'s deposit may be sent to pay `to` `amount_minor_units` on
/// `bill`, or `None` when it may (§15.7).
///
/// `now` is a §9.3 instant. `bill` is the bill as the store holds it at the
/// moment of sending, and `obligation` is this payer's obligation read from
/// it with `chosen` as the payout for `to` (§14.8), or `None` when the bill
/// has no rate. `chosen` is the payout the quote was asked for; `None` means
/// the payee's first.
///
/// Checked in this order, the first that applies answering: the quote has
/// expired; its deposit needs a memo, which a payment request cannot carry
/// and without which the deposit is lost; `chosen` is no longer declared; a
/// payment this payer already sent covers the debt (§14.4), so a second
/// deposit would pay it twice; the debt is no longer owed in exactly
/// `amount_minor_units`; the payout's address is not the quote's
/// `recipient`, so the deposit pays an address the payee replaced; its asset
/// or chain is not the quote's, so one address may be a different account or
/// none; and the bill's rate no longer converts `amount_minor_units` to the
/// quote's `amount_in_zatoshi`, so the deposit is sized by a rate the record
/// does not state.
///
/// Refused as [`fiat_to_zatoshi`] refuses when the bill's rate cannot price
/// its own currency.
pub fn swap_send_refusal(
    quote: &SwapQuote,
    now: &str,
    bill: &Bill,
    obligation: Option<&PayerObligation>,
    to: &str,
    amount_minor_units: i64,
    chosen: Option<&Payout>,
) -> splitz_core::Result<Option<SwapSendRefusal>> {
    if quote.has_expired(now) {
        return Ok(Some(SwapSendRefusal::Expired));
    }
    if quote.deposit_memo.as_deref().is_some_and(|m| !m.is_empty()) {
        return Ok(Some(SwapSendRefusal::NeedsMemo));
    }
    let payouts = bill.participant(to).map_or(&[][..], |p| &p.payouts[..]);
    let payout = match chosen {
        None => payouts.first(),
        Some(chosen) => match declared_payout_index(payouts, chosen) {
            None => return Ok(Some(SwapSendRefusal::PayoutGone)),
            Some(at) => payouts.get(at),
        },
    };
    if let Some(waiting) = obligation.and_then(|o| o.awaiting.iter().find(|a| a.to == to)) {
        return Ok(Some(SwapSendRefusal::Held {
            paid_to: waiting.paid_to.clone(),
        }));
    }
    let owed = obligation.is_some_and(|o| {
        o.request
            .unpayable
            .iter()
            .any(|u| u.id == to && u.minor_units == amount_minor_units)
    });
    if !owed {
        return Ok(Some(SwapSendRefusal::NotOwed));
    }
    let Some(payout) = payout.filter(|p| p.address.is_some()) else {
        return Ok(Some(SwapSendRefusal::RecipientChanged));
    };
    if quote.recipient != payout.address {
        return Ok(Some(SwapSendRefusal::RecipientChanged));
    }
    match (&payout.asset, &payout.chain) {
        (Some(asset), Some(chain)) if quote.asset.answers(asset, chain) => {}
        _ => return Ok(Some(SwapSendRefusal::AssetChanged)),
    }
    let Some(rate) = &bill.rate else {
        return Ok(Some(SwapSendRefusal::RateChanged));
    };
    let zatoshi = fiat_to_zatoshi(
        amount_minor_units,
        rate,
        Some(&bill.currency),
        RateRounding::Up,
    )?;
    if quote.amount_in_zatoshi != zatoshi {
        return Ok(Some(SwapSendRefusal::RateChanged));
    }
    Ok(None)
}

/// The entries to withdraw once the swap `reference` names is reported
/// failed: the ids of the entries that recorded `me`'s unconfirmed `swap`
/// payments carrying that reference, in the order `folded` lists the payments
/// (§15.7).
///
/// While such a record stands the debt reads as paid and waiting and nothing
/// can pay it again; its author may withdraw it (§10.8). A confirmed record
/// is left: the payee has said the money arrived. A record somebody else
/// wrote is theirs to withdraw.
pub fn failed_swap_withdrawals(folded: &FoldedBill, me: &str, reference: &str) -> Vec<String> {
    let bill = &folded.bill;
    bill.payments
        .iter()
        .filter(|p| {
            p.method == "swap"
                && p.reference.as_deref() == Some(reference)
                && p.from == me
                && !bill.confirmed_payments.contains(&p.id)
        })
        .filter_map(|p| folded.payment_entries.get(&p.id).cloned())
        .collect()
}

/// Where a swap has got to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapState {
    /// The provider has not seen the deposit.
    AwaitingDeposit,
    /// The deposit landed; the asset has not been delivered.
    Processing,
    /// The swap will not complete and the provider has begun returning the
    /// ZEC to the refund address. Not finished: [`SwapState::Failed`] follows
    /// once it has.
    Refunding,
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

// --- the 1Click shape, as values rather than calls -------------------------
//
// A provider is reached over HTTP, and HTTP is the wallet's. What is not the
// wallet's is the shape of the request and what the answer means, so those are
// functions over text: a caller that makes its own request still reads the
// answer the way every other implementation does.

/// The body a quote request carries.
///
/// `deadline` is a §9.3 instant the caller chose: a clock and a calendar are
/// the wallet's (§15.1) and this crate carries neither.
pub fn quote_request_body(
    zec_asset_id: &str,
    asset: &TradableAsset,
    amount_in_zatoshi: i64,
    recipient: &str,
    refund_to: &str,
    deadline: &str,
    referral: Option<&str>,
) -> Result<String, HostError> {
    if amount_in_zatoshi <= 0 {
        return Err(swap_error("A swap sends more than nothing", false));
    }
    if recipient.is_empty() || refund_to.is_empty() {
        // A quote with no refund address risks the whole deposit if the swap
        // fails, which is the one failure the payer cannot recover from.
        return Err(swap_error(
            "A swap states both who receives it and where a refund goes",
            false,
        ));
    }
    let mut request = json!({
        "dry": false,
        "swapType": "EXACT_INPUT",
        "slippageTolerance": OneClickSwaps::SLIPPAGE_BASIS_POINTS,
        "originAsset": zec_asset_id,
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
    if let Some(referral) = referral.filter(|r| !r.is_empty()) {
        request["referral"] = Value::from(referral);
    }
    Ok(request.to_string())
}

/// Every asset a provider's token list says it will deliver.
pub fn assets_from_tokens(body: &str) -> Result<Vec<TradableAsset>, HostError> {
    let body = decode_body(body, "tokens")?;
    let raw = if body.is_array() {
        body.clone()
    } else {
        body.get("tokens").cloned().unwrap_or(Value::Null)
    };
    let Some(list) = raw.as_array() else {
        return Err(swap_error("The provider listed no tokens", false));
    };
    list.iter()
        .filter(|t| t.is_object())
        .map(asset_from)
        .collect()
}

/// The quote a provider's answer states, once it is shown to answer
/// `request` — the body [`quote_request_body`] produced and the wallet posted.
///
/// The provider echoes the request it quoted (`quoteRequest`, required by its
/// schema). A quote for another recipient, asset or amount delivers somebody
/// else's money, or this payer's to somebody else, and nothing downstream
/// would notice, so a difference in any of them is a refusal.
pub fn quote_from_response(
    body: &str,
    request: &str,
    asset: &TradableAsset,
    amount_in_zatoshi: i64,
    asked_deadline: &str,
) -> Result<SwapQuote, HostError> {
    let body = decode_body(body, "quote")?;
    if !body.is_object() {
        return Err(swap_error("Malformed quote response", false));
    }
    let Some(quote) = body.get("quote").filter(|q| q.is_object()) else {
        return Err(swap_error("A quote response carries a quote", false));
    };
    let asked = decode_body(request, "quote request")?;
    let Some(echoed) = body.get("quoteRequest").filter(|q| q.is_object()) else {
        return Err(swap_error(
            "A quote response carries the request it quotes",
            false,
        ));
    };
    for field in [
        "recipient",
        "destinationAsset",
        "originAsset",
        "amount",
        "refundTo",
        "swapType",
    ] {
        if echoed.get(field) != asked.get(field) {
            return Err(swap_error(
                format!("The provider quoted a different {field}"),
                false,
            ));
        }
    }
    if quote.get("amountIn").and_then(Value::as_str) != Some(&amount_in_zatoshi.to_string()) {
        return Err(swap_error(
            "The provider quoted a different amount in",
            false,
        ));
    }
    // The floor is what the recipient is guaranteed. One below the tolerance
    // asked for lets the provider keep more than the payer agreed to lose, and
    // the record still says the whole debt was paid.
    if let Some(floor) = optional(quote, "minAmountOut") {
        // Base units are decimal digits and nothing else: `parse::<u128>`
        // alone takes `+15`, which no other reader does.
        let digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
        let amount_out = required(quote, "amountOut")?;
        let out = digits(&amount_out)
            .then(|| amount_out.parse::<u128>().ok())
            .flatten();
        let least = digits(&floor).then(|| floor.parse::<u128>().ok()).flatten();
        let (Some(out), Some(least)) = (out, least) else {
            return Err(swap_error(
                "The provider quoted amounts out of shape",
                false,
            ));
        };
        let tolerated = (10_000 - OneClickSwaps::SLIPPAGE_BASIS_POINTS).unsigned_abs() as u128;
        // The provider rounds its floor down, so the bound is the product
        // rounded down too: 15150548 at 1% is 14999042.52, stated 14999042.
        let short = match out.checked_mul(tolerated) {
            Some(o) => least < o / 10_000,
            None => true,
        };
        if short {
            return Err(swap_error(
                "The provider guarantees less than the tolerance asked for",
                false,
            ));
        }
    }
    Ok(SwapQuote {
        deposit_address: required(quote, "depositAddress")?,
        recipient: asked
            .get("recipient")
            .and_then(Value::as_str)
            .map(str::to_owned),
        deposit_memo: optional(quote, "depositMemo"),
        amount_in_zatoshi,
        amount_out: required(quote, "amountOut")?,
        min_amount_out: optional(quote, "minAmountOut"),
        asset: asset.clone(),
        // The provider's own deadline where it states one: honouring a longer
        // one of ours would quote a price it has stopped holding.
        deadline: optional(quote, "deadline")
            .and_then(|raw| splitz_core::canonical_instant(&raw).ok())
            .unwrap_or_else(|| asked_deadline.to_owned()),
        reference: optional(&body, "correlationId"),
    })
}

/// What a provider's status answer means.
pub fn status_from_response(body: &str) -> Result<SwapStatus, HostError> {
    let body = decode_body(body, "status")?;
    if !body.is_object() {
        return Err(swap_error("Malformed status response", false));
    }
    let status = body
        .get("status")
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_uppercase();
    // Everything past the status sits under `swapDetails`: the delivery's
    // hash in `destinationChainTxHashes[].hash`, a failure's reason in
    // `refundReason`.
    let details = body.get("swapDetails").filter(|d| d.is_object());
    // A provider reports a refund under way with the same status word as a
    // delivery under way; only a positive `refundedAmount` tells them apart.
    let state = state_of(&status);
    let refunding = matches!(state, SwapState::AwaitingDeposit | SwapState::Processing)
        && details
            .and_then(|d| d.get("refundedAmount"))
            .is_some_and(positive_decimal);
    Ok(SwapStatus {
        state: if refunding {
            SwapState::Refunding
        } else {
            state
        },
        destination_tx_hash: details
            .and_then(|d| d.get("destinationChainTxHashes"))
            .and_then(Value::as_array)
            .and_then(|hashes| hashes.first())
            .filter(|first| first.is_object())
            .and_then(|first| optional(first, "hash")),
        detail: details.and_then(|d| optional(d, "refundReason")),
    })
}

fn decode_body(text: &str, what: &str) -> Result<Value, HostError> {
    serde_json::from_str(text)
        .map_err(|_| swap_error(format!("The {what} response is not JSON"), false))
}

/// A [`SwapProvider`] speaking the 1Click request shape.
///
/// Three endpoints, relative to `origin`: `GET /v0/tokens`, `POST /v0/quote`,
/// `GET /v0/status`. Held to the provider's schema by
/// `tests/oneclick_contract.rs`. `origin` is the wallet's — a
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
    /// How far the delivered amount may fall below the quote, in basis
    /// points (1/100 of a percent). The provider requires it on every quote;
    /// 100 is 1%.
    pub const SLIPPAGE_BASIS_POINTS: i64 = 100;

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

    /// The response body, or why the provider could not be reached.
    ///
    /// The transport is the wallet's, so its failures are its own. A network
    /// fault is retryable; nothing here can tell which, so the caller is told
    /// it may be. What the body *means* is the business of the functions
    /// above, which a caller making its own request uses too.
    fn read(&self, body: Result<String, String>) -> Result<String, HostError> {
        body.map_err(|e| swap_error(format!("The swap provider could not be reached: {e}"), true))
    }
}

fn required(object: &Value, key: &str) -> Result<String, HostError> {
    match object.get(key).and_then(Value::as_str) {
        Some(value) if !value.is_empty() => Ok(value.to_owned()),
        _ => Err(swap_error(format!("The provider omitted {key}"), false)),
    }
}

/// Whether `raw` is a decimal string of digits naming more than zero.
///
/// Read as text rather than as a number: the schema types the amount as a
/// string, and a figure past 64 bits must not wrap to zero or below.
fn positive_decimal(raw: &Value) -> bool {
    raw.as_str().is_some_and(|s| {
        !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && s.bytes().any(|b| b != b'0')
    })
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
        decimals: decimals(token)?,
    })
}

/// The token's `decimals`, which the schema requires.
///
/// Refused rather than defaulted: a guessed figure shows what arrives off by
/// a power of ten, beside a deposit that cannot be taken back.
fn decimals(token: &Value) -> Result<i32, HostError> {
    token
        .get("decimals")
        .and_then(Value::as_u64)
        .and_then(|d| i32::try_from(d).ok())
        .ok_or_else(|| swap_error("The provider omitted decimals".to_owned(), false))
}

/// The provider's own vocabulary, mapped onto §9.2's answers.
///
/// **An unrecognised status is [`SwapState::Processing`], never
/// [`SwapState::Delivered`].** Reading an unknown word as success would tell a
/// payer their debt is settled on the strength of a string nobody here has
/// defined.
///
/// `INCOMPLETE_DEPOSIT` is awaiting a deposit: the provider has received less
/// than the quote asked for, and nothing has been delivered on it.
fn state_of(status: &str) -> SwapState {
    match status {
        "PENDING_DEPOSIT" | "KNOWN_DEPOSIT_TX" | "INCOMPLETE_DEPOSIT" => SwapState::AwaitingDeposit,
        "PROCESSING" => SwapState::Processing,
        "SUCCESS" => SwapState::Delivered,
        "FAILED" | "REFUNDED" => SwapState::Failed,
        _ => SwapState::Processing,
    }
}

impl SwapProvider for OneClickSwaps<'_> {
    fn tradable_assets(&self) -> Result<Vec<TradableAsset>, HostError> {
        if let Some(cached) = self.tokens.borrow().as_ref() {
            return Ok(cached.clone());
        }
        let body = self.read(self.transport.get(&self.url("/v0/tokens", &[])))?;
        let tokens = assets_from_tokens(&body)?;
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
        let deadline = (self.deadline)();
        let request = quote_request_body(
            &self.zec_asset_id,
            asset,
            amount_in_zatoshi,
            recipient,
            refund_to,
            &deadline,
            self.referral.as_deref(),
        )?;
        let body = self.read(self.transport.post(&self.url("/v0/quote", &[]), &request))?;
        quote_from_response(&body, &request, asset, amount_in_zatoshi, &deadline)
    }

    fn status_of(&self, quote: &SwapQuote) -> Result<SwapStatus, HostError> {
        let mut query: Vec<(&str, &str)> = vec![("depositAddress", &quote.deposit_address)];
        if let Some(memo) = quote.deposit_memo.as_deref().filter(|m| !m.is_empty()) {
            query.push(("depositMemo", memo));
        }
        let body = self.read(self.transport.get(&self.url("/v0/status", &query)))?;
        status_from_response(&body)
    }
}

/// The most decimals a token states: a provider's figure, held in a `uint8`
/// by the token standards it reports. A larger one would size the rendering
/// by whatever the provider answered.
pub const MAX_TOKEN_DECIMALS: i32 = 255;

/// `base_units` of a token with `decimals` as whole tokens: `39990000` at 6
/// decimals is `39.99`. Trailing fractional zeros are dropped, so one value
/// has one rendering. `None` when `base_units` is not decimal digits or
/// `decimals` is outside 0..=[`MAX_TOKEN_DECIMALS`].
pub fn format_base_units(base_units: &str, decimals: i32) -> Option<String> {
    if base_units.is_empty()
        || !base_units.bytes().all(|b| b.is_ascii_digit())
        || !(0..=MAX_TOKEN_DECIMALS).contains(&decimals)
    {
        return None;
    }
    let decimals = decimals as usize;
    let digits = base_units.trim_start_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };
    if decimals == 0 {
        return Some(digits.to_owned());
    }
    let padded = format!("{digits:0>width$}", width = decimals + 1);
    let (whole, fraction) = padded.split_at(padded.len() - decimals);
    let fraction = fraction.trim_end_matches('0');
    Some(if fraction.is_empty() {
        whole.to_owned()
    } else {
        format!("{whole}.{fraction}")
    })
}

/// The `note` a swap's payment record carries (§9.2): the asset and the chain
/// it is delivered on — which the reference alone does not name once the
/// payee changes their payout — and, when the quote stated a floor, the least
/// the recipient is guaranteed, since the record claims the whole debt.
pub fn swap_record_note(asset_symbol: &str, asset_chain: &str, guaranteed: Option<&str>) -> String {
    match guaranteed {
        Some(floor) => format!("at least {floor} {asset_symbol} on {asset_chain}"),
        None => format!("{asset_symbol} on {asset_chain}"),
    }
}

/// What sending a swap's deposit takes: the request handed to the wallet, and
/// the note stored before the wallet is called (§14.3), which carries the
/// swap so its record can be written after a restart from the note alone.
#[derive(Debug, Clone, PartialEq)]
pub struct SwapDeposit {
    pub uri: String,
    pub note: crate::pending_sends::PendingSend,
}

/// The deposit for `quote`, settling `amount_minor_units` of the debt to `to`
/// on `bill_id` at the bill's `rate`; `at` is the wallet's §9.3 instant.
///
/// The request pays the quote's deposit address the quote's zatoshi and
/// carries no memo, so a quote whose deposit needs one is refused: a deposit
/// that arrives without the memo its provider requires is lost. Run
/// [`swap_send_refusal`] first; this builds what that check let through.
pub fn swap_deposit(
    bill_id: &str,
    quote: &SwapQuote,
    to: &str,
    amount_minor_units: i64,
    rate: &splitz_core::ExchangeRate,
    at: &str,
) -> Result<SwapDeposit, HostError> {
    if quote.deposit_memo.as_deref().is_some_and(|m| !m.is_empty()) {
        return Err(HostError::Swap {
            message: "this swap's deposit needs a memo a payment request cannot carry".to_owned(),
            transient: false,
        });
    }
    if amount_minor_units <= 0 {
        return Err(HostError::Malformed(format!(
            "a swap settles more than nothing, got {amount_minor_units}"
        )));
    }
    let uri = splitz_core::render_uri(
        &[splitz_core::Zip321Payment {
            address: quote.deposit_address.clone(),
            zatoshi: quote.amount_in_zatoshi,
            fiat: None,
            memo: None,
            label: Some(format!("swap to {}", quote.asset.symbol)),
            message: None,
        }],
        false,
    )
    .map_err(HostError::Protocol)?;
    let watch = crate::swap_watch::SwapWatch {
        bill_id: bill_id.to_owned(),
        reference: quote.payment_reference().to_owned(),
        to: to.to_owned(),
        deposit_address: quote.deposit_address.clone(),
        deposit_memo: quote.deposit_memo.clone(),
        asset_symbol: quote.asset.symbol.clone(),
        asset_chain: quote.asset.chain.clone(),
    };
    let note = crate::pending_sends::PendingSend {
        bill_id: bill_id.to_owned(),
        uri: uri.clone(),
        carried: std::collections::BTreeMap::from([(to.to_owned(), amount_minor_units)]),
        at: at.to_owned(),
        sent: std::collections::BTreeMap::new(),
        rate: Some(rate.clone()),
        swap: Some(watch),
        zatoshi: Some(quote.amount_in_zatoshi),
        txid: None,
    };
    Ok(SwapDeposit { uri, note })
}

/// One transaction paying `obligation`'s request and the deposit for
/// `quote` (§14.10): every ZEC payee the request carries, and the swap that
/// settles `amount_minor_units` of the debt to `to`, from one review and one
/// send.
///
/// The note carries all of it, so a restart records each half from the note
/// alone (§14.3).
///
/// `bill` is the bill as the store holds it now, and `obligation` this
/// payer's obligation read from it. Refuses a request that carries nobody in
/// ZEC and a swap to a payee the request already pays, which would be paid
/// twice; then, with [`HostError::SwapRefused`], whatever
/// [`swap_send_refusal`] refuses the swap leg at `at`, as it would a deposit
/// sent alone.
pub fn combined_send(
    bill_id: &str,
    bill: &Bill,
    obligation: &splitz_core::host::PayerObligation,
    quote: &SwapQuote,
    to: &str,
    amount_minor_units: i64,
    at: &str,
) -> Result<SwapDeposit, HostError> {
    let zec = obligation.carried_to();
    if zec.is_empty() || obligation.request.payments.is_empty() {
        return Err(HostError::Swap {
            message: "the request carries nobody to pay in ZEC".to_owned(),
            transient: false,
        });
    }
    if zec.contains_key(to) {
        return Err(HostError::Swap {
            message: "the request already pays this payee".to_owned(),
            transient: false,
        });
    }
    if let Some(refusal) = swap_send_refusal(
        quote,
        at,
        bill,
        Some(obligation),
        to,
        amount_minor_units,
        None,
    )
    .map_err(HostError::Protocol)?
    {
        return Err(HostError::SwapRefused(refusal));
    }
    let alone = swap_deposit(bill_id, quote, to, amount_minor_units, &obligation.rate, at)?;
    let mut payments = obligation.request.payments.clone();
    payments.push(splitz_core::Zip321Payment {
        address: quote.deposit_address.clone(),
        zatoshi: quote.amount_in_zatoshi,
        fiat: None,
        memo: None,
        label: Some(format!("swap to {}", quote.asset.symbol)),
        message: None,
    });
    let uri = splitz_core::render_uri(&payments, false).map_err(HostError::Protocol)?;
    let mut carried = zec;
    carried.insert(to.to_owned(), amount_minor_units);
    let note = crate::pending_sends::PendingSend {
        bill_id: bill_id.to_owned(),
        uri: uri.clone(),
        carried,
        at: at.to_owned(),
        sent: obligation.carried_zatoshi(),
        rate: Some(obligation.rate.clone()),
        swap: alone.note.swap,
        zatoshi: Some(quote.amount_in_zatoshi),
        txid: None,
    };
    Ok(SwapDeposit { uri, note })
}

/// The id `assets` — a provider's own token list — names native ZEC by, the
/// one a Zcash wallet's deposit is: symbol `ZEC` on chain `zec`, both read
/// ignoring case. `None` when the list carries none.
///
/// Read from the list rather than written down: a provider lists ZEC more
/// than once (wrapped on other chains too), and quoting the wrong one asks for
/// a deposit on a chain the wallet cannot send on.
pub fn zec_asset_in(assets: &[TradableAsset]) -> Option<String> {
    assets
        .iter()
        .find(|a| a.answers("ZEC", "zec"))
        .map(|a| a.asset_id.clone())
}
