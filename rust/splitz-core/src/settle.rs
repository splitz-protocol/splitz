//! Settlement (SPEC.md §6).
//!
//! Balances are netted, then partitioned into as many zero-sum groups as
//! possible. A group of `k` needs exactly `k−1` payments, so maximising groups
//! minimises the total.

use std::collections::BTreeMap;

use crate::balances::{direct_debts, net_balances, DirectDebt};
use crate::error::{code, Result, SplitError};
use crate::model::Bill;

/// The default number of non-zero participants solved exactly.
pub const DEFAULT_EXACT_LIMIT: usize = 14;

/// The ceiling on the exact limit.
///
/// The partition search allocates `2^n` entries and costs `3^n`. The ceiling
/// is normative because the failure past it is silent: at 26 the search does
/// not finish, and at 64 the shift overflows and the search returns an empty
/// plan that still reports itself optimal.
pub const MAX_EXACT_LIMIT: usize = 20;

/// One payment in a plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settlement {
    pub from: String,
    pub to: String,
    pub amount: i64,
    /// The original debts this payment discharges (§6.3).
    ///
    /// Empty for a plan computed from balances: net balances do not carry the
    /// debts that produced them. [`settle_bill`] populates it.
    pub covers: Vec<DirectDebt>,
}

impl Settlement {
    /// The part of this payment no direct debt explains (§6.3).
    ///
    /// Anybody holding the invite may write an expense, and §4 admits a
    /// negative total, so a peer can attribute a refund to somebody who never
    /// agreed to it. The victim's settlement then exceeds every debt the bill
    /// records for them.
    ///
    /// Refused with `amount_overflow` when the covers, or the amount less
    /// them, leave the signed 64-bit range: a settlement handed in across a
    /// boundary is not one §6 produced.
    pub fn unexplained(&self) -> Result<i64> {
        let covered =
            crate::money::checked_sum(self.covers.iter().map(|c| c.amount), code::AMOUNT_OVERFLOW)?;
        Ok(crate::money::checked_sub(self.amount, covered, code::AMOUNT_OVERFLOW)?.max(0))
    }

    /// Whether any part of this payment discharges a debt owed to somebody
    /// other than `to` (§6.3).
    ///
    /// Membership is not the test. A payment covering a little of what the
    /// payee lent and a great deal of what two other people lent is still a
    /// payment the payer cannot account for by looking at the payee.
    pub fn is_rerouted(&self) -> bool {
        self.covers
            .iter()
            .filter(|c| c.to != self.to)
            .map(|c| c.amount)
            .sum::<i64>()
            > 0
    }
}

/// A whole plan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SettlementPlan {
    pub settlements: Vec<Settlement>,
    /// False when the partition search was not run. A plan that overshoots by
    /// a payment is acceptable; one that claims minimality it has not
    /// established is not.
    pub is_optimal: bool,
}

impl SettlementPlan {
    pub fn payment_count(&self) -> usize {
        self.settlements.len()
    }
}

/// Plans the fewest payments that clear `net`.
pub fn settle_balances(net: &BTreeMap<String, i64>, exact_limit: usize) -> Result<SettlementPlan> {
    if exact_limit > MAX_EXACT_LIMIT {
        return Err(SplitError::new(
            code::EXACT_LIMIT_TOO_LARGE,
            format!("An exact limit of {exact_limit} exceeds {MAX_EXACT_LIMIT}"),
        ));
    }

    // §5.1: balances reaching the search sum to zero, checked without wrapping.
    // A balance of i64::MIN has no positive counterpart, so no settlement
    // amount can carry it (§2.2, §8.1).
    if net.values().any(|v| *v == i64::MIN) {
        return Err(SplitError::new(
            code::AMOUNT_OVERFLOW,
            "A balance of i64::MIN cannot be settled: no payment can carry it",
        ));
    }

    // §5.1, decided by cancellation so no value the set does not contain is
    // formed.
    if !crate::balances::residual_is_zero(net.values().copied()) {
        return Err(SplitError::new(
            code::BALANCES_NONZERO_RESIDUAL,
            "Net balances do not sum to zero",
        ));
    }

    let ids: Vec<String> = net
        .iter()
        .filter(|(_, v)| **v != 0)
        .map(|(k, _)| k.clone())
        .collect();
    let values: Vec<i64> = ids.iter().map(|id| net[id]).collect();
    let is_optimal = ids.len() <= exact_limit;
    let groups = zero_sum_groups(&values, exact_limit)?;

    let mut settlements: Vec<Settlement> = Vec::new();
    for group in groups {
        let mut balance: BTreeMap<&str, i64> = group
            .iter()
            .map(|&i| (ids[i].as_str(), values[i]))
            .collect();
        loop {
            let mut owing: Vec<(&str, i64)> = balance
                .iter()
                .filter(|(_, v)| **v < 0)
                .map(|(k, v)| (*k, *v))
                .collect();
            let mut owed: Vec<(&str, i64)> = balance
                .iter()
                .filter(|(_, v)| **v > 0)
                .map(|(k, v)| (*k, *v))
                .collect();
            if owing.is_empty() || owed.is_empty() {
                break;
            }
            owing.sort_by(|a, b| a.1.cmp(&b.1).then(a.0.as_bytes().cmp(b.0.as_bytes())));
            owed.sort_by(|a, b| b.1.cmp(&a.1).then(a.0.as_bytes().cmp(b.0.as_bytes())));

            let (debtor, debt) = owing[0];
            let (creditor, credit) = owed[0];
            let amount = (-debt).min(credit);
            settlements.push(Settlement {
                from: debtor.to_owned(),
                to: creditor.to_owned(),
                amount,
                covers: Vec::new(),
            });
            *balance.get_mut(debtor).expect("a debtor") += amount;
            *balance.get_mut(creditor).expect("a creditor") -= amount;
        }
    }

    settlements.sort_by(|a, b| {
        a.from
            .as_bytes()
            .cmp(b.from.as_bytes())
            .then(a.to.as_bytes().cmp(b.to.as_bytes()))
    });
    Ok(SettlementPlan {
        settlements,
        is_optimal,
    })
}

/// Partitions indices into as many zero-sum groups as possible.
fn zero_sum_groups(values: &[i64], exact_limit: usize) -> Result<Vec<Vec<usize>>> {
    let n = values.len();
    if n == 0 {
        return Ok(Vec::new());
    }
    if n > exact_limit {
        return Ok(vec![(0..n).collect()]);
    }

    let full = (1usize << n) - 1;
    // A subset whose sum cannot be formed in a signed 64-bit integer is not
    // zero and not a group — but the bill around it may settle perfectly, so
    // it is skipped rather than refused. Tracked per subset: a bound over the
    // whole set depends on an order whoever joined chose.
    let mut sums = vec![0i64; 1 << n];
    let mut exact = vec![true; 1 << n];
    for mask in 1..=full {
        let low = mask & mask.wrapping_neg();
        let rest = mask ^ low;
        if !exact[rest] {
            exact[mask] = false;
            continue;
        }
        match sums[rest].checked_add(values[low.trailing_zeros() as usize]) {
            Some(total) => sums[mask] = total,
            None => exact[mask] = false,
        }
    }

    let mut best = vec![0usize; 1 << n];
    let mut pick = vec![0usize; 1 << n];
    for mask in 1..=full {
        let lowest = mask & mask.wrapping_neg();
        let mut sub = mask;
        while sub != 0 {
            if sub & lowest != 0 && exact[sub] && sums[sub] == 0 {
                let candidate = best[mask ^ sub] + 1;
                if candidate > best[mask] {
                    best[mask] = candidate;
                    pick[mask] = sub;
                }
            }
            sub = (sub - 1) & mask;
        }
        if pick[mask] == 0 {
            pick[mask] = mask;
        }
    }

    let mut groups = Vec::new();
    let mut mask = full;
    while mask != 0 {
        let sub = pick[mask];
        groups.push((0..n).filter(|i| sub >> i & 1 == 1).collect());
        mask ^= sub;
    }
    Ok(groups)
}

/// Plans the fewest payments that clear `bill`.
pub fn settle_bill(bill: &Bill, exact_limit: usize) -> Result<SettlementPlan> {
    let plan = settle_balances(&net_balances(bill)?, exact_limit)?;
    Ok(SettlementPlan {
        settlements: attribute_coverage(plan.settlements, &direct_debts(bill)?),
        is_optimal: plan.is_optimal,
    })
}

/// Attributes each settlement to the original debts it discharges (§6.3).
///
/// A payer's direct debts, in their §5.2 order, are consumed against that
/// payer's settlements in plan order, each settlement taking as much of each
/// remaining debt as it needs.
pub fn attribute_coverage(settlements: Vec<Settlement>, debts: &[DirectDebt]) -> Vec<Settlement> {
    // Rows are addressed by index into `debts` so that two identical (debtor,
    // creditor, amount) rows stay distinct and are drawn down separately.
    let mut rows_of: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (i, d) in debts.iter().enumerate() {
        rows_of.entry(&d.from).or_default().push(i);
    }
    let mut left: Vec<i64> = debts.iter().map(|d| d.amount).collect();

    settlements
        .into_iter()
        .map(|s| {
            let mut need = s.amount;
            let mut covers = Vec::new();
            for &i in rows_of.get(s.from.as_str()).unwrap_or(&Vec::new()) {
                if need <= 0 {
                    break;
                }
                // A pair aggregating to a negative amount is a credit on that
                // pair.
                if left[i] <= 0 {
                    continue;
                }
                let take = need.min(left[i]);
                covers.push(DirectDebt {
                    from: s.from.clone(),
                    to: debts[i].to.clone(),
                    amount: take,
                });
                left[i] -= take;
                need -= take;
            }
            Settlement { covers, ..s }
        })
        .collect()
}
