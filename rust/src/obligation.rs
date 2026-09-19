//! One payer's obligation as a payment request (SPEC.md §8.5).

use crate::error::{code, Result, SplitError};
use crate::model::Bill;
use crate::money::checked_add;
use crate::rate::{fiat_to_zatoshi, ExchangeRate, RateRounding};
use crate::settle::Settlement;
use crate::zip321::{render_uri, FiatPrice, Zip321Payment};

/// A recipient the request cannot carry, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unpayable {
    pub id: String,
    /// `no_address` when nothing is published, `payout_not_zec` when the
    /// preferred payout is a swap or cash. The two need different remedies.
    pub reason: &'static str,
    pub minor_units: i64,
}

/// What one payer's obligation came to.
///
/// Three groups, because a request that silently covers three of a payer's
/// four debts is indistinguishable, to the payer who sends it, from one that
/// settles all four.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Obligation {
    /// None when nothing could be carried.
    pub uri: Option<String>,
    pub payments: Vec<Zip321Payment>,
    pub unpayable: Vec<Unpayable>,
    /// What the URI sends. Never present a figure pricing the whole obligation
    /// as this.
    pub carried_minor_units: i64,
    pub withheld_minor_units: i64,
}

impl Obligation {
    pub fn is_complete(&self) -> bool {
        self.unpayable.is_empty()
    }
}

/// Renders one payer's settlements as a payment request.
///
/// With `skip_unpayable` the payable outputs are rendered and the rest
/// reported; without it a recipient the request cannot carry refuses the whole
/// request. An implementation must not do neither.
pub fn render_obligation(
    settlements: &[Settlement],
    bill: &Bill,
    rate: &ExchangeRate,
    skip_unpayable: bool,
    include_fiat: bool,
) -> Result<Obligation> {
    let mut payments = Vec::new();
    let mut unpayable = Vec::new();
    let mut carried: i64 = 0;
    let mut withheld: i64 = 0;

    for settlement in settlements {
        let Some(who) = bill.participant(&settlement.to) else {
            // A merge or storage fault, needing a different remedy from a
            // missing address.
            return Err(SplitError::new(
                code::UNKNOWN_PARTICIPANT,
                format!(
                    "The plan settles to {}, who is not on this bill",
                    settlement.to
                ),
            ));
        };
        match who.payable_address() {
            None => {
                let reason = if who.payouts.is_empty() {
                    "no_address"
                } else {
                    "payout_not_zec"
                };
                if !skip_unpayable {
                    return Err(SplitError::new(
                        code::ZIP321_NO_ADDRESS,
                        format!(
                            "{} has published no address this request can carry",
                            settlement.to
                        ),
                    ));
                }
                unpayable.push(Unpayable {
                    id: settlement.to.clone(),
                    reason,
                    minor_units: settlement.amount,
                });
                withheld = checked_add(withheld, settlement.amount, code::AMOUNT_OVERFLOW)?;
            }
            Some(address) => {
                payments.push(Zip321Payment {
                    address: address.to_owned(),
                    zatoshi: fiat_to_zatoshi(
                        settlement.amount,
                        rate,
                        Some(&bill.currency),
                        RateRounding::Up,
                    )?,
                    fiat: Some(FiatPrice {
                        currency: bill.currency.clone(),
                        minor_units: settlement.amount,
                    }),
                    label: Some(who.name.clone()),
                    ..Default::default()
                });
                carried = checked_add(carried, settlement.amount, code::AMOUNT_OVERFLOW)?;
            }
        }
    }

    let uri = if payments.is_empty() {
        None
    } else {
        Some(render_uri(&payments, include_fiat)?)
    };

    Ok(Obligation {
        uri,
        payments,
        unpayable,
        carried_minor_units: carried,
        withheld_minor_units: withheld,
    })
}
