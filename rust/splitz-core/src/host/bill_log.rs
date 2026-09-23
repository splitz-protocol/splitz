//! The log a device holds for one bill, and the bill it folds to.
//!
//! §10.2 merges by set union keyed by entry id, so a device holds entries and
//! derives everything else. Nothing here caches a bill across a change: the
//! bill is a function of the entries, and a cached one is a second source of
//! truth that goes stale without saying so.

use serde_json::Value;

use crate::authority::Identities;
use crate::error::Result;
use crate::log::{
    fold_log, fold_log_verified, merge_logs, order_entries, ReplacedAddress, SetAside,
};
use crate::model::Bill;
use crate::serialization::decode_bill;

use super::host::BillHost;

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
    pub set_aside: Vec<SetAside>,
    pub withdrawn: Vec<String>,
    /// Every pay-to address that changed, which §13 says a wallet MUST show
    /// before settling.
    pub replaced_addresses: Vec<ReplacedAddress>,
    /// Which keys §10.7 binds, and which ids two keys each claim.
    ///
    /// A contested id is not an error and its entries still apply — refusing
    /// them would let anyone make a bill unopenable by minting a rival claim.
    /// What a contest costs is the ability to be paid: §10.7 says a wallet
    /// MUST NOT settle to a contested participant's address without putting it
    /// in front of the payer first.
    ///
    /// Empty of contests when the host does not verify: without a verifier no
    /// self-claim is checked, so nothing is bound and nothing is contested.
    pub identities: Identities,
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
            set_aside: result.set_aside,
            withdrawn: result.withdrawn,
            replaced_addresses: result.replaced_addresses,
            identities: result.identities,
        })
    }

    /// True when this log opens a bill. A log a peer has only half-delivered
    /// does not, and that is a state to show rather than a failure to report.
    pub fn opens_a_bill(&self) -> bool {
        !self.entries.is_empty() && self.fold().is_ok()
    }
}
