//! Paying what this device owes, and recording that it did.
//!
//! The protocol renders the payment request; the wallet sends it. What sits
//! between them is the part that has to survive a person walking away
//! mid-send, and that is what this file is for.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

use crate::error::Result;
use crate::obligation::{
    render_obligation, withholdings, Awaiting, Contested, Obligation, Unpayable,
};
use crate::settle::{settle_bill, Settlement, DEFAULT_EXACT_LIMIT};

use super::bill_log::{BillLog, FoldedBill};
use super::entries::{record_payment, sign_entry};
use super::host::{BillHost, SendResult, Sent};

/// What this device owes, and the request that carries it.
///
/// Wraps the protocol's own [`Obligation`] rather than restating it: the
/// carried and withheld totals, the payments and the unpayable recipients are
/// its answers, and a second copy of them here would be a second thing to keep
/// right.
#[derive(Debug, Clone, PartialEq)]
pub struct PayerObligation {
    /// The settlements this device is the payer of and the request carries.
    pub settlements: Vec<Settlement>,

    /// Debts this device has already paid, whose payee has not yet confirmed.
    ///
    /// Left out of `settlements` and out of the request. A payment record does
    /// not discharge a debt (§10.5) — the payee says when the money arrived —
    /// so these are still in the plan, and asking for them again would send
    /// the same money a second time.
    ///
    /// A payment that never landed sits here too, and no retry goes through
    /// this path until its record is voided.
    pub awaiting: Vec<Awaiting>,

    /// Debts to a participant whose identity two keys claim.
    ///
    /// Left out of `settlements` and out of the request. The payer decides,
    /// having been shown the contest — nothing here decides for them by paying
    /// whichever record happens to be on the bill.
    pub contested: Vec<Contested>,

    /// The protocol's answer: the request, what it carries, and who it could
    /// not carry with the reason for each.
    pub request: Obligation,
}

impl PayerObligation {
    pub fn uri(&self) -> Option<&str> {
        self.request.uri.as_deref()
    }

    /// Who the request could not carry, and why: `no_address` when nothing is
    /// published, `bad_address` when what is published is not an address
    /// §8.3 admits, `payout_not_zec` when the preferred payout is a swap or
    /// cash. Each needs a different remedy (§8.5).
    pub fn unpayable(&self) -> &[Unpayable] {
        &self.request.unpayable
    }

    /// True when every debt this device owes is in the request. A request that
    /// covers less must say so: the payer cannot tell from the URI (§8.5).
    pub fn is_complete(&self) -> bool {
        self.request.unpayable.is_empty() && !self.settlements.is_empty()
    }

    /// What the request will move.
    pub fn carried_minor_units(&self) -> i64 {
        self.request.carried_minor_units
    }

    /// What it leaves outstanding.
    pub fn withheld_minor_units(&self) -> i64 {
        self.request.withheld_minor_units
    }

    /// What the request pays each recipient, in minor units: the amounts a
    /// send records, one record per recipient.
    ///
    /// **What the request carries, not what the payer owes.** The two differ
    /// whenever a recipient's preferred payout is not a Zcash address: §8.5
    /// leaves them out of the URI and reports them, and recording them would
    /// claim a transaction settled a debt it never paid. Settlements to one
    /// recipient sum, as they do in §14.
    pub fn carried_to(&self) -> BTreeMap<String, i64> {
        let unpayable: BTreeSet<&str> = self
            .request
            .unpayable
            .iter()
            .map(|u| u.id.as_str())
            .collect();
        let mut owed: BTreeMap<String, i64> = BTreeMap::new();
        for settlement in &self.settlements {
            if unpayable.contains(settlement.to.as_str()) {
                continue;
            }
            *owed.entry(settlement.to.clone()).or_insert(0) += settlement.amount;
        }
        owed
    }
}

/// What a settlement attempt produced.
///
/// `records` is non-empty only when `result` is [`SendResult::Sent`]. A send
/// that is still pending records nothing and is not a failure either: the
/// caller shows `detail` and does not retry until the wallet resolves it.
#[derive(Debug, Clone, PartialEq)]
pub struct Settled {
    pub result: SendResult,
    /// The transaction id, present when `result` is [`SendResult::Sent`].
    pub txid: Option<String>,
    /// What to put in front of a person when nothing was recorded.
    pub detail: Option<String>,
    /// The payment entries, one per recipient. Already appended to the log —
    /// returned so a caller can hand them to a peer, not so a caller can
    /// decide whether to keep them.
    pub records: Vec<Value>,
}

/// Reads what this device owes on `folded`.
///
/// Returns `None` when the bill carries no rate: an unpriced bill is an
/// ordinary bill, not a refusal, and there is no §12 code to catch.
pub fn obligation_for(
    host: &dyn BillHost,
    folded: &FoldedBill,
    pay_anyway: &BTreeSet<String>,
) -> Result<Option<PayerObligation>> {
    let Some(rate) = folded.bill.rate.as_ref() else {
        return Ok(None);
    };

    // §14 decides this, not this layer: which debts a request may carry, and
    // which wait on a confirmation or on a contest, is a function of the bill
    // and the identities the fold resolved. A second implementation here is a
    // second place for the rule to drift.
    let plan = settle_bill(&folded.bill, DEFAULT_EXACT_LIMIT)?;
    let split = withholdings(
        &plan.settlements,
        &folded.bill,
        host.me(),
        &folded.identities.contested,
        pay_anyway,
        Some(&folded.payment_authors),
    )?;

    // `render_obligation` is the protocol's own answer to the hazard in §8.5:
    // either refuse the whole request, or render what can be carried and
    // report the rest. Writing the loop by hand is how a wallet ends up doing
    // neither.
    let request = render_obligation(&split.carried, &folded.bill, rate, true, false)?;

    Ok(Some(PayerObligation {
        settlements: split.carried,
        awaiting: split.awaiting,
        contested: split.contested,
        request,
    }))
}

/// The id of the payment record for `to`'s share of the transaction `txid`.
///
/// One transaction paying several people is several records, and §10.5
/// requires each to carry its own id: a confirmation names one record, so two
/// under one id would let one recipient's word settle a debt another never
/// vouched for, and the fold sets the second aside as `duplicate_payment` —
/// losing the record of a payment that was made. The transaction itself goes
/// in the record's `reference`, which is what `onChain` reads.
pub fn payment_id_for_send(txid: &str, to: &str) -> String {
    format!("{txid}:{to}")
}

/// Sends `obligation` and records that it was sent.
///
/// **Everything the record needs is read before the broadcast.** A send that
/// lands while its record is lost leaves the bill showing a debt that is paid
/// and the payee never seeing the payment, and the transaction cannot be
/// unsent. Nothing here reads state after the wallet is called.
///
/// A record is a claim, not a settlement (§10.5): the balance does not move
/// until somebody confirms it.
pub fn settle(
    host: &dyn BillHost,
    log: &mut BillLog<'_>,
    obligation: &PayerObligation,
) -> Result<Settled> {
    let Some(uri) = obligation.uri() else {
        return Ok(Settled {
            result: SendResult::Failed,
            txid: None,
            detail: Some("there is nothing to send".to_owned()),
            records: Vec::new(),
        });
    };

    // Captured first, deliberately: after `broadcast` returns, this device may
    // be anywhere.
    let owed = obligation.carried_to();
    if owed.is_empty() {
        return Ok(Settled {
            result: SendResult::Failed,
            txid: None,
            detail: Some("there is nothing this request can carry".to_owned()),
            records: Vec::new(),
        });
    }

    let sent: Sent = host.broadcast(uri);

    // A transaction that was built but not broadcast may still land. Recording
    // it as paid would settle a debt nothing on chain has settled; recording
    // nothing and letting a retry through would pay it twice. Neither is
    // chosen here — the caller is told, and the debt stays exactly as it was.
    if sent.result != SendResult::Sent {
        return Ok(Settled {
            result: sent.result,
            txid: None,
            detail: sent.detail,
            records: Vec::new(),
        });
    }

    let Some(txid) = sent.txid else {
        // `Sent::sent` cannot produce this; a hand-built `Sent` can. Pending,
        // not failed: the wallet said money left, so a retry could pay twice.
        return Ok(Settled {
            result: SendResult::Pending,
            txid: None,
            detail: Some("the wallet reported a send with no transaction id".to_owned()),
            records: Vec::new(),
        });
    };

    let records = record_send(host, log, &owed, &txid)?;
    Ok(Settled {
        result: SendResult::Sent,
        txid: Some(txid),
        detail: None,
        records,
    })
}

/// Records that the transaction `txid` paid `carried`: one signed payment
/// record per recipient, appended to `log` and returned.
///
/// What [`settle`] writes after a send that succeeded, and what a wallet
/// writes when a send it could not resolve at the time is later found on
/// chain. The two must be the same records: a payment recorded twice under
/// different ids is two payments to every reader.
///
/// Signed before they are kept. A verifying fold applies an entry written as
/// a bound participant only from a copy that verifies against their key
/// (§10.3), so an unsigned record of this payer's own payment would be set
/// aside on this device and the debt offered to them again.
pub fn record_send(
    host: &dyn BillHost,
    log: &mut BillLog<'_>,
    carried: &BTreeMap<String, i64>,
    txid: &str,
) -> Result<Vec<Value>> {
    let bill_id = log.bill_id()?;
    let mut records = Vec::new();
    for (to, amount) in carried {
        let payment_id = payment_id_for_send(txid, to);
        let unsigned = record_payment(
            host,
            &payment_id,
            to,
            *amount,
            "shieldedZec",
            Some(txid),
            None,
            None,
            None,
        )?;
        let record = sign_entry(host, &unsigned, &bill_id)?;
        log.add(vec![record.clone()])?;
        records.push(record);
    }
    Ok(records)
}
