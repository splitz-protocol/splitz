//! What the wallet tells this layer about itself, per call.
//!
//! SPEC.md §15 names seven interfaces a wallet implements. Across a foreign
//! boundary those are seven sets of callbacks, and a callback into a managed
//! runtime is the one thing every bindings generator gets wrong differently.
//!
//! So nothing here calls back. A wallet passes the facts it owns — who it
//! speaks as, the moment, the nonce — and gets an answer. Storage, the keychain, the relay and the send stay in the wallet's
//! own language, where they already are.

use splitz_core::host::{BillHost, Sent, SignEntry, VerifyEntry};

/// The facts §15.1 says a wallet owns, for one call.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct HostFacts {
    /// The participant id every entry this device writes is authored by.
    /// §10.4 decides what that id authorises. For a wallet that publishes an
    /// identity key it is the id that key derives (§10.7,
    /// `participant_id_for_key`), or the key binds nothing.
    pub me: String,
    /// A §9.3 instant. Read when an entry is written and never while folding:
    /// §10.2 orders a log by instant, so a fold that consulted a clock would
    /// answer differently for one unchanged entry set.
    pub now: String,
    /// Bytes nobody can predict, for §9.4's nonce. Sixteen are used; more are
    /// ignored and fewer is a refusal. Two bills opened in one second by one
    /// person are one bill when this can be guessed.
    pub nonce: Vec<u8>,
}

/// A `BillHost` over facts rather than callbacks.
///
/// `broadcast` is unreachable: sending is the wallet's, and every function
/// here that would have sent returns the payment request instead.
pub(crate) struct FactHost<'a> {
    pub facts: &'a HostFacts,
    pub sign: Option<SignEntry<'a>>,
    pub verify: Option<VerifyEntry<'a>>,
}

impl BillHost for FactHost<'_> {
    fn me(&self) -> &str {
        &self.facts.me
    }

    fn now(&self) -> String {
        self.facts.now.clone()
    }

    fn random_bytes(&self, byte_count: usize) -> Vec<u8> {
        let mut out = self.facts.nonce.clone();
        out.resize(byte_count, 0);
        out
    }

    fn broadcast(&self, _payment_request_uri: &str) -> Sent {
        unreachable!("this layer renders a payment request; sending is the wallet's")
    }

    fn signer(&self) -> Option<SignEntry<'_>> {
        self.sign
    }

    fn verifier(&self) -> Option<VerifyEntry<'_>> {
        self.verify
    }
}
