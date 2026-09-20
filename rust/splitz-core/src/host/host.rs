//! What the host layer needs from the wallet that embeds it.
//!
//! The protocol hands transport, keys, signing, broadcast and the clock to its
//! host (§13), and this layer is not a wallet either — it holds no key, opens
//! no socket and reads no clock of its own. Everything it cannot do is
//! declared here, so the whole feature can be exercised without one.

use serde_json::Value;

/// How a send ended.
///
/// A wallet has a third answer between success and failure: a transaction
/// built and signed but not yet handed to the network, which may still land
/// later. It is neither paid nor unpaid, and collapsing it into either one
/// loses money — recorded as paid, a transaction that never lands leaves a
/// real debt showing as settled; recorded as nothing, one that does land is
/// paid a second time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendResult {
    /// The network has the transaction.
    Sent,
    /// Built, not broadcast. Nothing may be recorded from it, and no retry is
    /// safe until the wallet says which way it went.
    Pending,
    /// It will not land. Nothing was spent.
    Failed,
}

/// What a send produced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sent {
    pub result: SendResult,
    /// Present when and only when `result` is [`SendResult::Sent`]. It becomes
    /// the id of the payment entry, so the record of a payment and the
    /// transaction that made it carry one identifier.
    pub txid: Option<String>,
    /// What to put in front of a person: why it failed, or what to check
    /// before trying again.
    pub detail: Option<String>,
}

impl Sent {
    // `Sent::sent` reads as a tautology to a lint and as the plain word to a
    // caller. The three constructors are named for the three outcomes, and
    // renaming one of them to satisfy the lint would leave the set uneven.
    #[allow(clippy::self_named_constructors)]
    pub fn sent(txid: impl Into<String>) -> Self {
        Self {
            result: SendResult::Sent,
            txid: Some(txid.into()),
            detail: None,
        }
    }

    pub fn pending(detail: Option<String>) -> Self {
        Self {
            result: SendResult::Pending,
            txid: None,
            detail,
        }
    }

    pub fn failed(detail: Option<String>) -> Self {
        Self {
            result: SendResult::Failed,
            txid: None,
            detail,
        }
    }
}

/// The host's signature over an entry's signing message (§10.6).
///
/// The curve operation is the host's (§13). This crate fixes the message and
/// everything around the answer, never the answer itself.
pub type SignEntry<'a> = &'a dyn Fn(&[u8]) -> String;

/// Whether the host takes an entry's signature to verify against a key.
pub type VerifyEntry<'a> = &'a dyn Fn(&Value, &str) -> bool;

/// The wallet, as this layer needs it.
///
/// One trait rather than loose callbacks so a host implements one thing and a
/// test fakes one thing.
pub trait BillHost {
    /// The participant id this device speaks as. Every entry it writes is
    /// authored by this id, and §10.4 decides what that authorises.
    fn me(&self) -> &str;

    /// The address this device is paid at, or `None` when it has none to
    /// offer. A participant with no address is reported as unpayable rather
    /// than silently dropped from a settlement.
    fn pay_to_address(&self) -> Option<&str>;

    /// A moment, as an RFC 3339 instant.
    ///
    /// Taken from the host rather than from a clock this crate reads, so a
    /// test can hold it still: §9.3 instants order a log, and a log that
    /// reorders between runs cannot be asserted. A string rather than a date
    /// type because this crate depends on no calendar library, and §9.3 is
    /// text on the wire either way — [`crate::canonical_instant`] refuses
    /// anything that is not one.
    fn now(&self) -> String;

    /// Bytes nobody can predict.
    ///
    /// §9.4 derives a bill's id from a nonce, so two bills created in the same
    /// second by the same person are the same bill unless this is
    /// unpredictable. The host supplies it because the host knows what secure
    /// randomness means on its platform.
    fn random_bytes(&self, byte_count: usize) -> Vec<u8>;

    /// What the wallet does with a payment request this layer renders.
    ///
    /// Synchronous, and deliberately: this crate pulls in no async runtime, so
    /// it cannot own the executor a future would need. A host whose send is
    /// asynchronous blocks on it here, where it already knows which runtime it
    /// is on.
    fn broadcast(&self, payment_request_uri: &str) -> Sent;

    /// This wallet's signer, or `None` when it does not sign entries.
    ///
    /// Without it every participant is unauthenticated and the fold says so
    /// rather than claiming otherwise. With it, §10.7 binds a key to a
    /// participant and a contested identity is reported as contested.
    fn signer(&self) -> Option<SignEntry<'_>> {
        None
    }

    /// Verifies a signature the way §10.7 asks, or `None` when this wallet
    /// does not verify.
    ///
    /// `None` and "verifies nothing" are different claims: without a verifier
    /// no self-claim is checked, so nothing is bound and nothing is contested,
    /// which is not the same as every claim failing.
    fn verifier(&self) -> Option<VerifyEntry<'_>> {
        None
    }
}
