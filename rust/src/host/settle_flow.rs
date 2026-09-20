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
use super::entries::record_payment;
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
    /// published, `payout_not_zec` when the preferred payout is a swap or
    /// cash. The two need different remedies (§8.5).
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
    );

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
    // be anywhere. Records to one recipient sum, as they do in §14.
    //
    // **What the request carries, not what the payer owes.** The two differ
    // whenever a recipient's preferred payout is not a Zcash address: §8.5
    // leaves them out of the URI and reports them, and recording them here
    // would claim a transaction settled a debt it never paid — a debt the
    // payee then has to contest rather than simply still be owed.
    let unpayable: BTreeSet<&str> = obligation
        .unpayable()
        .iter()
        .map(|u| u.id.as_str())
        .collect();
    let mut owed: BTreeMap<&str, i64> = BTreeMap::new();
    for settlement in &obligation.settlements {
        if unpayable.contains(settlement.to.as_str()) {
            continue;
        }
        *owed.entry(settlement.to.as_str()).or_insert(0) += settlement.amount;
    }
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
        // `Sent::sent` cannot produce this; a hand-built `Sent` can.
        return Ok(Settled {
            result: SendResult::Failed,
            txid: None,
            detail: Some("the wallet reported a send with no transaction id".to_owned()),
            records: Vec::new(),
        });
    };

    let mut records = Vec::new();
    for (to, amount) in owed {
        let record = record_payment(
            host,
            &txid,
            to,
            amount,
            "shieldedZec",
            None,
            None,
            None,
            None,
        )?;
        log.add(vec![record.clone()])?;
        records.push(record);
    }
    Ok(Settled {
        result: SendResult::Sent,
        txid: Some(txid),
        detail: None,
        records,
    })
}

/// Records a debt settled in cash (§9.2).
///
/// Nothing is sent and nothing is verified: cash moved outside this protocol,
/// and the only evidence it ever has is the recipient's confirmation (§10.5).
/// **A caller must not present this with the confidence of an on-chain
/// payment** — anyone on the bill can write one.
///
/// `payment_id` is the caller's, because there is no transaction to take one
/// from. It MUST be unique on the bill: two cash payments sharing an id are
/// one payment to every reader that folds the log.
pub fn settle_cash(
    host: &dyn BillHost,
    log: &mut BillLog,
    payment_id: &str,
    to: &str,
    amount: i64,
    note: Option<&str>,
) -> Result<Value> {
    let record = record_payment(host, payment_id, to, amount, "cash", None, None, None, note)?;
    log.add(vec![record.clone()])?;
    Ok(record)
}

/// Records a debt settled by a swap off this chain (§9.2).
///
/// **Verifiable only in half.** What left this wallet is ZEC and is recorded
/// in `zatoshi`; what the recipient was owed arrives as another asset on
/// another chain, which this bill cannot see. A caller MUST NOT present this
/// as confirmed on the strength of the ZEC leg alone — that the deposit was
/// sent is not that the recipient was paid, and only the recipient can say
/// the latter (§10.5).
///
/// `reference` identifies the swap: the provider's intent id, or the
/// transaction on the destination chain. **It is not a Zcash txid**, and a
/// reader that renders it as one is wrong for every swap. The chain it names
/// is the chain of the `swap` payout being settled.
///
/// Where a participant's payout may change after this is recorded, that chain
/// stops being derivable from the current payout; pass it in `note`.
#[allow(clippy::too_many_arguments)]
pub fn settle_swap(
    host: &dyn BillHost,
    log: &mut BillLog,
    reference: &str,
    to: &str,
    amount: i64,
    zatoshi: Option<i64>,
    paid_at_rate: Option<Value>,
    note: Option<&str>,
) -> Result<Value> {
    let record = record_payment(
        host,
        // The swap's own identifier is what a reader checks this record
        // against, so it is the payment id as well as the reference. A second
        // identifier here would be one nothing outside this bill has ever
        // heard of.
        reference,
        to,
        amount,
        "swap",
        Some(reference),
        zatoshi,
        paid_at_rate,
        note,
    )?;
    log.add(vec![record.clone()])?;
    Ok(record)
}
