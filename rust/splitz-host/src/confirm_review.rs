//! What a payee is warned about before confirming a payment, beyond what
//! §14.2 requires be shown (§14.7).

use splitz_core::host::FoldedBill;
use splitz_core::PaymentRecord;

use crate::pricing::rate_far_from_live;

/// Why a payment wants the payee's own look rather than a one-tap confirm.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PaymentConcern {
    /// The payer set the bill's rate: the figure the record's ZEC is checked
    /// against is the payer's own.
    RateSetByPayer,
    /// The record was priced at a rate other than the bill's.
    PricedAtAnotherRate,
    /// The rate the record is priced at is
    /// [`RATE_WARNING_PERCENT`](crate::pricing::RATE_WARNING_PERCENT) or more
    /// from a live price.
    RateFarFromLive,
}

/// The concerns `payment` on `folded` raises, in this enum's order; empty
/// when none does. `live` is a live price of one ZEC in the payment's
/// currency, or `None` when none could be read — not by itself a concern.
///
/// A host MAY confirm several `arrived` payments on one acceptance (§14.7),
/// and SHOULD leave out of it every one with a concern: each is checked
/// against a figure somebody with a stake in it chose.
pub fn concerns_before_confirming(
    payment: &PaymentRecord,
    folded: &FoldedBill,
    live: Option<i64>,
) -> Vec<PaymentConcern> {
    let rate = folded.bill.rate.as_ref();
    let priced_at = payment.paid_at_rate.as_ref().or(rate);
    let mut out = Vec::new();
    if folded.rate_author.is_some() && folded.rate_author.as_deref() == Some(payment.from.as_str())
    {
        out.push(PaymentConcern::RateSetByPayer);
    }
    if let Some(paid_at) = &payment.paid_at_rate {
        let same = rate.is_some_and(|r| {
            r.currency == paid_at.currency && r.minor_units_per_zec == paid_at.minor_units_per_zec
        });
        if !same {
            out.push(PaymentConcern::PricedAtAnotherRate);
        }
    }
    if let (Some(priced_at), Some(live)) = (priced_at, live) {
        if priced_at.currency == payment.currency
            && rate_far_from_live(priced_at.minor_units_per_zec, live)
        {
            out.push(PaymentConcern::RateFarFromLive);
        }
    }
    out
}
