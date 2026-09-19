//! A shared-bill protocol for Zcash wallets.
//!
//! Expenses go in; out come the fewest payments that settle them, and the
//! ZIP 321 payment request URI that carries one payer's whole obligation in a
//! single transaction. The protocol is specified in `SPEC.md`; this crate
//! implements it.

pub mod allocation;
pub mod authority;
pub mod balances;
pub mod canonical_json;
pub mod error;
pub mod instant;
pub mod invite;
pub mod log;
pub mod model;
pub mod money;
pub mod obligation;
pub mod ordering;
pub mod rate;
pub mod serialization;
pub mod settle;
pub mod sha256;
pub mod split;
pub mod zip321;

pub use allocation::{allocate, allocate_evenly};
pub use authority::{resolve_identities, signing_message, Identities};
pub use balances::{creditors, debtors, direct_debts, net_balances};
pub use canonical_json::canonical_json;
pub use error::{code, Result, SplitError};
pub use invite::{
    decode_payload, encode_payload, parse_invite, parse_sealed_frame, render_invite, Invite,
};
pub use log::{check_entry, derive_bill_id, fold_log, merge_logs, MergeResult};
pub use money::{check_currency, is_currency, MAX_AMOUNT, MIN_AMOUNT};
pub use obligation::{render_obligation, Obligation, Unpayable};
pub use rate::{fiat_to_zatoshi, zatoshi_to_fiat, ExchangeRate, RateRounding};
pub use serialization::{decode_bill, BILL_VERSION};
pub use settle::{attribute_coverage, settle_balances, settle_bill, DEFAULT_EXACT_LIMIT};
pub use sha256::{sha256, sha256_hex};
pub use split::{split_expense, Shares};
pub use zip321::{render_amount, render_uri, FiatPrice, Zip321Payment};
