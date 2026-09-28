//! The protocol's host seam, implemented over a wallet.

use splitz_core::host::{BillHost, Sent, SignEntry, VerifyEntry};

use crate::wallet::{SplitsWallet, WalletSendPhase};

/// Hands `splitz-core` what SPEC.md §13 says a wallet owes it.
///
/// Two seams, not one. The protocol's `BillHost` is what the protocol asks
/// for; [`SplitsWallet`] (§15.1) is what a Zcash wallet already has. This maps
/// the second onto the first, so neither has to know about the other and
/// either can change without the other's callers noticing.
pub struct WalletBillHost<'a> {
    wallet: &'a dyn SplitsWallet,
    me: Option<String>,
    sign: Option<SignEntry<'a>>,
    verify: Option<VerifyEntry<'a>>,
}

impl<'a> WalletBillHost<'a> {
    pub fn new(wallet: &'a dyn SplitsWallet) -> Self {
        Self {
            wallet,
            me: None,
            sign: None,
            verify: None,
        }
    }

    /// Writes entries as `me` rather than as the account's own id.
    ///
    /// A host that signs passes the id its identity key derives (§10.7),
    /// which [`Signer::participant_id_from_seed`](crate::Signer::participant_id_from_seed)
    /// computes. A join written under any other id with that key is set aside
    /// with `participant_id_not_derived`.
    pub fn speaking_as(mut self, me: String) -> Self {
        self.me = Some(me);
        self
    }

    /// Signs the entries this device writes. Without it every participant is
    /// unauthenticated and the fold says so rather than claiming otherwise.
    pub fn signing_with(mut self, sign: SignEntry<'a>) -> Self {
        self.sign = Some(sign);
        self
    }

    /// Answers §10.7's signature questions. Absent one, no key is bound to any
    /// participant, which is a different claim from every key checking out.
    pub fn verifying_with(mut self, verify: VerifyEntry<'a>) -> Self {
        self.verify = Some(verify);
        self
    }
}

impl BillHost for WalletBillHost<'_> {
    fn me(&self) -> &str {
        self.me.as_deref().unwrap_or(&self.wallet.account().id)
    }

    fn now(&self) -> String {
        self.wallet.now()
    }

    fn random_bytes(&self, byte_count: usize) -> Vec<u8> {
        self.wallet.random_bytes(byte_count)
    }

    /// Maps the wallet's four send phases onto the protocol's three (§14.3).
    ///
    /// `Aborted` and `Failed` are one answer to a bill — nothing was spent —
    /// and differ only in the message a person is shown. `PendingBroadcast`
    /// keeps its own state: it is the one outcome from which nothing may be
    /// recorded and no retry is safe.
    fn broadcast(&self, payment_request_uri: &str) -> Sent {
        let outcome = self.wallet.sender().send(payment_request_uri);
        match outcome.phase {
            WalletSendPhase::Succeeded => match outcome.txid {
                Some(txid) => Sent::sent(txid),
                // Pending, not failed: the wallet says money left, and a
                // retry could pay it twice.
                None => Sent::pending(
                    Some("the wallet reported a send with no transaction id".to_owned()),
                    None,
                ),
            },
            WalletSendPhase::PendingBroadcast => Sent::pending(
                Some(outcome.status_message.unwrap_or_else(|| {
                    "The transaction was created but not broadcast yet. \
                     Check its status before trying again."
                        .to_owned()
                })),
                outcome.txid,
            ),
            WalletSendPhase::Failed | WalletSendPhase::Aborted => Sent::failed(Some(
                outcome
                    .error
                    .unwrap_or_else(|| "The transaction could not be sent.".to_owned()),
            )),
        }
    }

    fn signer(&self) -> Option<SignEntry<'_>> {
        self.sign
    }

    fn verifier(&self) -> Option<VerifyEntry<'_>> {
        self.verify
    }
}
