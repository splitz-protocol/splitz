//! What one ZEC costs, and where that figure comes from.

use serde_json::Value;

use crate::currencies::currency_exponent;
use crate::error::{HostError, Result};
use crate::transport::{query_encode, HttpTransport};
use crate::wallet::ZecPrices;

/// A price this build was told, rather than one it looked up.
///
/// For a demonstration, and for a test that needs the arithmetic to be
/// predictable. It is not a feed and does not pretend to be: what it returns
/// was true whenever somebody typed it.
#[derive(Debug, Clone, Default)]
pub struct FixedZecPrices {
    prices: std::collections::BTreeMap<String, i64>,
}

impl FixedZecPrices {
    /// Currency code to minor units per ZEC. Codes are compared upper-case.
    pub fn new(prices: impl IntoIterator<Item = (String, i64)>) -> Self {
        Self {
            prices: prices
                .into_iter()
                .map(|(code, price)| (code.to_uppercase(), price))
                .collect(),
        }
    }
}

impl ZecPrices for FixedZecPrices {
    fn minor_units_per_zec(&self, currency: &str) -> Result<Option<i64>> {
        Ok(self.prices.get(&currency.to_uppercase()).copied())
    }
}

/// A source that prices nothing.
///
/// The honest default for a build with no feed wired in. Every bill is then
/// unpriced until somebody types a figure, and the screen says so instead of
/// showing a number nothing stands behind.
#[derive(Debug, Clone, Copy, Default)]
pub struct NoZecPrices;

impl ZecPrices for NoZecPrices {
    fn minor_units_per_zec(&self, _currency: &str) -> Result<Option<i64>> {
        Ok(None)
    }
}

/// The largest figure a price may be: 2^53 - 1, so it is exact wherever a
/// binding carries it as a double.
pub const MAX_MINOR_UNITS_PER_ZEC: i64 = 9_007_199_254_740_991;

/// Minor units of `currency` one ZEC costs, read from a CoinGecko
/// `/simple/price?ids=zcash&vs_currencies=…` answer.
///
/// The price is read as the shortest decimal that names the parsed number,
/// and scaled by the currency's ISO 4217 exponent in exact integers, rounding
/// halves up — never multiplied as a float, where `1.005 × 100` is
/// `100.49999…`.
///
/// `None` when the answer does not price `currency`, when the register gives
/// `currency` no exponent (§2.1), and when the price is not positive, rounds
/// to nothing, or exceeds [`MAX_MINOR_UNITS_PER_ZEC`]. Refused with
/// [`HostError::Price`] when `body` is not such an answer at all.
pub fn price_from_coingecko(body: &str, currency: &str) -> Result<Option<i64>> {
    let malformed = |why: &str| HostError::Price(why.to_owned());
    let decoded: Value =
        serde_json::from_str(body).map_err(|_| malformed("the price answer is not JSON"))?;
    let zcash = decoded
        .as_object()
        .ok_or_else(|| malformed("the price answer is not an object"))?
        .get("zcash")
        .and_then(Value::as_object)
        .ok_or_else(|| malformed("the price answer names no zcash price"))?;
    let Some(exponent) = currency_exponent(currency) else {
        return Ok(None);
    };
    let Some(raw) = zcash.get(&currency.to_lowercase()) else {
        return Ok(None);
    };
    let Value::Number(number) = raw else {
        if raw.is_null() {
            return Ok(None);
        }
        return Err(malformed(&format!("the {currency} price is not a number")));
    };
    // The shortest decimal naming the number: an integer as written, a float
    // as its shortest round-trip form.
    let decimal = match (number.as_u64(), number.as_i64(), number.as_f64()) {
        (Some(u), _, _) => u.to_string(),
        (None, Some(i), _) => i.to_string(),
        (None, None, Some(f)) if f.is_finite() => format!("{f}"),
        _ => return Ok(None),
    };
    Ok(scale_exactly(&decimal, exponent).filter(|v| (1..=MAX_MINOR_UNITS_PER_ZEC).contains(v)))
}

/// `decimal` × 10^`exponent`, rounded half up, or `None` for a negative value
/// or one too large to hold.
fn scale_exactly(decimal: &str, exponent: u32) -> Option<i64> {
    if decimal.starts_with('-') {
        return None;
    }
    let (whole, fraction) = decimal.split_once('.').unwrap_or((decimal, ""));
    if whole.is_empty() || !(whole.bytes().chain(fraction.bytes())).all(|b| b.is_ascii_digit()) {
        return None;
    }
    let digits: String = format!("{whole}{fraction}");
    let digits = digits.trim_start_matches('0');
    // value = digits × 10^shift, and the answer is value × 10^exponent.
    let shift = exponent as i64 - fraction.len() as i64;
    if digits.is_empty() {
        return Some(0);
    }
    if shift >= 0 {
        // Anything past nineteen digits exceeds every bound here.
        if digits.len() as i64 + shift > 19 {
            return None;
        }
        let value: i128 = digits.parse().ok()?;
        return i64::try_from(value * 10i128.pow(shift as u32)).ok();
    }
    let cut = (-shift) as usize;
    let (kept, dropped) = if digits.len() > cut {
        digits.split_at(digits.len() - cut)
    } else {
        ("", digits)
    };
    if kept.len() > 19 {
        return None;
    }
    let quotient: i128 = if kept.is_empty() {
        0
    } else {
        kept.parse().ok()?
    };
    // Half up: the first dropped digit, padded to the full cut, decides.
    let first_dropped = if dropped.len() == cut {
        dropped.as_bytes()[0]
    } else {
        b'0'
    };
    let rounded = quotient + i128::from(first_dropped >= b'5');
    i64::try_from(rounded).ok()
}

/// The `/simple/price` request for one ZEC in `currency`, under the API root
/// `origin`: the currency lower-cased, a trailing `/` on `origin` dropped.
pub fn coingecko_price_url(origin: &str, currency: &str) -> String {
    format!(
        "{}/simple/price?ids=zcash&vs_currencies={}",
        origin.trim_end_matches('/'),
        query_encode(&currency.to_lowercase())
    )
}

/// ZEC prices from CoinGecko's `/simple/price` (or a proxy speaking it).
///
/// `origin` is the API root the wallet chose, such as
/// `https://api.coingecko.com/api/v3`: a public host, or one the wallet runs
/// in front of it. Held to the provider's answers by
/// `tests/coingecko_contract.rs`, and against the live service daily by
/// `tools/contracts/coingecko.py --check`.
///
/// A failed fetch is refused with [`HostError::Price`]: an unreachable feed is
/// not an unpriced currency.
pub struct CoinGeckoZecPrices<'a> {
    origin: String,
    transport: &'a dyn HttpTransport,
}

impl<'a> CoinGeckoZecPrices<'a> {
    pub fn new(origin: &str, transport: &'a dyn HttpTransport) -> Self {
        Self {
            origin: origin.trim_end_matches('/').to_owned(),
            transport,
        }
    }

    /// The request for `currency`'s price.
    pub fn request_for(&self, currency: &str) -> String {
        coingecko_price_url(&self.origin, currency)
    }
}

impl ZecPrices for CoinGeckoZecPrices<'_> {
    fn minor_units_per_zec(&self, currency: &str) -> Result<Option<i64>> {
        if !splitz_core::is_currency(currency) || currency_exponent(currency).is_none() {
            return Ok(None);
        }
        let body = self
            .transport
            .get(&self.request_for(currency))
            .map_err(HostError::Price)?;
        price_from_coingecko(&body, currency)
    }
}
