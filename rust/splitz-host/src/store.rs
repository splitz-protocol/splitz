//! Where a device keeps the bills it holds.

use serde_json::Value;
use splitz_core::{merge_logs, order_entries, SetAside};

use crate::error::{HostError, Result};
use crate::wallet::BillStorage;

const PREFIX: &str = "splitz_bill_";

/// The bills this device holds, and the entries each is made of.
pub struct BillStore<'a> {
    storage: &'a dyn BillStorage,
}

impl<'a> BillStore<'a> {
    pub fn new(storage: &'a dyn BillStorage) -> Self {
        Self { storage }
    }

    /// The storage underneath, for a device-local list that is not a bill.
    ///
    /// Exposed rather than wrapped because what else a device keeps beside its
    /// bills is not this type's business — it owns the `splitz_bill_`
    /// namespace and says so, and another list picks its own.
    pub fn storage(&self) -> &'a dyn BillStorage {
        self.storage
    }

    fn name(bill_id: &str) -> String {
        format!("{PREFIX}{bill_id}")
    }

    /// Every bill id this device holds entries for.
    pub fn bill_ids(&self) -> Result<Vec<String>> {
        Ok(self
            .storage
            .keys(PREFIX)?
            .into_iter()
            .map(|key| key[PREFIX.len()..].to_owned())
            .collect())
    }

    /// The entries held for `bill_id`, in the order §10.2 puts them, or none.
    ///
    /// Stored text that will not decode is returned as no entries rather than
    /// refused: a bill this device cannot read is a state to show, and failing
    /// here would take down whatever listed the bills.
    pub fn read(&self, bill_id: &str) -> Result<Vec<Value>> {
        let Some(stored) = self.storage.read(&Self::name(bill_id))? else {
            return Ok(Vec::new());
        };
        if stored.is_empty() {
            return Ok(Vec::new());
        }
        let Ok(Value::Array(decoded)) = serde_json::from_str::<Value>(&stored) else {
            return Ok(Vec::new());
        };
        let mut entries: Vec<Value> = decoded.into_iter().filter(Value::is_object).collect();
        order_entries(&mut entries);
        Ok(entries)
    }

    /// Merges `incoming` into what is held, and returns the merged log.
    ///
    /// The merge is the protocol's, so it is the same set union a peer
    /// performs: idempotent, commutative, and deciding a collision by content
    /// rather than by which copy arrived first. Whatever it refuses at ingress
    /// is returned with the log, because an entry that vanished silently is
    /// indistinguishable from one that was never sent.
    pub fn merge(&self, bill_id: &str, incoming: Vec<Value>) -> Result<MergedBill> {
        let held = self.read(bill_id)?;
        let merged = merge_logs(&[held, incoming])
            .map_err(|e| HostError::Storage(format!("The merge refused the log: {e}")))?;
        let text = serde_json::to_string(&merged.merged)
            .map_err(|e| HostError::Storage(format!("The merged log is not writable: {e}")))?;
        self.storage.write(&Self::name(bill_id), &text)?;
        Ok(MergedBill {
            entries: merged.merged,
            refused: merged.refused,
        })
    }

    /// Forgets a bill entirely.
    pub fn forget(&self, bill_id: &str) -> Result<()> {
        self.storage.delete(&Self::name(bill_id))
    }

    /// Clears anything an interrupted write left behind. See
    /// [`BillStorage::sweep_unfinished_writes`].
    pub fn sweep_unfinished_writes(&self) -> Result<usize> {
        self.storage.sweep_unfinished_writes()
    }
}

/// A merged log, and what the merge would not take.
#[derive(Debug, Clone)]
pub struct MergedBill {
    pub entries: Vec<Value>,
    /// Refused at ingress under §10.1. A caller that ignores this has dropped
    /// somebody's entry without telling them.
    pub refused: Vec<SetAside>,
}
