//! §14.9: a bill is settled only once its creator has closed it.
//!
//! The close is an entry (§10.9), so every device reads one answer from the
//! log. These are the host's refusals and builders around it; the fold itself
//! stays permissive, because a payment someone already made is a fact whatever
//! state the bill was in.

use serde_json::Value;

use super::bill_log::FoldedBill;
use super::entries::{close_bill, void_entry};
use super::host::BillHost;
use crate::error::{code, Result, SplitError};

/// Why no payment may start on `folded` now, or none when one may: the bill
/// is open, and its creator has not closed it for settling (§14.9).
///
/// Every way of paying asks this — the request, a swap, a cash record — so a
/// debt is never paid while the expenses that make it can still change.
pub fn settle_refusal(folded: &FoldedBill) -> Option<&'static str> {
    (!folded.closed()).then_some(code::BILL_NOT_CLOSED)
}

/// Why no expense may be added or corrected on `folded` now, or none when one
/// may: the bill is closed, and its debts are being paid (§14.9).
///
/// The fold would accept one and reopen the bill (§10.9); a host refuses to
/// write it so that a person changes what is owed only by asking the creator
/// to reopen.
pub fn expense_refusal(folded: &FoldedBill) -> Option<&'static str> {
    folded.closed().then_some(code::BILL_CLOSED)
}

/// The entry that closes `folded` for settling (§10.9).
///
/// Refused unless `host` is the bill's creator: a close by anybody else is set
/// aside by every fold, and writing one would tell its author the bill was
/// closed when it is not.
pub fn close_for(host: &dyn BillHost, folded: &FoldedBill) -> Result<Value> {
    if host.me() != folded.creator_id {
        return Err(SplitError::new(
            code::UNAUTHORIZED_ENTRY,
            "Only the creator closes a bill",
        ));
    }
    close_bill(host, &folded.closed_over, folded.last_close_at.as_deref())
}

/// The entry that reopens `folded`, withdrawing the close it is closed by
/// (§10.8), or none when it is open.
pub fn reopen_for(host: &dyn BillHost, folded: &FoldedBill) -> Result<Option<Value>> {
    let Some(close) = folded.close_entry.as_deref() else {
        return Ok(None);
    };
    if host.me() != folded.creator_id {
        return Err(SplitError::new(
            code::UNAUTHORIZED_ENTRY,
            "Only the creator reopens a bill",
        ));
    }
    void_entry(host, close, folded.last_close_at.as_deref()).map(Some)
}
