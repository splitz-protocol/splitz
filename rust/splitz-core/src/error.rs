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
    pub const AMOUNT_TOO_LARGE: &str = "amount_too_large";
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
    pub const DUPLICATE_PAYMENT: &str = "duplicate_payment";
    pub const DUPLICATE_EXPENSE: &str = "duplicate_expense";
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
    pub const PARTICIPANT_ID_NOT_DERIVED: &str = "participant_id_not_derived";

    // §11 invites, payloads and sealing
    pub const INVITE_NOT_AN_INVITE: &str = "invite_not_an_invite";
    pub const INVITE_MISSING_VERSION: &str = "invite_missing_version";
    pub const INVITE_FUTURE_VERSION: &str = "invite_future_version";
    pub const INVITE_MISSING_BILL_ID: &str = "invite_missing_bill_id";
    pub const INVITE_BAD_BILL_ID: &str = "invite_bad_bill_id";
    pub const INVITE_MISSING_KEY: &str = "invite_missing_key";
    pub const INVITE_BAD_EXPIRY: &str = "invite_bad_expiry";
    pub const INVITE_BAD_LINK: &str = "invite_bad_link";
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
    pub const ZIP321_NOT_CANONICAL: &str = "zip321_not_canonical";
    pub const ZIP321_MEMO_UNDELIVERABLE: &str = "zip321_memo_undeliverable";

    // §8.6 addresses
    pub const ADDRESS_INVALID: &str = "address_invalid";

    // §14.8 paying by a lower preference
    pub const PAYOUT_NOT_DECLARED: &str = "payout_not_declared";
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

/// The sentence a host MAY show a person for each §12 code.
///
/// A default, not part of the protocol: the code is what a caller branches on,
/// and a host with its own words keeps them. `vectors/messages.json` holds the
/// same table, and both implementations reproduce it.
const PLAIN_MESSAGES: &[(&str, &str)] = &[
    // Generated by tools/corpus/messages.py.
    ("address_invalid", "This isn't a Zcash address."),
    ("allocation_overflow", "This amount is too large to divide."),
    (
        "ambiguous_create",
        "This bill was started twice. It can't be opened.",
    ),
    (
        "amend_kind_mismatch",
        "A change must be the same kind as what it changes.",
    ),
    ("amount_overflow", "This amount is too large."),
    ("amount_too_large", "This amount is too large."),
    (
        "balances_nonzero_residual",
        "The balances don't add up. The bill may be damaged.",
    ),
    ("bill_ambiguous_entry", "Part of this bill is damaged."),
    ("bill_bad_currency", "This bill's currency isn't valid."),
    ("bill_bad_participant_id", "Part of this bill is damaged."),
    (
        "bill_future_version",
        "This bill needs a newer app. Update to open it.",
    ),
    ("bill_missing_currency", "This bill has no currency."),
    (
        "bill_missing_entry_payload",
        "Part of this bill is damaged.",
    ),
    ("bill_missing_version", "This bill can't be read."),
    ("bill_not_scalar_values", "This bill is damaged."),
    ("bill_type_error", "This bill is damaged."),
    (
        "bill_unknown_confirmation_method",
        "This bill needs a newer app. Update to open it.",
    ),
    (
        "bill_unknown_entry_kind",
        "This bill needs a newer app. Update to open it.",
    ),
    (
        "bill_unknown_payout_method",
        "This bill needs a newer app. Update to open it.",
    ),
    (
        "bill_unknown_settlement_method",
        "This bill needs a newer app. Update to open it.",
    ),
    (
        "bill_unknown_split_mode",
        "This bill needs a newer app. Update to open it.",
    ),
    (
        "bill_unknown_split_type",
        "This bill needs a newer app. Update to open it.",
    ),
    (
        "canonical_json_float",
        "This bill holds a number this app can't read.",
    ),
    (
        "confirmation_missing_reference",
        "Add the transaction to confirm this.",
    ),
    ("create_id_not_derived", "This bill's start is damaged."),
    ("create_unbound", "This bill's start is damaged."),
    (
        "currency_mismatch",
        "This uses a different currency from the bill.",
    ),
    ("duplicate_expense", "This expense is already on the bill."),
    (
        "duplicate_participant",
        "That person is already on this bill.",
    ),
    ("duplicate_payment", "This payment is already recorded."),
    ("empty_split", "Pick who shares this cost."),
    ("empty_weights", "Nobody is sharing this cost."),
    ("entry_id_not_derived", "Part of this bill is damaged."),
    (
        "exact_limit_too_large",
        "This bill has too many people to settle exactly.",
    ),
    (
        "exact_total_mismatch",
        "The amounts don't add up to the total.",
    ),
    (
        "invite_bad_bill_id",
        "This invite is damaged. Ask for a new one.",
    ),
    (
        "invite_bad_expiry",
        "This invite is damaged. Ask for a new one.",
    ),
    (
        "invite_bad_link",
        "This invite link can't be made from that address.",
    ),
    (
        "invite_future_version",
        "This invite needs a newer app. Update to join.",
    ),
    (
        "invite_missing_bill_id",
        "This invite is incomplete. Ask for a new one.",
    ),
    (
        "invite_missing_key",
        "This invite is incomplete. Ask for a new one.",
    ),
    ("invite_missing_version", "This invite can't be read."),
    ("invite_not_an_invite", "This isn't a bill invite."),
    ("itemized_no_items", "Add at least one item."),
    (
        "itemized_total_mismatch",
        "The items don't add up to the total.",
    ),
    ("itemized_unassigned_item", "Pick who shares each item."),
    ("log_empty", "This bill is empty."),
    ("log_no_create", "This bill's start is missing. Sync again."),
    ("negative_amount", "An amount can't be negative."),
    ("negative_share", "A share can't be negative."),
    ("negative_weight", "A share can't be negative."),
    (
        "participant_id_not_derived",
        "This person's id doesn't match their key.",
    ),
    (
        "participant_still_named",
        "This person still has costs or payments on the bill.",
    ),
    ("payload_damaged", "This code is damaged. Scan it again."),
    (
        "payload_future_version",
        "This code needs a newer app. Update to scan it.",
    ),
    (
        "payload_missing_body",
        "This code is incomplete. Scan it again.",
    ),
    ("payload_not_a_payload", "This isn't a bill code."),
    (
        "payload_too_large",
        "This bill is too big for one code. Share an invite.",
    ),
    (
        "payout_not_declared",
        "They haven't added that way to be paid.",
    ),
    (
        "percentage_not_full_scale",
        "The percentages don't add up to 100.",
    ),
    (
        "rate_amount_too_large",
        "This amount is too large to price.",
    ),
    (
        "rate_currency_mismatch",
        "This price is in a different currency from the bill.",
    ),
    ("rate_not_positive", "A price must be more than zero."),
    (
        "sealed_future_version",
        "An update to this bill needs a newer app.",
    ),
    ("sealed_malformed", "An update to this bill is damaged."),
    ("self_payment", "You can't pay yourself."),
    (
        "unauthorized_confirmation",
        "Only the person paid can confirm this.",
    ),
    (
        "unauthorized_entry",
        "Only the person who wrote this can change it.",
    ),
    (
        "unauthorized_payment",
        "Only the payer can record this payment.",
    ),
    ("unknown_entry", "That item isn't on this bill."),
    ("unknown_participant", "That person isn't on this bill."),
    ("unknown_payment", "That payment isn't on this bill."),
    ("weight_sum_overflow", "The shares are too large to add up."),
    ("zero_weight_sum", "The shares add up to nothing."),
    (
        "zip321_amount_not_positive",
        "An amount to pay must be more than zero.",
    ),
    (
        "zip321_amount_too_large",
        "This amount is more ZEC than exists.",
    ),
    ("zip321_bad_address", "An address on this bill isn't valid."),
    (
        "zip321_bad_currency_code",
        "This bill's currency isn't valid.",
    ),
    (
        "zip321_fiat_not_positive",
        "An amount must be more than zero.",
    ),
    ("zip321_fiat_too_many_digits", "This amount is too large."),
    ("zip321_memo_too_large", "This note is too long."),
    (
        "zip321_memo_undeliverable",
        "A note can't be sent to this address.",
    ),
    (
        "zip321_no_address",
        "Someone on this bill has no address to be paid at.",
    ),
    ("zip321_no_payments", "There's nothing to pay."),
    (
        "zip321_not_canonical",
        "This payment request wasn't made by this bill.",
    ),
    (
        "zip321_too_many_payments",
        "Too many people to pay at once.",
    ),
    // End of generated table.
];

/// A plain-language sentence for the §12 `code`, or `None` for a code this
/// crate does not define.
///
/// `None` rather than a generic sentence: a code from a newer version is
/// something the caller can see and say, and a guess would hide it.
pub fn describe_code(code: &str) -> Option<&'static str> {
    PLAIN_MESSAGES
        .iter()
        .find(|(c, _)| *c == code)
        .map(|(_, text)| *text)
}
