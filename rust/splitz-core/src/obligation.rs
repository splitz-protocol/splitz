//! One payer's obligation as a payment request (SPEC.md §8.5).

use std::collections::{BTreeMap, BTreeSet};

use crate::error::{code, Result, SplitError};
use crate::model::Bill;
use crate::money::{checked_add, checked_sum};
use crate::rate::{fiat_to_zatoshi, ExchangeRate, RateRounding};
use crate::settle::Settlement;
use crate::zip321::{render_amount, render_fiat, render_uri, FiatPrice, Zip321Payment};

/// A recipient the request cannot carry, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Unpayable {
    pub id: String,
    /// `no_address` when nothing is published, `bad_address` when what is
    /// published is not an address §8.3 admits, `payout_not_zec` when the
    /// preferred payout is a swap or cash, `unpriceable` when the debt is past
    /// what one request can price at this rate. Each needs a different remedy.
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
    /// The participant each of `payments` pays, in the same order.
    pub recipients: Vec<String>,
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

/// The refusals one output's size produces (§7.1, §8.1, §8.4).
const UNPRICEABLE: [&str; 3] = [
    code::RATE_AMOUNT_TOO_LARGE,
    code::ZIP321_AMOUNT_TOO_LARGE,
    code::ZIP321_FIAT_TOO_MANY_DIGITS,
];

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
    let mut recipients = Vec::new();
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
                let bad = who.published_address().is_some_and(|a| !a.is_empty());
                let reason = if bad {
                    "bad_address"
                } else if who.payouts.is_empty() {
                    "no_address"
                } else {
                    "payout_not_zec"
                };
                if !skip_unpayable {
                    return Err(SplitError::new(
                        if bad {
                            code::ZIP321_BAD_ADDRESS
                        } else {
                            code::ZIP321_NO_ADDRESS
                        },
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
                let fiat = FiatPrice {
                    currency: bill.currency.clone(),
                    minor_units: settlement.amount,
                };
                // Each output is priced, and checked against what §8 renders,
                // on its own: one debt past what a request can carry is that
                // debt's to report, not a reason to carry none of the others.
                let priced = fiat_to_zatoshi(
                    settlement.amount,
                    rate,
                    Some(&bill.currency),
                    RateRounding::Up,
                )
                .and_then(|zatoshi| {
                    render_amount(zatoshi)?;
                    if include_fiat {
                        render_fiat(&fiat)?;
                    }
                    Ok(zatoshi)
                });
                let zatoshi = match priced {
                    Ok(zatoshi) => zatoshi,
                    // Only the refusals one output's size produces. One about
                    // the rate itself refuses every output alike and is raised.
                    Err(e) if skip_unpayable && UNPRICEABLE.contains(&e.code) => {
                        unpayable.push(Unpayable {
                            id: settlement.to.clone(),
                            reason: "unpriceable",
                            minor_units: settlement.amount,
                        });
                        withheld = checked_add(withheld, settlement.amount, code::AMOUNT_OVERFLOW)?;
                        continue;
                    }
                    Err(e) => return Err(e),
                };
                payments.push(Zip321Payment {
                    address: address.to_owned(),
                    zatoshi,
                    fiat: Some(fiat),
                    label: Some(who.name.clone()),
                    ..Default::default()
                });
                recipients.push(settlement.to.clone());
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
        recipients,
        unpayable,
        carried_minor_units: carried,
        withheld_minor_units: withheld,
    })
}

/// A debt held back because a payment to that participant is unconfirmed
/// (section 14.4).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Awaiting {
    /// Who the plan says is owed.
    pub to: String,
    /// What the plan still says is owed. An unconfirmed payment does not
    /// reduce it (section 10.5).
    pub owed: i64,
    /// What this payer has already sent and is waiting to have confirmed.
    /// Less than `owed` when the payment was partial.
    pub paid: i64,
    /// Who that unconfirmed money went to, in ascending id order. Not `to`
    /// when netting rerouted the debt (§6.3): the payment to confirm, or to
    /// take back, is theirs.
    pub paid_to: Vec<String>,
}

/// One payer's settlements, split into what a request may carry and what
/// section 14 holds back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Withholdings {
    /// Safe to render. Still subject to section 8.4: a participant here may
    /// have no payout address, which `render_obligation` reports as unpayable.
    pub carried: Vec<Settlement>,
    pub awaiting: Vec<Awaiting>,
}

/// Splits `payer`'s settlements into what may be requested and what section
/// 14.4 holds back.
///
/// Pure: it reads the bill, and decides nothing a wallet is entitled to
/// decide. Given `recorded_by` — the fold's author of each payment record —
/// only a record the payer wrote withholds anything.
pub fn withholdings(
    plan: &[Settlement],
    bill: &Bill,
    payer: &str,
    recorded_by: Option<&BTreeMap<String, String>>,
) -> Result<Withholdings> {
    // Section 10.5: only a confirmed payment moves a balance, so a debt this
    // payer has already paid is still in the plan. Records to one id sum.
    let mut pending: BTreeMap<&str, i64> = BTreeMap::new();
    for p in &bill.payments {
        if p.from != payer || bill.confirmed_payments.contains(&p.id) {
            continue;
        }
        // A record somebody else wrote is their word, not a payment this
        // payer has in flight.
        if recorded_by.is_some_and(|by| by.get(&p.id).map(String::as_str) != Some(payer)) {
            continue;
        }
        let held = pending.entry(p.to.as_str()).or_insert(0);
        *held = checked_add(*held, p.amount, code::AMOUNT_OVERFLOW)?;
    }

    let mut carried = Vec::new();
    let mut awaiting = Vec::new();
    for s in plan.iter().filter(|s| s.from == payer) {
        // The payee, and every creditor whose debt this settlement covers
        // (§6.3): netting can reroute a debt already paid onto somebody else.
        let mut owed_to: BTreeSet<&str> = BTreeSet::new();
        owed_to.insert(s.to.as_str());
        owed_to.extend(s.covers.iter().map(|c| c.to.as_str()));
        let paid_to: Vec<&str> = owed_to
            .iter()
            .copied()
            .filter(|t| pending.contains_key(t))
            .collect();
        if !paid_to.is_empty() {
            awaiting.push(Awaiting {
                to: s.to.clone(),
                owed: s.amount,
                paid: checked_sum(paid_to.iter().map(|t| pending[t]), code::AMOUNT_OVERFLOW)?,
                paid_to: paid_to.iter().map(|t| (*t).to_owned()).collect(),
            });
        } else {
            carried.push(s.clone());
        }
    }
    Ok(Withholdings { carried, awaiting })
}
