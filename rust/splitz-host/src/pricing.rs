//! What one ZEC costs, and where that figure comes from.

use crate::error::Result;
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
