//! The log a device holds for one bill, and the bill it folds to.
//!
//! §10.2 merges by set union keyed by entry id, so a device holds entries and
//! derives everything else. Nothing here caches a bill across a change: the
//! bill is a function of the entries, and a cached one is a second source of
//! truth that goes stale without saying so.

use serde_json::Value;
use std::collections::BTreeMap;

use crate::authority::Identities;
use crate::error::Result;
use crate::log::{
    fold_log, fold_log_verified, merge_logs, order_entries, ReplacedAddress, SetAside,
};
use crate::model::Bill;
use crate::serialization::decode_bill;

use super::host::BillHost;

/// Refusals an entry outgrows: each names a participant, entry or payment
/// this device may not hold yet, and the entry applies once a sync brings it
/// (§10.3). See [`BillLog::refusal_of`].
pub const CODES_AN_ENTRY_OUTGROWS: [&str; 3] = [
    crate::error::code::UNKNOWN_PARTICIPANT,
    crate::error::code::UNKNOWN_ENTRY,
    crate::error::code::UNKNOWN_PAYMENT,
];

/// What a fold produced, and what it refused.
///
/// The refusals travel with the bill because an entry that vanished silently
/// is indistinguishable from one that was never sent. §10.3 makes the report
/// per occurrence and not part of the convergent state, so two devices may
/// list different refusals for one history and still hold the same bill —
/// which is why these are not compared across devices anywhere.
#[derive(Debug, Clone, PartialEq)]
pub struct FoldedBill {
    pub bill: Bill,
    /// Who opened the bill: the author of the one `createBill` the fold kept —
    /// for this bill's id and, when the host verifies, signed by the key that
    /// entry states (§10.1). §10.8 and §10.4 give this participant alone some
    /// powers, so a reader decides them from this and never from whichever
    /// create a log happens to list first.
    pub creator_id: String,
    pub set_aside: Vec<SetAside>,
    pub withdrawn: Vec<String>,
    /// Every pay-to address that changed, which §13 says a wallet MUST show
    /// before settling.
    pub replaced_addresses: Vec<ReplacedAddress>,
    /// Which keys §10.7 binds. Empty when the host does not verify: without a
    /// verifier no self-claim is checked, so nothing is bound.
    pub identities: Identities,
    /// Who wrote each payment record on the bill, by the payment's id (§14.4).
    pub payment_authors: BTreeMap<String, String>,
    /// What each payment record says, by the payment's id: the digest a
    /// confirmation of it carries as `record` (§10.5).
    pub payment_digests: BTreeMap<String, String>,
    /// The entry that introduced each expense, by the expense's own id: what
    /// an amendment or a withdrawal of it targets. The fold's answer, not the
    /// log's, which also holds entries the fold set aside.
    pub expense_entries: BTreeMap<String, String>,
    /// Who wrote each expense, by the expense's own id.
    pub expense_authors: BTreeMap<String, String>,
    /// The entry that recorded each payment, by the payment's id.
    pub payment_entries: BTreeMap<String, String>,
    /// The `setRate` entry whose rate the bill carries.
    pub rate_entry: Option<String>,
    /// Who wrote that `setRate`: the name §14.2 puts beside the rate.
    pub rate_author: Option<String>,
    /// The entries in force, in §10.2's order: what §10.8's still-named check
    /// reads. An entry refused at ingress, withdrawn, replaced by a
    /// restatement or a restatement that does not apply is not among them.
    pub in_force: Vec<String>,
    /// The amendment §10.4 would apply to each entry, by the entry's id.
    pub amendment_of: BTreeMap<String, String>,
}

/// One bill's entries, and the answers derived from them.
pub struct BillLog<'h> {
    host: &'h dyn BillHost,
    entries: Vec<Value>,
    bill_id: Option<String>,
}

impl<'h> BillLog<'h> {
    pub fn new(host: &'h dyn BillHost) -> Self {
        Self {
            host,
            entries: Vec::new(),
            bill_id: None,
        }
    }

    pub fn with_entries(host: &'h dyn BillHost, entries: Vec<Value>) -> Self {
        Self {
            host,
            entries,
            bill_id: None,
        }
    }

    /// Names the bill these entries belong to.
    ///
    /// A device that holds a bill always knows it, and a fold that is not
    /// told reads whatever single create the log holds — so anyone holding
    /// the invite who pushes a valid create for another bill into the channel
    /// makes this one unopenable (§10.3's `ambiguous_create`). It is omitted
    /// only by a caller about to learn the id from the log, such as one
    /// opening a bill it just created.
    pub fn for_bill(mut self, bill_id: impl Into<String>) -> Self {
        self.bill_id = Some(bill_id.into());
        self
    }

    /// The bill these entries belong to: the one named with [`Self::for_bill`],
    /// or the one their create entry opens. What an entry written for this
    /// log is signed on (§10.6).
    pub fn bill_id(&self) -> Result<String> {
        match &self.bill_id {
            Some(id) => Ok(id.clone()),
            None => Ok(self.fold()?.bill.id),
        }
    }

    /// The entries this device holds, in the order §10.2 puts them.
    pub fn entries(&self) -> Vec<Value> {
        let mut ordered = self.entries.clone();
        order_entries(&mut ordered);
        ordered
    }

    /// Adds entries this device wrote, or a peer's.
    ///
    /// Returns what the merge refused at ingress (§10.1). A caller that
    /// ignores it has dropped somebody's entry without telling them.
    pub fn add(&mut self, incoming: Vec<Value>) -> Result<Vec<SetAside>> {
        let merged = merge_logs(&[std::mem::take(&mut self.entries), incoming])?;
        self.entries = merged.merged;
        Ok(merged.refused)
    }

    /// Folds to a bill (§10.3).
    ///
    /// The §12 code the fold would set `entry` aside with were it appended to
    /// this log, or `None` when it would apply (§10.8, "Asking before
    /// writing").
    ///
    /// Answered by folding the log with `entry` in it, so it is the fold's own
    /// rule and cannot drift from it. Pass the entry as it would be written —
    /// signed, where this host verifies. A refusal in
    /// [`CODES_AN_ENTRY_OUTGROWS`] waits on an entry this device may not hold
    /// yet and applies once a sync brings it; any other is written, synced and
    /// refused on every device for good, so a host writes nothing on one.
    pub fn refusal_of(&self, entry: &Value) -> Result<Option<String>> {
        if let Err(e) = crate::log::check_entry(entry) {
            return Ok(Some(e.code.to_owned()));
        }
        let mut entries = self.entries.clone();
        entries.push(entry.clone());
        let mut trial = BillLog::with_entries(self.host, entries);
        trial.bill_id = self.bill_id.clone();
        let id = entry.get("id").and_then(Value::as_str);
        // A log the entry leaves opening no single bill — a second create on a
        // log naming none — is refused whole.
        let folded = match trial.fold() {
            Ok(folded) => folded,
            Err(e) => return Ok(Some(e.code.to_owned())),
        };
        Ok(folded
            .set_aside
            .into_iter()
            .find(|s| Some(s.id.as_str()) == id)
            .map(|s| s.code.to_owned()))
    }

    /// Fails only when the log opens no bill at all — no entries, or none that
    /// creates one. An entry that cannot be applied is set aside and reported,
    /// never raised, so one bad entry does not take the bill with it.
    pub fn fold(&self) -> Result<FoldedBill> {
        let result = match self.host.verifier() {
            None => fold_log(&self.entries, self.bill_id.as_deref())?,
            Some(verify) => {
                fold_log_verified(&self.entries, self.bill_id.as_deref(), Some(verify))?
            }
        };
        Ok(FoldedBill {
            bill: decode_bill(&result.bill)?,
            creator_id: result.creator.clone(),
            set_aside: result.set_aside,
            withdrawn: result.withdrawn,
            replaced_addresses: result.replaced_addresses,
            identities: result.identities,
            payment_authors: result.payment_authors,
            payment_digests: result.payment_digests,
            expense_entries: result.expense_entries,
            expense_authors: result.expense_authors,
            payment_entries: result.payment_entries,
            rate_entry: result.rate_entry,
            rate_author: result.rate_author,
            in_force: result.in_force,
            amendment_of: result.amendment_of,
        })
    }

    /// True when this log opens a bill. A log a peer has only half-delivered
    /// does not, and that is a state to show rather than a failure to report.
    pub fn opens_a_bill(&self) -> bool {
        !self.entries.is_empty() && self.fold().is_ok()
    }
}
