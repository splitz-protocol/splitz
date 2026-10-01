//! Moving a bill between devices through a relay that holds only ciphertext.

use std::collections::BTreeSet;

use serde_json::Value;
use splitz_core::SetAside;

use crate::error::{HostError, Result, SyncFailure};
use crate::keys::SplitsKeys;
use crate::relay::channel_for_bill;
use crate::sealing::Sealing;
use crate::store::BillStore;
use crate::wallet::SplitsRelay;

/// What one sync produced.
#[derive(Debug, Clone)]
pub struct SyncResult {
    /// The merged log, in the order §10.2 puts it.
    pub entries: Vec<Value>,
    /// What the merge refused at ingress (§10.1).
    pub refused: Vec<SetAside>,
    /// Blobs in the channel that would not open under this bill's key.
    ///
    /// Counted rather than ignored. A channel where every blob is unopenable
    /// is a key that is wrong, and that looks identical to a quiet relay
    /// unless somebody is counting.
    pub unopenable: usize,
}

/// Syncs one bill through a relay.
///
/// Everything promised about the relay lives here: entries are sealed under
/// the bill key before they leave, the channel is the bill id's hash so the
/// relay cannot name the bill, and merging is the same order-independent set
/// union the local store already performs — so a device that has been offline
/// merges what it missed rather than reconciling two versions of a summary.
pub struct SplitsSync<'a> {
    store: &'a BillStore<'a>,
    keys: &'a SplitsKeys<'a>,
    relay: &'a dyn SplitsRelay,
}

impl<'a> SplitsSync<'a> {
    pub fn new(
        store: &'a BillStore<'a>,
        keys: &'a SplitsKeys<'a>,
        relay: &'a dyn SplitsRelay,
    ) -> Self {
        Self { store, keys, relay }
    }

    /// Pulls what this device does not hold, then pushes what the channel
    /// does not.
    ///
    /// Pull first, so a push the relay refuses — a store that is full, a log
    /// that has outgrown what one request carries — never stops this device
    /// seeing what the others wrote. The push then sends only blobs the fetch
    /// did not return: sealing is deterministic, so a blob the channel holds
    /// is an entry it holds. Both directions merge by content, so running this
    /// twice, or on two devices at once, converges.
    pub fn sync(&self, bill_id: &str) -> Result<SyncResult> {
        let (result, fetched) = self.pull_blobs(bill_id)?;
        self.push_except(bill_id, &fetched)?;
        Ok(result)
    }

    /// Seals every entry this device holds, as it holds it, and pushes it to
    /// the bill's channel.
    ///
    /// **Nothing is signed here.** An entry is signed by the device that
    /// writes it, when it writes it. The store also holds what peers pushed,
    /// and an unsigned entry a peer wrote in this device's name would
    /// otherwise be signed with this device's key on the next push — a forged
    /// confirmation becoming a genuine one. A blob is keyed by its content, so
    /// pushing the whole log every time is safe: the relay stores each entry
    /// once however often it is sent.
    pub fn push(&self, bill_id: &str) -> Result<Vec<Value>> {
        self.push_except(bill_id, &BTreeSet::new())
    }

    /// [`Self::push`], leaving out the blobs in `held` — what the channel is
    /// known to hold.
    fn push_except(&self, bill_id: &str, held: &BTreeSet<String>) -> Result<Vec<Value>> {
        let entries = self.store.read(bill_id)?;
        if entries.is_empty() {
            return Ok(entries);
        }
        let key = self.require_key(bill_id)?;
        refuse_foreign_key(bill_id, &key, &entries)?;
        let blobs = entries
            .iter()
            .map(|entry| Sealing.seal(entry, &key))
            .collect::<std::result::Result<Vec<_>, _>>()?;
        let fresh: Vec<String> = blobs.into_iter().filter(|b| !held.contains(b)).collect();
        if !fresh.is_empty() {
            self.relay.push(&channel_for_bill(bill_id), &fresh)?;
        }
        Ok(entries)
    }

    /// Fetches the channel, opens what it can, and merges it into what is
    /// held.
    ///
    /// Everything that opens is merged. Authorship is **not** judged here: an
    /// entry admitted or refused by what this device happened to hold when it
    /// arrived would make the stored log depend on network order, and two
    /// devices that pulled the same entries in a different order would then
    /// hold different bills. §10.7 decides authorship over the whole log at
    /// fold time, where the answer is the same on every device and a locally
    /// written entry faces exactly the rules a synced one does.
    pub fn pull(&self, bill_id: &str) -> Result<SyncResult> {
        Ok(self.pull_blobs(bill_id)?.0)
    }

    /// [`Self::pull`], and the blobs the channel returned.
    fn pull_blobs(&self, bill_id: &str) -> Result<(SyncResult, BTreeSet<String>)> {
        let key = self.require_key(bill_id)?;
        let blobs = self.relay.fetch(&channel_for_bill(bill_id))?;

        let mut unopenable = 0;
        let mut entries = Vec::new();
        for blob in &blobs {
            match Sealing.open(blob, &key) {
                Ok(entry) => entries.push(entry),
                // A foreign or altered blob is skipped rather than failing the
                // whole sync, so one bad blob cannot strand a bill.
                Err(_) => unopenable += 1,
            }
        }

        refuse_foreign_key(bill_id, &key, &entries)?;

        // Merged only while this device still holds the bill's key. A bill
        // forgotten while the fetch was in flight is not written back: it
        // would return with no key, and the next Share would mint a key
        // nobody else holds.
        let still_held = self
            .keys
            .read_bill_key(bill_id)?
            .is_some_and(|k| !k.is_empty());
        if !still_held {
            return Err(HostError::Sync {
                kind: SyncFailure::Forgotten,
                message: format!("{bill_id} was forgotten while it synced"),
            });
        }
        let merged = self.store.merge(bill_id, entries)?;
        Ok((
            SyncResult {
                entries: merged.entries,
                refused: merged.refused,
                unopenable,
            },
            blobs.into_iter().collect(),
        ))
    }

    fn require_key(&self, bill_id: &str) -> Result<String> {
        // A keychain refuses while the session is locked. A poll that lands
        // then is a sync that could not run, and is reported as one.
        let key = self
            .keys
            .read_bill_key(bill_id)
            .map_err(|e| HostError::Sync {
                kind: SyncFailure::KeyLocked,
                message: format!("Cannot read the key for {bill_id}: {e}"),
            })?;
        match key {
            Some(key) if !key.is_empty() => Ok(key),
            _ => Err(HostError::Sync {
                kind: SyncFailure::NoKey,
                message: format!("No key for {bill_id}; it cannot be synced"),
            }),
        }
    }
}

/// §9.4. Refuses `entries` when they hold `bill_id`'s create and it commits to
/// a key other than `key`.
///
/// A key handed over with a real bill's id opens whatever its maker sealed
/// under it: a copy of the bill with an entry only this device sees, and
/// everything this device writes then reaches nobody else. The bill's own
/// create names the key it was made with, so a log that disagrees is not
/// merged and a held log is not sealed under a key that is not its own.
fn refuse_foreign_key(bill_id: &str, key: &str, entries: &[Value]) -> Result<()> {
    let foreign = entries
        .iter()
        .any(|e| splitz_core::create_refuses_key(e, bill_id, key));
    if foreign {
        return Err(HostError::ForeignKey(bill_id.to_owned()));
    }
    Ok(())
}
