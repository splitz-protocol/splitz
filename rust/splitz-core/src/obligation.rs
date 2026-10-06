//! One payer's obligation as a payment request (SPEC.md §8.5).

use std::collections::BTreeMap;

use crate::address::parse_address;
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
    render_reading(
        settlements,
        bill,
        rate,
        skip_unpayable,
        include_fiat,
        &|_| true,
    )
}

/// [`render_obligation`], with an address `reads_address` answers false for
/// reported as one the request cannot carry (§14.6).
pub(crate) fn render_reading(
    settlements: &[Settlement],
    bill: &Bill,
    rate: &ExchangeRate,
    skip_unpayable: bool,
    include_fiat: bool,
    reads_address: &dyn Fn(&str) -> bool,
) -> Result<Obligation> {
    let mut payments = Vec::new();
    let mut recipients = Vec::new();
    let mut unpayable = Vec::new();
    let mut carried: i64 = 0;
    let mut withheld: i64 = 0;

    // §8.5: a request carries one payer's debts. Built from a whole plan it
    // would ask this payer to send every other payer's too.
    if settlements.iter().any(|s| s.from != settlements[0].from) {
        return Err(SplitError::new(
            code::OBLIGATION_MIXED_PAYERS,
            "These settlements are owed by more than one payer",
        ));
    }

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
        // §14.6: an address the payer's own reader cannot read is one the
        // whole request fails on, so it is reported like any other it cannot
        // carry.
        match who.payable_address().filter(|a| reads_address(a)) {
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
                    // §8.5: what ties the send to this bill, where the address
                    // takes one.
                    memo: takes_memo(address).then(|| bill_memo(&bill.id)),
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
    /// What other payers have sent `to` and is waiting to be confirmed, each
    /// record written by its own payer, when that is why the debt is held: it
    /// already covers what the plan still owes `to`. Zero otherwise, and
    /// `paid` is then this payer's own.
    pub others_paid: i64,
}

/// `bill` with each participant `via` names paid by the payout chosen for
/// them (§14.8).
///
/// `via` maps a participant id to the index of one of their declared payouts.
/// That payout moves to the front and the rest keep their order; everything
/// else on the bill is unchanged. Rendering a request from the result carries
/// a chosen `zec` payout's address. Ids are checked in §2.3's order: one not
/// on the bill is refused with `unknown_participant`, an index outside what
/// that participant declared with `payout_not_declared`.
pub fn choose_payouts(bill: &Bill, via: &BTreeMap<String, i64>) -> Result<Bill> {
    // A `BTreeMap<String, _>` iterates in byte order, which is §2.3's.
    let mut chosen = BTreeMap::new();
    for (id, &index) in via {
        let Some(who) = bill.participant(id) else {
            return Err(SplitError::new(
                code::UNKNOWN_PARTICIPANT,
                format!("{id} is not on this bill"),
            ));
        };
        let declared = who.payouts.len();
        let Some(at) = usize::try_from(index).ok().filter(|&i| i < declared) else {
            return Err(SplitError::new(
                code::PAYOUT_NOT_DECLARED,
                format!("{id} declared {declared} payouts, not one at {index}"),
            ));
        };
        chosen.insert(id.as_str(), at);
    }
    let mut out = bill.clone();
    for p in &mut out.participants {
        if let Some(&at) = chosen.get(p.id.as_str()) {
            let first = p.payouts.remove(at);
            p.payouts.insert(0, first);
        }
    }
    Ok(out)
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

    // What was paid to each creditor beyond this payer's own settlement to
    // them. A payment is that settlement's first; only the rest can be a debt
    // netting moved onto somebody else.
    let mut own: BTreeMap<&str, i64> = BTreeMap::new();
    for s in plan.iter().filter(|s| s.from == payer) {
        let sum = own.entry(s.to.as_str()).or_insert(0);
        *sum = checked_add(*sum, s.amount, code::AMOUNT_OVERFLOW)?;
    }
    let beyond: BTreeMap<&str, i64> = pending
        .iter()
        .filter_map(|(t, paid)| {
            let mine = own.get(t).copied().unwrap_or(0);
            (*paid > mine).then(|| (*t, paid - mine))
        })
        .collect();

    // What the request may still carry: the payer's debt less everything
    // pending. Netting can move a debt already paid onto a creditor no
    // settlement's covers name, and only this bound stops it being asked for
    // again. Negative when later expenses left more pending than is owed.
    let mut room = checked_sum(own.values().copied(), code::AMOUNT_OVERFLOW)?
        - checked_sum(pending.values().copied(), code::AMOUNT_OVERFLOW)?;
    let beyond_paid = checked_sum(beyond.values().copied(), code::AMOUNT_OVERFLOW)?;

    // What every other payer has in flight to each payee, each record written
    // by its own payer, against what the plan still owes that payee. §6 plans
    // from confirmed balances, so a confirmation can move a debt onto a payee
    // another payer is already paying; asked for again, they are paid twice.
    let mut credit: BTreeMap<&str, i64> = BTreeMap::new();
    for s in plan {
        let held = credit.entry(s.to.as_str()).or_insert(0);
        *held = checked_add(*held, s.amount, code::AMOUNT_OVERFLOW)?;
    }
    let mut inbound: BTreeMap<&str, i64> = BTreeMap::new();
    for p in &bill.payments {
        if p.from == payer || bill.confirmed_payments.contains(&p.id) {
            continue;
        }
        if let Some(authors) = recorded_by {
            if authors.get(&p.id) != Some(&p.from) {
                continue;
            }
        }
        let held = inbound.entry(p.to.as_str()).or_insert(0);
        *held = checked_add(*held, p.amount, code::AMOUNT_OVERFLOW)?;
    }
    let mut left: BTreeMap<&str, i64> = credit
        .iter()
        .map(|(to, owed)| (*to, owed - inbound.get(to).copied().unwrap_or(0)))
        .collect();

    let mut carried = Vec::new();
    let mut awaiting = Vec::new();
    for s in plan.iter().filter(|s| s.from == payer) {
        // The payee's own pending payments, and what was paid beyond their
        // own settlement to any other creditor this one covers (§6.3):
        // netting can reroute a debt already paid onto somebody else.
        let mut held: BTreeMap<&str, i64> = BTreeMap::new();
        if let Some(paid) = pending.get(s.to.as_str()) {
            held.insert(s.to.as_str(), *paid);
        }
        for c in &s.covers {
            if c.to != s.to {
                if let Some(extra) = beyond.get(c.to.as_str()) {
                    held.insert(c.to.as_str(), *extra);
                }
            }
        }
        if !held.is_empty() {
            awaiting.push(Awaiting {
                to: s.to.clone(),
                owed: s.amount,
                paid: checked_sum(held.values().copied(), code::AMOUNT_OVERFLOW)?,
                paid_to: held.keys().map(|t| (*t).to_owned()).collect(),
                others_paid: 0,
            });
        } else if s.amount > room {
            // Only money paid beyond some settlement can leave too little
            // room: what was paid within one is that settlement's, and it is
            // held above.
            awaiting.push(Awaiting {
                to: s.to.clone(),
                owed: s.amount,
                paid: beyond_paid,
                paid_to: beyond.keys().map(|t| (*t).to_owned()).collect(),
                others_paid: 0,
            });
        } else if s.amount > left.get(s.to.as_str()).copied().unwrap_or(0) {
            // Other payers' records to this payee cover what the plan still
            // owes them: held until those are confirmed or withdrawn.
            awaiting.push(Awaiting {
                to: s.to.clone(),
                owed: s.amount,
                paid: 0,
                paid_to: Vec::new(),
                others_paid: inbound.get(s.to.as_str()).copied().unwrap_or(0),
            });
        } else {
            room -= s.amount;
            if let Some(remaining) = left.get_mut(s.to.as_str()) {
                *remaining -= s.amount;
            }
            carried.push(s.clone());
        }
    }
    Ok(Withholdings { carried, awaiting })
}

/// The memo a request carries to every output that takes one (§8.5): the
/// UTF-8 bytes of `splitz:` and the bill's id. The payee's wallet reads it
/// back to tell a payment for this bill from one sent for anything else
/// (§14.7).
pub fn bill_memo(bill_id: &str) -> Vec<u8> {
    format!("splitz:{bill_id}").into_bytes()
}

/// Whether `address` decodes (§8.6) to one that can receive a memo. One that
/// does not decode carries none: §8.3 admits strings no reader decodes, and a
/// request to one still renders.
fn takes_memo(address: &str) -> bool {
    parse_address(address).is_ok_and(|a| a.can_receive_memo)
}
