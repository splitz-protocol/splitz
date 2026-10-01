//! Exact integer money (SPEC.md §2).
//!
//! An amount is a signed integer count of the smallest indivisible unit of one
//! currency. No amount is a floating point value at any point in its life.

use crate::error::{code, Result, SplitError};

/// The largest amount this protocol admits (SPEC.md §2.2).
///
/// Stated as a constant rather than taken from the host's widest type, so that
/// every implementation accepts and refuses the same inputs.
pub const MAX_AMOUNT: i64 = i64::MAX;

/// The largest magnitude one expense may carry (SPEC.md §2.2).
///
/// The largest amount §7.1 can price: it forms `minorUnits × 100000000`,
/// which fits an `i64` up to exactly this figure. A balance reaches
/// [`MAX_AMOUNT`] only after 100000001 expenses at the cap.
pub const MAX_ENTRY_AMOUNT: i64 = 92_233_720_368;

/// The most negative amount. It has no positive counterpart, so taking its
/// magnitude is the identity (SPEC.md §3, step 3).
pub const MIN_AMOUNT: i64 = i64::MIN;

/// Whether `code` is an ISO 4217 alpha-3 code in upper case (SPEC.md §2.1).
///
/// Lower case is refused rather than folded: `usd` and `USD` are one currency
/// to a person and two to the byte order of §2.3.
pub fn is_currency(value: &str) -> bool {
    value.len() == 3 && value.bytes().all(|b| b.is_ascii_uppercase())
}

/// Refuses `value` with `bill_bad_currency` unless it is a currency.
pub fn check_currency(value: &str) -> Result<()> {
    if is_currency(value) {
        Ok(())
    } else {
        Err(SplitError::new(
            code::BILL_BAD_CURRENCY,
            format!("A currency is three upper-case letters, got \"{value}\""),
        ))
    }
}

/// `a + b`, refused rather than wrapped (SPEC.md §2.2).
pub fn checked_add(a: i64, b: i64, refusal: &'static str) -> Result<i64> {
    a.checked_add(b).ok_or_else(|| {
        SplitError::new(
            refusal,
            format!("Adding {a} and {b} overflows a 64-bit integer"),
        )
    })
}

/// `a - b`, refused rather than wrapped (SPEC.md §2.2).
///
/// Not `checked_add(a, -b)`: negating the smallest 64-bit value overflows
/// before the addition is checked.
pub fn checked_sub(a: i64, b: i64, refusal: &'static str) -> Result<i64> {
    a.checked_sub(b).ok_or_else(|| {
        SplitError::new(
            refusal,
            format!("Subtracting {b} from {a} overflows a 64-bit integer"),
        )
    })
}

/// `balance`, refused with `amount_overflow` when it is `i64::MIN` (§2.2).
///
/// A balance has a magnitude, so its range is symmetric: the most negative
/// 64-bit value has no positive counterpart, and §5.1's residual and §6's
/// matching could not form one for it.
pub fn checked_balance(balance: i64) -> Result<i64> {
    if balance == MIN_AMOUNT {
        return Err(SplitError::new(
            code::AMOUNT_OVERFLOW,
            format!("A balance of {balance} has no magnitude"),
        ));
    }
    Ok(balance)
}

/// `a * b`, refused rather than wrapped (SPEC.md §2.2).
pub fn checked_mul(a: i64, b: i64, refusal: &'static str) -> Result<i64> {
    a.checked_mul(b).ok_or_else(|| {
        SplitError::new(
            refusal,
            format!("Multiplying {a} by {b} overflows a 64-bit integer"),
        )
    })
}

/// The sum of `values`, refused rather than wrapped.
pub fn checked_sum(values: impl IntoIterator<Item = i64>, refusal: &'static str) -> Result<i64> {
    let mut total: i64 = 0;
    for value in values {
        total = checked_add(total, value, refusal)?;
    }
    Ok(total)
}

/// An integer section 2.2 holds. Section 9.3: a number's code follows from its
/// value — one past the 64-bit range, or one no double holds, is
/// `amount_overflow` however it was written, and any other non-integer is
/// `canonical_json_float`.
pub fn document_integer(value: &serde_json::Value) -> Result<i64> {
    if let Some(n) = value.as_i64() {
        // Section 2.2's range is symmetric: the most negative 64-bit value has
        // no positive counterpart, so its magnitude is past the range.
        if n == i64::MIN {
            return Err(SplitError::new(
                code::AMOUNT_OVERFLOW,
                "A number past 64 bits",
            ));
        }
        return Ok(n);
    }
    if !value.is_number() {
        return Err(SplitError::new(
            code::BILL_TYPE_ERROR,
            "Expected an integer",
        ));
    }
    let past = value.is_u64()
        || value
            .as_f64()
            .is_none_or(|f| !f.is_finite() || f.abs() >= 9_223_372_036_854_775_808.0);
    Err(if past {
        SplitError::new(code::AMOUNT_OVERFLOW, "A number past 64 bits")
    } else {
        SplitError::new(
            code::CANONICAL_JSON_FLOAT,
            "A number that is not an integer",
        )
    })
}
