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
pub mod currencies;
pub mod error;
pub mod fold;
pub mod keys;
pub mod pending_sends;
pub mod pricing;
pub mod relay;
pub mod sealing;
pub mod seam_contracts;
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
pub use currencies::{currency_exponent, ISO_4217_EXPONENTS};
pub use error::{HostError, Result};
pub use fold::{fold_unverified, fold_verified, FoldFailure};
pub use keys::{
    identity_seed_from, is_well_formed_key, Randomness, SplitsKeys, SystemRandomness,
    IDENTITY_DOMAIN, KEY_LENGTH_BYTES,
};
pub use pending_sends::{PendingSend, PendingSends, SendEnded, Unrecordable};
pub use pricing::{
    coingecko_price_url, price_from_coingecko, CoinGeckoZecPrices, FixedZecPrices, NoZecPrices,
    MAX_MINOR_UNITS_PER_ZEC,
};
pub use relay::{channel_for_bill, HttpSplitsRelay, InMemorySplitsRelay, UnconfiguredSplitsRelay};
pub use sealing::{Sealing, BLOB_VERSION};
pub use seam_contracts::{
    check_bill_storage, check_secret_store, check_splits_relay, check_zec_prices, SeamFinding,
};
pub use signing::{base64url_decode, base64url_encode, Signer, VerifiedLog, SEED_BYTES};
pub use split_draft::{DraftItem, SplitDraft, SplitKind};
pub use store::{BillStore, MergedBill};
pub use swap_watch::{SwapWatch, SwapWatchList};
pub use swaps::{
    assets_from_tokens, quote_from_response, quote_request_body, status_from_response,
    OneClickSwaps, SwapQuote, SwapState, SwapStatus, TradableAsset, UnconfiguredSwaps,
};
pub use sync::{SplitsSync, SyncResult};
pub use transport::{component_encode, query_encode, HttpTransport};
pub use wallet::{
    BillStorage, InMemoryBillStorage, InMemorySecretStore, SecretStore, SplitsRelay, SplitsWallet,
    SwapProvider, WalletAccount, WalletSendOutcome, WalletSendPhase, WalletSender, ZecPrices,
};
pub use wallet_bill_host::WalletBillHost;
