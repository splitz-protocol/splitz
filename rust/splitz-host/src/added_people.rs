//! Somebody the creator puts on a bill before they have joined (§14.11).

use serde_json::Value;
use splitz_core::host::{join_bill, BillHost, FoldedBill, Sent, SignEntry, VerifyEntry};
use splitz_core::{code, Result, SplitError};

/// A host writing as `me` on `inner`'s clock and randomness, for somebody
/// added by hand.
///
/// Signs nothing: this device holds no key of theirs, and signing an entry
/// authored by them with this device's key would assert something false.
/// §10.7 binds nothing to an entry written this way, which is the honest
/// state — the person binds their own identity by joining from a device of
/// their own.
pub struct HostAs<'a> {
    inner: &'a dyn BillHost,
    me: String,
}

impl<'a> HostAs<'a> {
    pub fn new(inner: &'a dyn BillHost, me: impl Into<String>) -> Self {
        Self {
            inner,
            me: me.into(),
        }
    }
}

impl BillHost for HostAs<'_> {
    fn me(&self) -> &str {
        &self.me
    }

    fn now(&self) -> String {
        self.inner.now()
    }

    fn random_bytes(&self, byte_count: usize) -> Vec<u8> {
        self.inner.random_bytes(byte_count)
    }

    fn broadcast(&self, payment_request_uri: &str) -> Sent {
        self.inner.broadcast(payment_request_uri)
    }

    fn signer(&self) -> Option<SignEntry<'_>> {
        None
    }

    fn verifier(&self) -> Option<VerifyEntry<'_>> {
        self.inner.verifier()
    }

    fn reads_address(&self, address: &str) -> bool {
        self.inner.reads_address(address)
    }
}

/// The `joinBill` that puts `name` on `folded` under `id`, written as them and
/// unsigned through [`HostAs`] (§14.11).
///
/// Refused with `duplicate_participant` when `id` is this device's own or
/// already on the bill: a join under a taken id renames the person holding it
/// and wipes their payout, and one under this device's own id renames its
/// holder. Refused with `bill_missing_entry_payload` for an empty `id`, which
/// names nobody (§9.1).
pub fn add_person_entry(
    host: &dyn BillHost,
    folded: &FoldedBill,
    id: &str,
    name: &str,
) -> Result<Value> {
    if id.is_empty() {
        return Err(SplitError::new(
            code::BILL_MISSING_ENTRY_PAYLOAD,
            "A person added needs an id",
        ));
    }
    if id == host.me() || folded.bill.participant(id).is_some() {
        return Err(SplitError::new(
            code::DUPLICATE_PARTICIPANT,
            "Somebody on this bill already goes by that",
        ));
    }
    join_bill(&HostAs::new(host, id), Some(name), None, None, None)
}

/// Whether this device is on `folded` as itself (§10.7): a record under `me`
/// that the fold binds to a key — this device's, since `me` is the id its key
/// derives.
///
/// A record under `me` alone is not: anybody holding the invite can write an
/// unsigned join under an id they know, with their own payout, before the
/// person joins. A device that took that record for its own would never write
/// its signed join, and every payment owed to it would go to the address the
/// record names. While this is false, the device writes its own join, which
/// binds its key and takes the record back.
pub fn joined_as_me(folded: &FoldedBill, me: &str) -> bool {
    folded.bill.participant(me).is_some() && folded.identities.bound.contains_key(me)
}
