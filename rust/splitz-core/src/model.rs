//! The bill and the things on it (SPEC.md §9).

use serde_json::Value;
use std::collections::BTreeSet;

use crate::rate::ExchangeRate;

/// How a participant wants to be paid, most preferred first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Payout {
    /// `zec`, `swap` or `cash`. A reader refuses a type it does not define
    /// rather than skipping it: skipping settles to the next preference down,
    /// which is a different address.
    pub kind: String,
    pub address: Option<String>,
    pub asset: Option<String>,
    pub chain: Option<String>,
}

/// Somebody on the bill.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Participant {
    pub id: String,
    pub name: String,
    /// Where money is sent. Absent means a payment request cannot carry an
    /// output for this participant, which §8.5 requires be reported.
    pub pay_to: Option<String>,
    /// The Ed25519 public key that alone may write as this participant
    /// (§10.7). Absent means the identity is unclaimed.
    pub identity_key: Option<String>,
    pub payouts: Vec<Payout>,
}

impl Participant {
    /// The address a payment request can carry, if any.
    ///
    /// Returns the address rather than a flag, so a caller cannot reach for
    /// one that is not there.
    pub fn payable_address(&self) -> Option<&str> {
        // One §8.3 does not admit is not returned: the renderer would refuse
        // the whole request over it, past the caller's choice to report an
        // unpayable recipient instead of refusing.
        self.published_address()
            .filter(|a| crate::zip321::is_zip321_address(a))
    }

    /// The Zcash address this participant published, as written, whether or
    /// not a request can carry it.
    pub fn published_address(&self) -> Option<&str> {
        match self.payouts.first() {
            None => self.pay_to.as_deref(),
            Some(first) if first.kind == "zec" => first.address.as_deref(),
            Some(_) => None,
        }
    }
}

/// A cost somebody covered.
#[derive(Debug, Clone, PartialEq)]
pub struct Expense {
    pub id: String,
    pub description: String,
    pub paid_by: String,
    pub amount: i64,
    pub currency: String,
    pub at: String,
    pub split: Value,
}

/// A claim that a debt was discharged.
#[derive(Debug, Clone, PartialEq)]
pub struct PaymentRecord {
    pub id: String,
    pub from: String,
    pub to: String,
    pub amount: i64,
    pub currency: String,
    /// `shieldedZec`, `swap` or `cash`. A label, not a branch: the ledger
    /// arithmetic is identical whichever happened.
    pub method: String,
    pub at: String,
    /// What left the payer's wallet. Advisory: the fiat `amount` settles the
    /// debt and this takes no part in §5 or §6.
    pub zatoshi: Option<i64>,
    pub paid_at_rate: Option<ExchangeRate>,
    /// What the payment went out as. For `shieldedZec`, the Zcash transaction
    /// (§10.5). For `swap`, the provider's intent id or the transaction on the
    /// destination chain — never a Zcash transaction, whatever it looks like
    /// (§9.2).
    pub reference: Option<String>,
    pub note: Option<String>,
}

/// A bill.
#[derive(Debug, Clone, PartialEq)]
pub struct Bill {
    pub id: String,
    pub name: String,
    /// A bill has exactly one currency (§2.4).
    pub currency: String,
    pub split_mode: String,
    pub participants: Vec<Participant>,
    pub expenses: Vec<Expense>,
    pub payments: Vec<PaymentRecord>,
    /// The ids of the payments §10.5 says are confirmed. A payment not in this
    /// set is a claim and moves no balance.
    pub confirmed_payments: BTreeSet<String>,
    /// Snapshotted, not looked up per device (§7).
    pub rate: Option<ExchangeRate>,
}

impl Bill {
    pub fn participant(&self, id: &str) -> Option<&Participant> {
        self.participants.iter().find(|p| p.id == id)
    }
}
