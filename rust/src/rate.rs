//! Fiat to zatoshi and back (SPEC.md §7).
//!
//! The rate is part of the bill's shared state, snapshotted rather than looked
//! up per device: six people applying six live rates to one dinner compute six
//! different amounts and the bill never closes.

use crate::error::{code, Result, SplitError};
use crate::money::{check_currency, checked_mul, MAX_AMOUNT};

/// One ZEC, in zatoshi.
pub const ZATOSHI_PER_ZEC: i64 = 100_000_000;

/// The largest amount §7.1 can convert, being `MAX_AMOUNT / ZATOSHI_PER_ZEC`.
pub const MAX_CONVERTIBLE_MINOR_UNITS: i64 = MAX_AMOUNT / ZATOSHI_PER_ZEC;

/// How the last zatoshi is decided when the division leaves a remainder.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RateRounding {
    /// Settlement amounts default to this. A debt rounded down leaves dust
    /// behind and the bill never quite closes.
    Up,
    Down,
    Nearest,
}

/// The price of one ZEC, in a currency's minor units.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeRate {
    pub currency: String,
    pub minor_units_per_zec: i64,
    pub at: String,
    pub source: Option<String>,
}

/// Converts `minor_units` to zatoshi at `rate`.
pub fn fiat_to_zatoshi(
    minor_units: i64,
    rate: &ExchangeRate,
    amount_currency: Option<&str>,
    rounding: RateRounding,
) -> Result<i64> {
    check_currency(&rate.currency)?;
    if let Some(currency) = amount_currency {
        check_currency(currency)?;
        if currency != rate.currency {
            return Err(SplitError::new(
                code::RATE_CURRENCY_MISMATCH,
                format!("A rate in {} does not price {currency}", rate.currency),
            ));
        }
    }
    if rate.minor_units_per_zec <= 0 {
        return Err(SplitError::new(
            code::RATE_NOT_POSITIVE,
            format!(
                "A ZEC costs more than nothing, got {}",
                rate.minor_units_per_zec
            ),
        ));
    }
    if minor_units < 0 {
        return Err(SplitError::new(
            code::NEGATIVE_AMOUNT,
            format!("An amount of {minor_units} is negative"),
        ));
    }
    if minor_units > MAX_CONVERTIBLE_MINOR_UNITS {
        return Err(SplitError::new(
            code::RATE_AMOUNT_TOO_LARGE,
            format!("Converting {minor_units} overflows a 64-bit integer"),
        ));
    }

    // The multiplication precedes the division so a small amount at a high ZEC
    // price does not collapse to zero.
    let numerator = minor_units * ZATOSHI_PER_ZEC;
    let quotient = numerator / rate.minor_units_per_zec;
    let remainder = numerator % rate.minor_units_per_zec;
    if remainder == 0 {
        return Ok(quotient);
    }
    Ok(match rounding {
        RateRounding::Up => quotient + 1,
        RateRounding::Down => quotient,
        RateRounding::Nearest => {
            // Written as a comparison rather than remainder * 2, which
            // wraps at 2^62 and then rounds the wrong way. Both sides are
            // bounded by the rate.
            if remainder >= rate.minor_units_per_zec - remainder {
                quotient + 1
            } else {
                quotient
            }
        }
    })
}

/// Converts `zatoshi` to minor units at `rate`, rounding halves up.
///
/// For display only — a label under a number, not a number anyone settles
/// against. Its result must never reach a settlement amount.
pub fn zatoshi_to_fiat(zatoshi: i64, rate: &ExchangeRate) -> Result<i64> {
    check_currency(&rate.currency)?;
    if rate.minor_units_per_zec <= 0 {
        return Err(SplitError::new(
            code::RATE_NOT_POSITIVE,
            format!(
                "A ZEC costs more than nothing, got {}",
                rate.minor_units_per_zec
            ),
        ));
    }
    if zatoshi < 0 {
        return Err(SplitError::new(
            code::NEGATIVE_AMOUNT,
            format!("A count of {zatoshi} zatoshi is negative"),
        ));
    }
    let product = checked_mul(zatoshi, rate.minor_units_per_zec, code::AMOUNT_OVERFLOW)?;
    let quotient = product / ZATOSHI_PER_ZEC;
    let remainder = product % ZATOSHI_PER_ZEC;
    Ok(if remainder * 2 >= ZATOSHI_PER_ZEC {
        quotient + 1
    } else {
        quotient
    })
}
