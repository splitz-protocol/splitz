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

pub mod activity;
pub mod confirm_review;
pub mod currencies;
pub mod error;
pub mod fold;
pub mod keys;
pub mod naming;
pub mod payer_review;
pub mod payouts;
pub mod pending_sends;
pub mod pricing;
pub mod refunds;
pub mod relay;
pub mod removal_plan;
pub mod sealing;
pub mod seam_contracts;
pub mod send_request;
pub mod signing;
pub mod split_draft;
pub mod store;
pub mod swap_watch;
pub mod swaps;
pub mod sync;
pub mod transport;
pub mod wallet;
pub mod wallet_bill_host;

pub use activity::{activity_of, awaiting_confirmation_by, BillEvent, BillEventKind};
pub use confirm_review::{concerns_before_confirming, PaymentConcern};
pub use currencies::{
    currency_exponent, parse_amount_in, parse_minor_units, parse_signed_amount_in,
    ISO_4217_EXPONENTS, MAX_PARSE_EXPONENT,
};
pub use error::{HostError, Result, SyncFailure};
pub use fold::{fold_unverified, fold_verified, FoldFailure};
pub use keys::{
    identity_secret_from_mnemonic, identity_seed_from, is_well_formed_key, Randomness, SplitsKeys,
    SystemRandomness, IDENTITY_DOMAIN, KEY_LENGTH_BYTES, MAX_ACCOUNT_INDEX,
};
pub use naming::{display_name_of, name_skeleton, shared_names, short_id};
pub use payer_review::{
    check_payee_review, check_payer_review, rate_figure, short_form, ReviewFinding, ReviewRule,
};
pub use payouts::{payout_fallback, ranked_payouts, PayoutFallback};
pub use pending_sends::{
    named_send_refusal, own_payment_withdrawal_refusal, unsent_claim_refusal, NamedSendRefusal,
    OwnPaymentWithdrawal, OwnTransaction, PendingSend, PendingSends, SendEnded, TransactionState,
    Unrecordable, UnsentClaimRefusal,
};
pub use pricing::{
    agreed_price, binance_price_url, coinbase_price_url, coingecko_price_url, creator_rate_missing,
    price_from_binance, price_from_coinbase, price_from_coingecko, rate_far_from_live,
    rate_percent_off, AgreeingZecPrices, BinanceZecPrices, CoinGeckoZecPrices, CoinbaseZecPrices,
    FirstZecPrices, FixedZecPrices, NoZecPrices, BINANCE_ZEC_SYMBOL, MAX_MINOR_UNITS_PER_ZEC,
    RATE_WARNING_PERCENT,
};
pub use refunds::{refunds_behind, RefundsBehind};
pub use relay::{channel_for_bill, HttpSplitsRelay, InMemorySplitsRelay, UnconfiguredSplitsRelay};
pub use removal_plan::{
    plan_removal, removal_entries, split_without, RemovalBlock, RemovalBlocker, RemovalEdit,
    RemovalPlan,
};
pub use sealing::{Sealing, BLOB_VERSION};
pub use seam_contracts::{
    check_bill_storage, check_secret_store, check_splits_relay, check_zec_prices, SeamFinding,
};
pub use send_request::{proposal_problem, send_payment_request};
pub use signing::{base64url_decode, base64url_encode, Signer, VerifiedLog, SEED_BYTES};
pub use split_draft::{DraftItem, SplitDraft, SplitKind};
pub use store::{BillStore, MergedBill};
pub use swap_watch::{SwapWatch, SwapWatchList};
pub use swaps::{
    assets_from_tokens, declared_payout_index, failed_swap_withdrawals, format_base_units,
    quote_from_response, quote_request_body, status_from_response, swap_answer, swap_deposit,
    swap_record_note, swap_send_refusal, zec_asset_in, OneClickSwaps, SwapDeposit, SwapQuote,
    SwapSendRefusal, SwapState, SwapStatus, TradableAsset, UnconfiguredSwaps, MAX_TOKEN_DECIMALS,
};
pub use sync::{SplitsSync, SyncResult};
pub use transport::{component_encode, query_encode, HttpTransport};
pub use wallet::{
    AccountSecretStore, BillStorage, InMemoryBillStorage, InMemorySecretStore, SecretStore,
    SplitsRelay, SplitsWallet, SwapProvider, WalletAccount, WalletSendOutcome, WalletSendPhase,
    WalletSender, ZecPrices,
};
pub use wallet_bill_host::WalletBillHost;
