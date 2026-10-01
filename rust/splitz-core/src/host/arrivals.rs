//! Payments to this device that its wallet has seen arrive (SPEC.md §14.7).
//!
//! A payment record is the payer's claim, and only the payee's confirmation
//! moves a balance (§10.5). A wallet that received the transaction a record
//! names already holds the evidence, so it can propose the confirmation
//! instead of asking the payee to find the payment by hand.

use std::collections::{BTreeMap, BTreeSet};

use crate::model::PaymentRecord;
use crate::ordering::compare_utf8;
use crate::zip321::MAX_ZATOSHI;

use super::bill_log::FoldedBill;

/// A transaction id as section 14.7 compares it: ASCII space, tab, carriage
/// return and line feed removed from both ends, and ASCII letters lower-cased.
///
/// Nothing wider: a txid is hexadecimal, and each language's own `trim` and
/// lower-casing reach different sets of Unicode characters, so one record
/// would match on one device and not on another.
pub fn txid_key(txid: &str) -> String {
    txid.trim_matches(|c| matches!(c, ' ' | '\t' | '\r' | '\n'))
        .to_ascii_lowercase()
}

/// Money this wallet received in one transaction: the sum of that
/// transaction's outputs to this account, in zatoshi.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncomingTransaction {
    pub txid: String,
    pub zatoshi: i64,
}

/// A payment record to this device, and the transaction it names.
#[derive(Debug, Clone, PartialEq)]
pub struct Arrival {
    pub bill_id: String,
    /// The record, as the bill folds it: what the payee is shown before
    /// confirming (§14.2) — its ZEC, the rate it was priced at, its
    /// reference.
    pub payment: PaymentRecord,
    /// The digest a confirmation of `payment` carries as `record` (§10.5).
    pub record: String,
    /// The transaction, lower-cased.
    pub txid: String,
}

/// What [`arrivals_for`] found.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Arrivals {
    /// Records whose transaction arrived carrying at least the ZEC they
    /// state. Each may be confirmed with `walletReceived` once the payee has
    /// been shown it.
    pub arrived: Vec<Arrival>,
    /// Records whose transaction arrived, but with less ZEC than they state
    /// once every other record naming it is counted.
    pub short: Vec<Arrival>,
    /// Records whose transaction arrived and which state no ZEC, so nothing
    /// can be checked against it.
    pub unstated: Vec<Arrival>,
    /// Records naming a transaction that records from another payer also
    /// name. None of them is proposed: a shielded transaction does not say who
    /// sent it, and any participant can copy a reference they have seen, so
    /// the payee has to settle which record it pays before confirming any.
    pub disputed: Vec<Arrival>,
}

fn clamp(zatoshi: i64) -> i64 {
    zatoshi.clamp(0, MAX_ZATOSHI)
}

/// What is left of `left` once `used` of it is spoken for; never below zero.
fn use_up(left: i64, used: i64) -> i64 {
    if used >= left {
        0
    } else {
        left - used
    }
}

/// Who a record says paid, for §14.7's "more than one payer".
#[derive(PartialEq, Eq, PartialOrd, Ord)]
enum Payer<'a> {
    /// The key §10.7 bound to the payer.
    Bound(&'a str),
    /// An unbound payer: the bill, and their id on it.
    Unbound(&'a str, &'a str),
}

/// Matches unconfirmed ZEC payment records to `me` against the transactions
/// `received`, across every one of `bills` at once (§14.7).
///
/// A record matches when its `reference` names a received transaction. The
/// ZEC that transaction brought is counted **once, across all bills**:
/// records already confirmed that name it use their share first, then the
/// rest in order of bill id and payment id. Without that, one real payment
/// recorded on two bills is evidence for both.
///
/// A transaction named by records from more than one payer is evidence for
/// none of them: every such record is [`Arrivals::disputed`].
///
/// Amounts are held inside [0, 21000000 ZEC]: no transaction brings more than
/// exists, and a record's `zatoshi` may be as large as §2.2 allows, so using
/// it up floors at zero rather than wrapping.
pub fn arrivals_for(bills: &[FoldedBill], me: &str, received: &[IncomingTransaction]) -> Arrivals {
    let mut left: BTreeMap<String, i64> = BTreeMap::new();
    for t in received {
        let id = txid_key(&t.txid);
        let sum = left.get(&id).copied().unwrap_or(0);
        left.insert(id, clamp(clamp(sum) + clamp(t.zatoshi)));
    }

    let mut ordered: Vec<&FoldedBill> = bills.iter().collect();
    ordered.sort_by(|a, b| compare_utf8(&a.bill.id, &b.bill.id));
    // Who each received transaction is claimed to be from, over every record
    // to `me` that names it, confirmed or not. A payer is the key §10.7 bound
    // to them, or, unbound, their id on that one bill: an id is chosen by
    // whoever joins, so the same string on two bills can be two people.
    let mut payers: BTreeMap<String, BTreeSet<Payer<'_>>> = BTreeMap::new();
    for folded in &ordered {
        for p in &folded.bill.payments {
            if p.to != me || p.method != "shieldedZec" {
                continue;
            }
            let Some(txid) = p.reference.as_deref().map(txid_key) else {
                continue;
            };
            if left.contains_key(&txid) {
                let payer = match folded.identities.bound.get(&p.from) {
                    Some(key) => Payer::Bound(key),
                    None => Payer::Unbound(&folded.bill.id, &p.from),
                };
                payers.entry(txid).or_default().insert(payer);
            }
        }
    }
    let mut candidates = Vec::new();
    for folded in ordered {
        let mut payments: Vec<&PaymentRecord> = folded.bill.payments.iter().collect();
        payments.sort_by(|a, b| compare_utf8(&a.id, &b.id));
        for p in payments {
            if p.to != me || p.method != "shieldedZec" {
                continue;
            }
            let Some(txid) = p.reference.as_deref().map(txid_key) else {
                continue;
            };
            let Some(available) = left.get(&txid).copied() else {
                continue;
            };
            if folded.bill.confirmed_payments.contains(&p.id) {
                left.insert(txid, use_up(available, p.zatoshi.unwrap_or(0)));
                continue;
            }
            let Some(record) = folded.payment_digests.get(&p.id) else {
                continue;
            };
            candidates.push(Arrival {
                bill_id: folded.bill.id.clone(),
                payment: p.clone(),
                record: record.clone(),
                txid,
            });
        }
    }

    let mut out = Arrivals::default();
    for a in candidates {
        if payers[&a.txid].len() > 1 {
            out.disputed.push(a);
            continue;
        }
        let available = left[&a.txid];
        match a.payment.zatoshi {
            None => out.unstated.push(a),
            Some(stated) if stated <= available => {
                left.insert(a.txid.clone(), use_up(available, stated));
                out.arrived.push(a);
            }
            Some(_) => out.short.push(a),
        }
    }
    out
}
