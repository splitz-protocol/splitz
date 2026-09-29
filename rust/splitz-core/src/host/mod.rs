//! The wallet seam: the entries a log is made of, and what a wallet lends.
//!
//! The rest of this crate decides what a bill is, what anyone owes and what
//! payment request settles it. This module supplies the two things §13 leaves
//! to the wallet that embeds it: something that assembles an entry and derives
//! §9.5's id, and the seam through which a wallet lends its keys, its clock
//! and its ability to send.
//!
//! Nothing here holds a key, opens a socket or reads a clock of its own, so
//! the whole feature runs in a test with no wallet behind it.
//!
//! It is a module rather than a second crate so that one dependency reaches
//! both, and the two can never be at different versions of the rules — bill
//! ids and entry ids are digests, and two devices running different rules do
//! not agree on what the bill is.

pub mod arrivals;
pub mod bill_log;
pub mod entries;
#[allow(clippy::module_inception)]
pub mod host;
pub mod lanes;
pub mod settle_flow;
pub mod sharing;
pub mod totals;

pub use arrivals::{arrivals_for, Arrival, Arrivals, IncomingTransaction};
pub use bill_log::{BillLog, FoldedBill};
pub use entries::{
    add_expense, amend_entry, base64url_no_pad, confirm_payment, create_bill, join_bill,
    record_payment, set_rate, sign_entry, void_entry, CREATOR_KEY_BYTES, ENTRY_VERSION,
    NONCE_BYTES,
};
pub use host::{BillHost, SendResult, Sent, SignEntry, VerifyEntry};
pub use lanes::{lane_for, SettleLane};
pub use settle_flow::{
    check_proposal, obligation_for, payment_id_for_send, record_send, settle, PayerObligation,
    ProposalCheck, ProposedOutput, Settled,
};
pub use sharing::{
    accept_scan, delta_for, invite_for, read_scan, shareable_bill, Scanned, ScannedBill,
};
pub use totals::{totals_across, Standing, Totals};
