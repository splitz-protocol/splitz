//! Every refusal this crate makes, and the codes that name them.
//!
//! The code is part of the protocol; the message beside it is prose and is not
//! (SPEC.md §12). A caller branches on the code, never on the message.

use std::fmt;

/// The refusal codes of SPEC.md §12.
pub mod code {
    // §3 allocation
    pub const EMPTY_WEIGHTS: &str = "empty_weights";
    pub const NEGATIVE_WEIGHT: &str = "negative_weight";
    pub const ZERO_WEIGHT_SUM: &str = "zero_weight_sum";
    pub const ALLOCATION_OVERFLOW: &str = "allocation_overflow";
    pub const WEIGHT_SUM_OVERFLOW: &str = "weight_sum_overflow";

    // §4 split methods
    pub const NEGATIVE_SHARE: &str = "negative_share";
    pub const AMOUNT_OVERFLOW: &str = "amount_overflow";
    pub const EMPTY_SPLIT: &str = "empty_split";
    pub const EXACT_TOTAL_MISMATCH: &str = "exact_total_mismatch";
    pub const PERCENTAGE_NOT_FULL_SCALE: &str = "percentage_not_full_scale";
    pub const ITEMIZED_NO_ITEMS: &str = "itemized_no_items";
    pub const ITEMIZED_UNASSIGNED_ITEM: &str = "itemized_unassigned_item";
    pub const ITEMIZED_TOTAL_MISMATCH: &str = "itemized_total_mismatch";

    // §2, §5, §6
    pub const CURRENCY_MISMATCH: &str = "currency_mismatch";
    pub const UNKNOWN_PARTICIPANT: &str = "unknown_participant";
    pub const UNKNOWN_ENTRY: &str = "unknown_entry";
    pub const DUPLICATE_PARTICIPANT: &str = "duplicate_participant";
    pub const SELF_PAYMENT: &str = "self_payment";
    pub const BALANCES_NONZERO_RESIDUAL: &str = "balances_nonzero_residual";
    pub const EXACT_LIMIT_TOO_LARGE: &str = "exact_limit_too_large";

    // §7 rates
    pub const RATE_CURRENCY_MISMATCH: &str = "rate_currency_mismatch";
    pub const RATE_NOT_POSITIVE: &str = "rate_not_positive";
    pub const NEGATIVE_AMOUNT: &str = "negative_amount";
    pub const RATE_AMOUNT_TOO_LARGE: &str = "rate_amount_too_large";

    // §9 the bill wire format
    pub const BILL_MISSING_VERSION: &str = "bill_missing_version";
    pub const BILL_FUTURE_VERSION: &str = "bill_future_version";
    pub const BILL_TYPE_ERROR: &str = "bill_type_error";
    pub const BILL_NOT_SCALAR_VALUES: &str = "bill_not_scalar_values";
    pub const ENTRY_ID_NOT_DERIVED: &str = "entry_id_not_derived";
    pub const BILL_BAD_PARTICIPANT_ID: &str = "bill_bad_participant_id";
    pub const BILL_MISSING_CURRENCY: &str = "bill_missing_currency";
    pub const BILL_BAD_CURRENCY: &str = "bill_bad_currency";
    pub const BILL_UNKNOWN_SPLIT_TYPE: &str = "bill_unknown_split_type";
    pub const BILL_UNKNOWN_SPLIT_MODE: &str = "bill_unknown_split_mode";
    pub const BILL_UNKNOWN_ENTRY_KIND: &str = "bill_unknown_entry_kind";
    pub const BILL_UNKNOWN_SETTLEMENT_METHOD: &str = "bill_unknown_settlement_method";
    pub const BILL_UNKNOWN_PAYOUT_METHOD: &str = "bill_unknown_payout_method";
    pub const BILL_UNKNOWN_CONFIRMATION_METHOD: &str = "bill_unknown_confirmation_method";
    pub const BILL_AMBIGUOUS_ENTRY: &str = "bill_ambiguous_entry";
    pub const BILL_MISSING_ENTRY_PAYLOAD: &str = "bill_missing_entry_payload";
    pub const CANONICAL_JSON_FLOAT: &str = "canonical_json_float";

    // §10 the log
    pub const LOG_EMPTY: &str = "log_empty";
    pub const LOG_NO_CREATE: &str = "log_no_create";
    pub const AMBIGUOUS_CREATE: &str = "ambiguous_create";
    pub const CREATE_UNBOUND: &str = "create_unbound";
    pub const CREATE_ID_NOT_DERIVED: &str = "create_id_not_derived";
    pub const UNAUTHORIZED_ENTRY: &str = "unauthorized_entry";
    pub const UNAUTHORIZED_PAYMENT: &str = "unauthorized_payment";
    pub const UNAUTHORIZED_CONFIRMATION: &str = "unauthorized_confirmation";
    pub const CONFIRMATION_MISSING_REFERENCE: &str = "confirmation_missing_reference";
    pub const UNKNOWN_PAYMENT: &str = "unknown_payment";
    pub const AMEND_KIND_MISMATCH: &str = "amend_kind_mismatch";
    pub const PARTICIPANT_STILL_NAMED: &str = "participant_still_named";

    // §11 invites, payloads and sealing
    pub const INVITE_NOT_AN_INVITE: &str = "invite_not_an_invite";
    pub const INVITE_MISSING_VERSION: &str = "invite_missing_version";
    pub const INVITE_FUTURE_VERSION: &str = "invite_future_version";
    pub const INVITE_MISSING_BILL_ID: &str = "invite_missing_bill_id";
    pub const INVITE_BAD_BILL_ID: &str = "invite_bad_bill_id";
    pub const INVITE_MISSING_KEY: &str = "invite_missing_key";
    pub const INVITE_BAD_EXPIRY: &str = "invite_bad_expiry";
    pub const PAYLOAD_NOT_A_PAYLOAD: &str = "payload_not_a_payload";
    pub const PAYLOAD_DAMAGED: &str = "payload_damaged";
    pub const PAYLOAD_MISSING_BODY: &str = "payload_missing_body";
    pub const PAYLOAD_FUTURE_VERSION: &str = "payload_future_version";
    pub const PAYLOAD_TOO_LARGE: &str = "payload_too_large";
    pub const SEALED_MALFORMED: &str = "sealed_malformed";
    pub const SEALED_FUTURE_VERSION: &str = "sealed_future_version";

    // §8 payment requests
    pub const ZIP321_NO_PAYMENTS: &str = "zip321_no_payments";
    pub const ZIP321_TOO_MANY_PAYMENTS: &str = "zip321_too_many_payments";
    pub const ZIP321_AMOUNT_NOT_POSITIVE: &str = "zip321_amount_not_positive";
    pub const ZIP321_AMOUNT_TOO_LARGE: &str = "zip321_amount_too_large";
    pub const ZIP321_MEMO_TOO_LARGE: &str = "zip321_memo_too_large";
    pub const ZIP321_BAD_ADDRESS: &str = "zip321_bad_address";
    pub const ZIP321_BAD_CURRENCY_CODE: &str = "zip321_bad_currency_code";
    pub const ZIP321_FIAT_NOT_POSITIVE: &str = "zip321_fiat_not_positive";
    pub const ZIP321_FIAT_TOO_MANY_DIGITS: &str = "zip321_fiat_too_many_digits";
    pub const ZIP321_NO_ADDRESS: &str = "zip321_no_address";
}

/// A refusal, carrying the code that names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SplitError {
    /// The code from SPEC.md §12.
    pub code: &'static str,
    /// Prose for a developer. Not part of the protocol.
    pub message: String,
}

impl SplitError {
    pub fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl fmt::Display for SplitError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.code, self.message)
    }
}

impl std::error::Error for SplitError {}

pub type Result<T> = std::result::Result<T, SplitError>;
