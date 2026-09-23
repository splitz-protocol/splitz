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
pub mod host;
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
pub use authority::{resolve_identities, signing_message, Identities, ENTRY_SIGNING_DOMAIN};
pub use balances::{
    creditors, debtors, direct_debts, net_balances, residual_is_zero, DirectDebt, Position,
};
pub use canonical_json::canonical_json;
pub use error::{code, Result, SplitError};
pub use instant::canonical_instant;
pub use invite::{
    channel_for, decode_payload, delta_for, encode_payload, frame_sealed, parse_invite,
    parse_sealed_frame, render_invite, sealed_nonce, sealed_plaintext, strip_scan_padding,
    within_depth, Delta, Invite, ScannedPayload, SealedFrame, BILL_PREFIX, DELTA_PREFIX,
    INVITE_VERSION, MAX_DOCUMENT_DEPTH, MAX_INVITE_BILL_ID, NONCE_BYTES, PAYLOAD_CAP,
    PAYLOAD_VERSION, SCAN_PADDING, SEALED_VERSION, TAG_BYTES,
};
pub use log::{
    check_entry, confirmation_rule, derive_bill_id, derive_entry_id, fold_log, merge_logs,
    order_entries, payload_for, payment_digest, FoldResult, MergeResult, ReplacedAddress, SetAside,
    BILL_ID_DOMAIN, ENTRY_ID_DOMAIN, ENTRY_KINDS, PAYMENT_DIGEST_DOMAIN,
};
pub use model::{Bill, Expense, Participant, PaymentRecord, Payout};
pub use money::{
    check_currency, checked_add, checked_mul, checked_sub, checked_sum, is_currency, MAX_AMOUNT,
    MIN_AMOUNT,
};
pub use obligation::{
    render_obligation, withholdings, Awaiting, Contested, Obligation, Unpayable, Withholdings,
};
pub use ordering::{compare_utf8, sorted_utf8, unique_sorted_utf8};
pub use rate::{
    fiat_to_zatoshi, zatoshi_to_fiat, ExchangeRate, RateRounding, MAX_CONVERTIBLE_MINOR_UNITS,
    ZATOSHI_PER_ZEC,
};
pub use serialization::{
    bill_to_json, decode_bill, decode_expense, decode_participant, decode_payment, decode_rate,
    expense_to_json, participant_to_json, payment_to_json, payout_to_json, rate_to_json,
    BILL_VERSION, SPLIT_MODES,
};
pub use settle::{
    attribute_coverage, settle_balances, settle_bill, Settlement, SettlementPlan,
    DEFAULT_EXACT_LIMIT, MAX_EXACT_LIMIT,
};
pub use sha256::{sha256, sha256_hex};
pub use split::{check_id_lists, split_expense, split_participants, Shares};
pub use zip321::{
    bounded_label, is_zip321_address, qchar, render_amount, render_uri, FiatPrice, Zip321Payment,
    MAX_FIAT_DIGITS, MAX_LABEL_BYTES, MAX_MEMO_BYTES, MAX_PAYMENTS, MAX_ZATOSHI,
};
