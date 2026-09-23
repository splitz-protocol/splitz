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
    /// Read once, when the host is made.
    ///
    /// `BillHost` borrows the address and the seam answers with an owned one,
    /// so something has to hold it. Holding it here also means one fold sees
    /// one address: a fold that re-read it could answer differently for one
    /// unchanged entry set, which is the property §10.2 rests on.
    pay_to: Option<String>,
    sign: Option<SignEntry<'a>>,
    verify: Option<VerifyEntry<'a>>,
}

impl<'a> WalletBillHost<'a> {
    pub fn new(wallet: &'a dyn SplitsWallet) -> Self {
        Self {
            pay_to: wallet.sender().pay_to_address(),
            wallet,
            sign: None,
            verify: None,
        }
    }

    /// Signs the entries this device writes. Without it every participant is
    /// unauthenticated and the fold says so rather than claiming otherwise.
    pub fn signing_with(mut self, sign: SignEntry<'a>) -> Self {
        self.sign = Some(sign);
        self
    }

    /// Answers §10.7's signature questions. Absent one, no key is bound to any
    /// participant, which is a different claim from nothing being contested.
    pub fn verifying_with(mut self, verify: VerifyEntry<'a>) -> Self {
        self.verify = Some(verify);
        self
    }
}

impl BillHost for WalletBillHost<'_> {
    fn me(&self) -> &str {
        &self.wallet.account().id
    }

    fn pay_to_address(&self) -> Option<&str> {
        self.pay_to.as_deref()
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
                None => Sent::pending(Some(
                    "the wallet reported a send with no transaction id".to_owned(),
                )),
            },
            WalletSendPhase::PendingBroadcast => {
                Sent::pending(Some(outcome.status_message.unwrap_or_else(|| {
                    "The transaction was created but not broadcast yet. \
                     Check its status before trying again."
                        .to_owned()
                })))
            }
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
