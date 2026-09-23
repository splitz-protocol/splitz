//! Moving a bill between devices through a relay that holds only ciphertext.

use serde_json::Value;
use splitz_core::host::{sign_entry, BillHost, Sent, SignEntry};
use splitz_core::SetAside;

use crate::error::{HostError, Result};
use crate::keys::SplitsKeys;
use crate::relay::channel_for_bill;
use crate::sealing::Sealing;
use crate::signing::{Signer, SEED_BYTES};
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

    /// Pushes what this device holds, then pulls what it does not.
    ///
    /// Push first, so a participant syncing right after us sees our entries;
    /// then pull, so we see theirs. Both directions merge by entry id, so
    /// running this twice, or on two devices at once, converges.
    pub fn sync(
        &self,
        bill_id: &str,
        signer_seed: Option<&[u8]>,
        author_id: Option<&str>,
    ) -> Result<SyncResult> {
        self.push(bill_id, signer_seed, author_id)?;
        self.pull(bill_id)
    }

    /// Seals every entry this device holds and pushes it to the bill's
    /// channel.
    ///
    /// Entries this device authored are signed first, so participants who
    /// receive them can confirm they came from this identity. A signature is
    /// deterministic and a blob is keyed by its content, so pushing the whole
    /// log every time is safe: the relay stores each entry once however often
    /// it is sent.
    pub fn push(
        &self,
        bill_id: &str,
        signer_seed: Option<&[u8]>,
        author_id: Option<&str>,
    ) -> Result<Vec<Value>> {
        // Checked here, where the seed enters: the signing closure below
        // cannot return an error.
        if let Some(seed) = signer_seed {
            if seed.len() != SEED_BYTES {
                return Err(HostError::Malformed(format!(
                    "an identity seed is {SEED_BYTES} bytes, not {}",
                    seed.len()
                )));
            }
        }
        let entries = self.store.read(bill_id)?;
        if entries.is_empty() {
            return Ok(entries);
        }
        let key = self.require_key(bill_id)?;

        let seed = signer_seed.map(<[u8]>::to_vec);
        let sign_closure = move |message: &[u8]| {
            Signer
                .sign(seed.as_deref().unwrap_or_default(), message)
                .expect("the seed's length was checked on entry")
        };

        let mut blobs = Vec::with_capacity(entries.len());
        for entry in &entries {
            let mine = signer_seed.is_some()
                && author_id.is_some()
                && entry.get("author").and_then(Value::as_str) == author_id
                && entry.get("sig").is_none();
            let to_seal = if mine {
                sign_entry(&SigningOnly(&sign_closure), entry, bill_id)
                    .map_err(|e| HostError::Sync(format!("An entry would not sign: {e}")))?
            } else {
                entry.clone()
            };
            blobs.push(Sealing.seal(&to_seal, &key)?);
        }
        self.relay.push(&channel_for_bill(bill_id), &blobs)?;
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

        // Merged only while this device still holds the bill's key. A bill
        // forgotten while the fetch was in flight is not written back: it
        // would return with no key, and the next Share would mint a key
        // nobody else holds.
        let still_held = self
            .keys
            .read_bill_key(bill_id)?
            .is_some_and(|k| !k.is_empty());
        if !still_held {
            return Err(HostError::Sync(format!(
                "{bill_id} was forgotten while it synced"
            )));
        }
        let merged = self.store.merge(bill_id, entries)?;
        Ok(SyncResult {
            entries: merged.entries,
            refused: merged.refused,
            unopenable,
        })
    }

    fn require_key(&self, bill_id: &str) -> Result<String> {
        // A keychain refuses while the session is locked. A poll that lands
        // then is a sync that could not run, and is reported as one.
        let key = self
            .keys
            .read_bill_key(bill_id)
            .map_err(|e| HostError::Sync(format!("Cannot read the key for {bill_id}: {e}")))?;
        match key {
            Some(key) if !key.is_empty() => Ok(key),
            _ => Err(HostError::Sync(format!(
                "No key for {bill_id}; it cannot be synced"
            ))),
        }
    }
}

/// A host that can do nothing but sign.
///
/// `sign_entry` takes a `BillHost`, and signing reads none of the rest of it.
/// Rather than require a whole wallet to seal an entry that already exists,
/// this supplies the one member that is used and panics on the others, so a
/// later change that starts reading them fails here instead of silently
/// signing with a placeholder identity.
struct SigningOnly<'a>(SignEntry<'a>);

impl BillHost for SigningOnly<'_> {
    fn signer(&self) -> Option<SignEntry<'_>> {
        Some(self.0)
    }

    fn me(&self) -> &str {
        unimplemented!("signing reads no author")
    }

    fn pay_to_address(&self) -> Option<&str> {
        unimplemented!("signing reads no address")
    }

    fn now(&self) -> String {
        unimplemented!("signing reads no clock")
    }

    fn random_bytes(&self, _byte_count: usize) -> Vec<u8> {
        unimplemented!("signing reads no randomness")
    }

    fn broadcast(&self, _payment_request_uri: &str) -> Sent {
        unimplemented!("signing sends nothing")
    }
}
