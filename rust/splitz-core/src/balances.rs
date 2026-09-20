//! Net positions and the debts as they arose (SPEC.md §5).

use std::collections::BTreeMap;

use crate::error::{code, Result, SplitError};
use crate::model::Bill;
use crate::money::checked_add;
use crate::split::split_expense;

/// Whether a set of balances sums to zero (§5.1), decided by cancellation.
///
/// A running total, or the sum of the positives, forms a value the set need
/// not contain: both exceed a signed 64-bit integer for sets whose residual is
/// zero and whose every member is representable. Cancelling largest against
/// largest never forms one, and the answer does not depend on the order a map
/// happens to yield.
pub fn residual_is_zero(values: impl IntoIterator<Item = i64>) -> bool {
    let mut owed: Vec<i64> = Vec::new();
    let mut owes: Vec<i64> = Vec::new();
    for v in values {
        if v > 0 {
            owed.push(v);
        } else if v < 0 {
            // i64::MIN has no positive counterpart, so it cannot be cancelled
            // against anything; such a set never balances.
            match v.checked_neg() {
                Some(m) => owes.push(m),
                None => return false,
            }
        }
    }
    owed.sort_unstable_by(|a, b| b.cmp(a));
    owes.sort_unstable_by(|a, b| b.cmp(a));

    let (mut i, mut j) = (0usize, 0usize);
    let (mut carry_owed, mut carry_owes) = (0i64, 0i64);
    while (i < owed.len() || carry_owed > 0) && (j < owes.len() || carry_owes > 0) {
        let a = if carry_owed > 0 {
            carry_owed
        } else {
            i += 1;
            owed[i - 1]
        };
        let b = if carry_owes > 0 {
            carry_owes
        } else {
            j += 1;
            owes[j - 1]
        };
        carry_owed = if a > b { a - b } else { 0 };
        carry_owes = if b > a { b - a } else { 0 };
    }
    i == owed.len() && j == owes.len() && carry_owed == 0 && carry_owes == 0
}

/// A participant and what the bill owes them, or they it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Position {
    pub id: String,
    pub amount: i64,
}

/// One debt, as it arose, before any netting.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DirectDebt {
    pub from: String,
    pub to: String,
    pub amount: i64,
}

/// Net balances for every participant, including those who net to zero.
///
/// Only a **confirmed** payment (§10.5) moves a balance: one recorded and not
/// yet confirmed is a claim, and the person who owes the money is the one
/// making it.
pub fn net_balances(bill: &Bill) -> Result<BTreeMap<String, i64>> {
    let mut net: BTreeMap<String, i64> = bill
        .participants
        .iter()
        .map(|p| (p.id.clone(), 0))
        .collect();

    for expense in &bill.expenses {
        let shares = split_expense(expense.amount, &expense.split)?;
        for id in shares.keys() {
            if !net.contains_key(id) {
                return Err(SplitError::new(
                    code::UNKNOWN_PARTICIPANT,
                    format!("An expense splits to {id}, who is not on this bill"),
                ));
            }
        }
        let payer = net
            .get_mut(&expense.paid_by)
            .expect("the payer is on the bill");
        *payer = checked_add(*payer, expense.amount, code::AMOUNT_OVERFLOW)?;
        for (id, owed) in &shares {
            let entry = net.get_mut(id).expect("checked above");
            *entry = checked_add(*entry, -owed, code::AMOUNT_OVERFLOW)?;
        }
    }

    for payment in &bill.payments {
        if !bill.confirmed_payments.contains(&payment.id) {
            continue;
        }
        let from = net
            .get_mut(&payment.from)
            .expect("the payer is on the bill");
        *from = checked_add(*from, payment.amount, code::AMOUNT_OVERFLOW)?;
        let to = net.get_mut(&payment.to).expect("the payee is on the bill");
        *to = checked_add(*to, -payment.amount, code::AMOUNT_OVERFLOW)?;
    }

    // An expense moves this sum by zero because every split sums to its total,
    // and a payment credits and debits the same amount. A non-zero residual is
    // this crate's own arithmetic having gone wrong, not a malformed input,
    // which is why SPEC.md §12 names it as the one code with no vector.
    if !residual_is_zero(net.values().copied()) {
        return Err(SplitError::new(
            code::BALANCES_NONZERO_RESIDUAL,
            "Net balances do not sum to zero",
        ));
    }
    Ok(net)
}

/// The positive balances, most owed first, ties by ascending id.
pub fn creditors(net: &BTreeMap<String, i64>) -> Vec<Position> {
    let mut rows: Vec<Position> = net
        .iter()
        .filter(|(_, v)| **v > 0)
        .map(|(k, v)| Position {
            id: k.clone(),
            amount: *v,
        })
        .collect();
    rows.sort_by(|a, b| {
        b.amount
            .cmp(&a.amount)
            .then(a.id.as_bytes().cmp(b.id.as_bytes()))
    });
    rows
}

/// The negative balances, largest debt first, ties by ascending id.
pub fn debtors(net: &BTreeMap<String, i64>) -> Vec<Position> {
    let mut rows: Vec<Position> = net
        .iter()
        .filter(|(_, v)| **v < 0)
        .map(|(k, v)| Position {
            id: k.clone(),
            amount: *v,
        })
        .collect();
    rows.sort_by(|a, b| {
        a.amount
            .cmp(&b.amount)
            .then(a.id.as_bytes().cmp(b.id.as_bytes()))
    });
    rows
}

/// The debts the bill created, before netting (§5.2).
///
/// Recorded payments are not subtracted: §6.3 reads these to explain a
/// rerouted payment.
pub fn direct_debts(bill: &Bill) -> Result<Vec<DirectDebt>> {
    // Keyed by debtor then creditor, so no separator can collide with an id.
    let mut pairs: BTreeMap<String, BTreeMap<String, i64>> = BTreeMap::new();
    for expense in &bill.expenses {
        let shares = split_expense(expense.amount, &expense.split)?;
        for (id, owed) in &shares {
            if *id == expense.paid_by || *owed == 0 {
                continue;
            }
            let row = pairs.entry(id.clone()).or_default();
            let entry = row.entry(expense.paid_by.clone()).or_insert(0);
            *entry = checked_add(*entry, *owed, code::AMOUNT_OVERFLOW)?;
        }
    }

    let mut rows = Vec::new();
    for (debtor, creditors) in &pairs {
        for (creditor, amount) in creditors {
            if *amount != 0 {
                rows.push(DirectDebt {
                    from: debtor.clone(),
                    to: creditor.clone(),
                    amount: *amount,
                });
            }
        }
    }
    Ok(rows)
}
