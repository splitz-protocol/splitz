//! Paying what this device owes, and recording that it did.
//!
//! The protocol renders the payment request; the wallet sends it. What sits
//! between them is the part that has to survive a person walking away
//! mid-send, and that is what this file is for.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

use crate::error::Result;
use crate::obligation::{
    choose_payouts, render_reading, withholdings, Awaiting, Obligation, Unpayable,
};
use crate::rate::ExchangeRate;
use crate::serialization::rate_to_json;
use crate::settle::{settle_bill, Settlement, DEFAULT_EXACT_LIMIT};
use crate::zip321::{read_request, Zip321Payment};

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

    /// The protocol's answer: the request, what it carries, and who it could
    /// not carry with the reason for each.
    pub request: Obligation,

    /// The rate `request` was priced at: the bill's, when this was read.
    pub rate: ExchangeRate,
}

impl PayerObligation {
    pub fn uri(&self) -> Option<&str> {
        self.request.uri.as_deref()
    }

    /// Who the request could not carry, and why: `no_address` when nothing is
    /// published, `bad_address` when what is published is not an address
    /// §8.3 admits, `payout_not_zec` when the preferred payout is a swap or
    /// cash, `unpriceable` when the debt is past what one request prices.
    /// Each needs a different remedy (§8.5).
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

    /// What the request sends each recipient, in zatoshi: the ZEC side of
    /// [`Self::carried_to`], summed per recipient as the request carries it.
    pub fn carried_zatoshi(&self) -> BTreeMap<String, i64> {
        let mut sent: BTreeMap<String, i64> = BTreeMap::new();
        for (payment, to) in self.request.payments.iter().zip(&self.request.recipients) {
            *sent.entry(to.clone()).or_insert(0) += payment.zatoshi;
        }
        sent
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
    /// The transaction id, present when `result` is [`SendResult::Sent`], and
    /// when it is [`SendResult::Pending`] and the wallet named the transaction
    /// it built. A pending one records nothing.
    pub txid: Option<String>,
    /// What to put in front of a person when nothing was recorded.
    pub detail: Option<String>,
    /// The §12 code, when a protocol rule refused the send; `detail` is then
    /// that code's plain-language sentence (`describe_code`).
    pub code: Option<String>,
    /// The payment entries, one per recipient. Already appended to the log —
    /// returned so a caller can hand them to a peer, not so a caller can
    /// decide whether to keep them.
    pub records: Vec<Value>,
}

/// Reads what this device owes on `folded`.
///
/// Returns `None` when the bill carries no rate: an unpriced bill is an
/// ordinary bill, not a refusal, and there is no §12 code to catch.
pub fn obligation_for(host: &dyn BillHost, folded: &FoldedBill) -> Result<Option<PayerObligation>> {
    obligation_via(host, folded, &BTreeMap::new())
}

/// Whether `reviewed` is still the request `folded` asks of this payer, with
/// the same payout choices `via` (§14.2).
///
/// Read immediately before the wallet is called: an entry merged after the
/// payer reviewed the request — an expense, a new rate, a changed address —
/// changes what is owed or where it goes, and the request they saw is no
/// longer the one the bill asks for. A host sends only while this holds.
pub fn request_stands(
    host: &dyn BillHost,
    folded: &FoldedBill,
    reviewed: &PayerObligation,
    via: &BTreeMap<String, i64>,
) -> Result<bool> {
    let now = obligation_via(host, folded, via)?;
    Ok(now.as_ref().and_then(PayerObligation::uri) == reviewed.uri())
}

/// `obligation_for`, with the payer's choice of payout for the recipients
/// `via` names (§14.8).
///
/// `via` maps a participant id to the index of one of their declared payouts,
/// as `choose_payouts` takes it. Who owes what is read from the bill as
/// folded — a preference takes no part in it (§9.1) — and only the request is
/// rendered from the chosen payouts. Refuses as `choose_payouts` does.
pub fn obligation_via(
    host: &dyn BillHost,
    folded: &FoldedBill,
    via: &BTreeMap<String, i64>,
) -> Result<Option<PayerObligation>> {
    let Some(rate) = folded.bill.rate.as_ref() else {
        return Ok(None);
    };

    // §14 decides this, not this layer: which debts a request may carry, and
    // which wait on a confirmation, is a function of the bill and the records
    // the fold applied. A second implementation here is a second place for
    // the rule to drift.
    let plan = settle_bill(&folded.bill, DEFAULT_EXACT_LIMIT)?;
    let split = withholdings(
        &plan.settlements,
        &folded.bill,
        host.me(),
        Some(&folded.payment_authors),
    )?;

    // `render_obligation` is the protocol's own answer to the hazard in §8.5:
    // either refuse the whole request, or render what can be carried and
    // report the rest. Writing the loop by hand is how a wallet ends up doing
    // neither.
    // Checked before anything is rendered, so a choice that names nobody is
    // refused whether or not this device owes anything.
    let chosen = choose_payouts(&folded.bill, via)?;
    let request = render_reading(&split.carried, &chosen, rate, true, false, &|a| {
        host.reads_address(a)
    })?;

    Ok(Some(PayerObligation {
        settlements: split.carried,
        awaiting: split.awaiting,
        request,
        rate: rate.clone(),
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
///
/// `from` is the payer who writes the record, and the id is theirs under
/// [`authored_id`](super::entries::authored_id).
pub fn payment_id_for_send(from: &str, txid: &str, to: &str) -> String {
    super::entries::authored_id(from, &format!("{txid}:{to}"))
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
    // §14.9: nothing is paid on a bill its creator has not closed.
    if let Some(refused) = super::closing::settle_refusal(&log.fold()?) {
        return Ok(Settled {
            result: SendResult::Failed,
            txid: None,
            detail: Some(
                crate::error::describe_code(refused)
                    .unwrap_or(refused)
                    .to_owned(),
            ),
            code: Some(refused.to_owned()),
            records: Vec::new(),
        });
    }
    let Some(uri) = obligation.uri() else {
        return Ok(Settled {
            result: SendResult::Failed,
            txid: None,
            detail: Some("there is nothing to send".to_owned()),
            records: Vec::new(),
            code: None,
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
            code: None,
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
            txid: sent.txid,
            detail: sent.detail,
            records: Vec::new(),
            code: None,
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
            code: None,
        });
    };

    let records = record_send(
        host,
        log,
        &owed,
        &txid,
        &obligation.carried_zatoshi(),
        Some(&obligation.rate),
    )?;
    Ok(Settled {
        result: SendResult::Sent,
        txid: Some(txid),
        detail: None,
        records,
        code: None,
    })
}

/// Records that the transaction `txid` paid `carried`: one signed payment
/// record per recipient, appended to `log` and returned.
///
/// Each record states what it sent in ZEC, from `zatoshi`, and the rate it
/// was priced at, from `rate` (§9.2): the payee confirms against a figure
/// they can compare with what arrived, not a fiat amount alone, so a rate a
/// payer lowered before paying shows on the record they confirm.
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
    zatoshi: &BTreeMap<String, i64>,
    rate: Option<&ExchangeRate>,
) -> Result<Vec<Value>> {
    let bill_id = log.bill_id()?;
    let mut records = Vec::new();
    for (to, amount) in carried {
        let payment_id = payment_id_for_send(host.me(), txid, to);
        let unsigned = record_payment(
            host,
            &payment_id,
            to,
            *amount,
            "shieldedZec",
            Some(txid),
            zatoshi.get(to).copied(),
            rate.map(rate_to_json),
            None,
        )?;
        let record = sign_entry(host, &unsigned, &bill_id)?;
        log.add(vec![record.clone()])?;
        records.push(record);
    }
    Ok(records)
}

/// One payment a wallet is about to make: what its own ZIP 321 reader made of
/// a request, before anything is signed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposedOutput {
    pub address: String,
    pub zatoshi: i64,
}

/// How the payments a wallet is about to sign differ from the request (§14.6).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProposalCheck {
    /// Payments the request carries that the proposal does not, in request
    /// order.
    pub missing: Vec<Zip321Payment>,
    /// Payments the proposal makes that the request does not carry, in the
    /// order they were given.
    pub unexpected: Vec<ProposedOutput>,
}

impl ProposalCheck {
    /// True when the proposal pays exactly what the request asks.
    pub fn matches(&self) -> bool {
        self.missing.is_empty() && self.unexpected.is_empty()
    }
}

/// Compares what a wallet is about to sign with the request `uri` (§14.6).
///
/// `outputs` are the payments the wallet's own reader produced from `uri`,
/// without change: a reader that keeps only the first of several payments
/// pays one recipient while the payer was shown them all. Each requested
/// payment is matched to one proposed output with the same address and the
/// same zatoshi; order is not significant, and one output cannot answer for
/// two payments.
///
/// `uri` must be a request this protocol wrote: it is read with
/// [`read_request`], which refuses anything else with `zip321_not_canonical`.
pub fn check_proposal(uri: &str, outputs: &[ProposedOutput]) -> Result<ProposalCheck> {
    let requested = read_request(uri)?;
    let mut pool: Vec<Option<&ProposedOutput>> = outputs.iter().map(Some).collect();
    let mut missing = Vec::new();
    for payment in requested {
        let found = pool.iter().position(|o| {
            o.is_some_and(|o| o.address == payment.address && o.zatoshi == payment.zatoshi)
        });
        match found {
            Some(at) => pool[at] = None,
            None => missing.push(payment),
        }
    }
    Ok(ProposalCheck {
        missing,
        unexpected: pool.into_iter().flatten().cloned().collect(),
    })
}
