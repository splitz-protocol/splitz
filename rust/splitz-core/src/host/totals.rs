//! Where this device stands with each person, summed over every bill it holds.
//!
//! A bill settles on its own (§6), and one person may be on several. A wallet
//! listing bills can also say "you owe Ana 45.00 overall" — per person, per
//! currency, since amounts in two currencies are never added (§2.1).

use std::collections::{BTreeMap, BTreeSet};

use crate::error::{code, Result};
use crate::money::checked_add;
use crate::ordering::compare_utf8;
use crate::settle::{settle_bill, DEFAULT_EXACT_LIMIT};

use super::bill_log::FoldedBill;

/// What this device and one other participant owe each other in one
/// currency, across every bill that names both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Standing {
    /// The other participant. One person is one id across bills only when
    /// the id derives from their key (§10.7); an unsigned participant is a
    /// different id on every bill.
    pub with_id: String,
    pub currency: String,
    /// What their settlement plans ask them to pay this device, in minor
    /// units.
    pub owed_to_me: i64,
    /// What the plans ask this device to pay them.
    pub owed_by_me: i64,
    /// Payments this device recorded to them that they have not confirmed.
    /// Still in `owed_by_me`: a record moves nothing until it is confirmed
    /// (§10.5), so this says what is already on its way.
    pub sent_awaiting: i64,
    /// Payments they recorded to this device that it has not confirmed.
    pub received_awaiting: i64,
    /// The bills this standing sums, in §2.3 order.
    pub bill_ids: Vec<String>,
}

impl Standing {
    /// Positive when they owe this device on balance, negative when it owes
    /// them. Not a sum to settle by: each bill is settled on its own plan.
    /// Both sides are non-negative, so the difference always fits.
    pub fn net(&self) -> i64 {
        self.owed_to_me - self.owed_by_me
    }
}

/// The standings, and the bills that could not be counted.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Totals {
    /// One per other participant and currency, in §2.3 order of id then
    /// currency. A pair with nothing owed and nothing awaiting either way is
    /// left out.
    pub standings: Vec<Standing>,
    /// Bills left out whole, by id, with the §12 code that kept each out: one
    /// that cannot be settled, or one whose amounts would carry a standing
    /// past what an amount can hold. Never partly counted.
    pub uncounted: BTreeMap<String, String>,
}

type Row = [i64; 4];

/// What one bill adds, by (other participant, currency), before any of it is
/// kept.
/// A standing's key: who, the currency, and the bill when §10.7 says the id
/// there is not the person another bill binds it to (empty otherwise).
type Key = (String, String, String);

fn adds_of(
    folded: &FoldedBill,
    me: &str,
    bound_somewhere: &BTreeSet<String>,
) -> Result<BTreeMap<Key, Row>> {
    let bill = &folded.bill;
    let mut adds: BTreeMap<Key, Row> = BTreeMap::new();
    let mut add = |with: &str, slot: usize, amount: i64| -> Result<()> {
        // §10.7: an id is one person across bills only when one key binds it.
        // Where this bill binds it, it is that key's; an id some bill binds
        // and this one does not is, here, whoever wrote a join under it, so
        // it is summed on its own rather than netted against the bound one.
        let whose = match folded.identities.bound.get(with) {
            Some(key) => format!("key:{key}"),
            None if bound_somewhere.contains(with) => bill.id.clone(),
            None => String::new(),
        };
        let row = adds
            .entry((with.to_owned(), bill.currency.clone(), whose))
            .or_insert([0; 4]);
        row[slot] = checked_add(row[slot], amount, code::AMOUNT_OVERFLOW)?;
        Ok(())
    };
    for s in settle_bill(bill, DEFAULT_EXACT_LIMIT)?.settlements {
        if s.to == me && s.from != me {
            add(&s.from, 0, s.amount)?;
        }
        if s.from == me && s.to != me {
            add(&s.to, 1, s.amount)?;
        }
    }
    for p in &bill.payments {
        if bill.confirmed_payments.contains(&p.id) {
            continue;
        }
        // §14.4: what is on its way is what its payer recorded.
        if folded.payment_authors.get(&p.id) != Some(&p.from) {
            continue;
        }
        if p.from == me && p.to != me {
            add(&p.to, 2, p.amount)?;
        }
        if p.to == me && p.from != me {
            add(&p.from, 3, p.amount)?;
        }
    }
    Ok(adds)
}

/// Sums what `me` owes and is owed across `bills` (§6, §10.5).
pub fn totals_across(bills: &[FoldedBill], me: &str) -> Totals {
    let mut sums: BTreeMap<Key, Row> = BTreeMap::new();
    let mut bills_of: BTreeMap<Key, BTreeSet<String>> = BTreeMap::new();
    let mut uncounted = BTreeMap::new();

    let mut ordered: Vec<&FoldedBill> = bills.iter().collect();
    ordered.sort_by(|a, b| compare_utf8(&a.bill.id, &b.bill.id));
    let bound_somewhere: BTreeSet<String> = ordered
        .iter()
        .flat_map(|f| f.identities.bound.keys().cloned())
        .collect();
    for folded in ordered {
        let merged = adds_of(folded, me, &bound_somewhere).map(|adds| {
            let mut merged = Vec::with_capacity(adds.len());
            for (key, row) in adds {
                let held = sums.get(&key).copied().unwrap_or([0; 4]);
                let mut next = [0; 4];
                let mut fits = true;
                for i in 0..4 {
                    match held[i].checked_add(row[i]) {
                        Some(v) => next[i] = v,
                        None => fits = false,
                    }
                }
                if fits {
                    merged.push((key, next));
                } else {
                    // What other bills already hold for this person would
                    // carry the row past what an amount can hold. This bill's
                    // part is kept as a row of its own rather than the bill
                    // left out: one bill stating an absurd figure cannot take
                    // another bill out of the totals.
                    let (with, currency, whose) = key;
                    let apart = if whose.is_empty() {
                        folded.bill.id.clone()
                    } else {
                        format!("{whose}\u{0}{}", folded.bill.id)
                    };
                    merged.push(((with, currency, apart), row));
                }
            }
            merged
        });
        match merged {
            Ok(merged) => {
                for (key, row) in merged {
                    bills_of
                        .entry(key.clone())
                        .or_default()
                        .insert(folded.bill.id.clone());
                    sums.insert(key, row);
                }
            }
            Err(e) => {
                uncounted.insert(folded.bill.id.clone(), e.code.to_owned());
            }
        }
    }

    let mut keys: Vec<&Key> = sums.keys().collect();
    keys.sort_by(|a, b| {
        compare_utf8(&a.0, &b.0)
            .then_with(|| compare_utf8(&a.1, &b.1))
            .then_with(|| compare_utf8(&a.2, &b.2))
    });
    let standings = keys
        .into_iter()
        .filter(|k| sums[*k].iter().any(|v| *v != 0))
        .map(|k| {
            let row = sums[k];
            let mut ids: Vec<String> = bills_of[k].iter().cloned().collect();
            ids.sort_by(|a, b| compare_utf8(a, b));
            Standing {
                with_id: k.0.clone(),
                currency: k.1.clone(),
                owed_to_me: row[0],
                owed_by_me: row[1],
                sent_awaiting: row[2],
                received_awaiting: row[3],
                bill_ids: ids,
            }
        })
        .collect();
    Totals {
        standings,
        uncounted,
    }
}
