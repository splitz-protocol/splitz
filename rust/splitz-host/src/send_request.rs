//! The order every wallet's send of a payment request keeps (§14.3, §14.6).

use splitz_core::host::{check_proposal, ProposedOutput};
use splitz_core::{render_amount, MAX_ZATOSHI};

use crate::wallet::{WalletSendOutcome, WalletSendPhase};

/// Why the payments a wallet read from `uri` are not the ones it asks for, in
/// words for the payer, or `None` when they are (§14.6).
///
/// `read` is what the wallet's own ZIP 321 reader produced: the reading its
/// proposal is built from. A request this protocol did not write is refused
/// with its code's sentence.
pub fn proposal_problem(uri: &str, read: &[ProposedOutput]) -> Option<String> {
    match check_proposal(uri, read) {
        Err(e) => Some(
            splitz_core::describe_code(e.code)
                .map(str::to_owned)
                .unwrap_or_else(|| e.code.to_owned()),
        ),
        Ok(check) if check.matches() => None,
        Ok(_) => Some("Your wallet read this payment differently. Nothing was sent.".to_owned()),
    }
}

/// Why a proposal whose fee is `fee` may not be sent to a payer who reviewed
/// `reviewed_fee`, in words for the payer, or `None` when it may (§14.2). Both
/// are zatoshi.
///
/// A fee above the reviewed one is refused; one at or below it is not. Notes
/// can change between the review and the send, and a person confirms the
/// figure they saw, not one chosen after.
pub fn fee_raised_problem(reviewed_fee: i64, fee: i64) -> Option<String> {
    (fee > reviewed_fee).then(|| {
        format!(
            "The fee went up from {} to {} since you reviewed this payment. \
             Nothing was sent; review it again.",
            zec(reviewed_fee),
            zec(fee)
        )
    })
}

/// `zatoshi` as ZEC in §8.1's form, or as zatoshi when it is not an amount
/// §8.1 renders.
fn zec(zatoshi: i64) -> String {
    match render_amount(zatoshi) {
        Ok(amount) if zatoshi <= MAX_ZATOSHI => format!("{amount} ZEC"),
        _ => format!("{zatoshi} zatoshi"),
    }
}

/// Sends `uri` in the order §14.3 and §14.6 require, as a `WalletSender::send`
/// does.
///
/// `read` is the wallet's own reading of the request, held against it with
/// [`proposal_problem`] before anything is built; `propose` builds and signs;
/// `broadcast` hands the transaction to the network and says which of the
/// three outcomes occurred.
///
/// A refusal before a transaction is built — a request read differently, too
/// little to spend, a reader or builder that failed — is `Failed`, never a
/// send that may still land: nothing was built, so nothing can arrive.
/// `broadcast`'s own outcome is returned as it is.
pub fn send_payment_request<P>(
    uri: &str,
    read: impl FnOnce() -> Result<Vec<ProposedOutput>, String>,
    propose: impl FnOnce() -> Result<P, String>,
    broadcast: impl FnOnce(P) -> WalletSendOutcome,
) -> WalletSendOutcome {
    let failed = |error: String| WalletSendOutcome {
        phase: WalletSendPhase::Failed,
        txid: None,
        status_message: None,
        error: Some(error),
    };
    let outputs = match read() {
        Ok(outputs) => outputs,
        Err(e) => return failed(format!("The wallet could not read this payment: {e}")),
    };
    if let Some(why) = proposal_problem(uri, &outputs) {
        return failed(why);
    }
    match propose() {
        Ok(proposal) => broadcast(proposal),
        Err(e) => failed(format!("The wallet could not build this payment: {e}")),
    }
}

/// [`send_payment_request`] for a payer shown `reviewed_fee` zatoshi as the
/// fee.
///
/// `propose` answers the proposal and its fee. A proposal whose fee
/// [`fee_raised_problem`] refuses is handed to `discard` and never broadcast,
/// and the send is `Failed` with that sentence; nothing was broadcast, so
/// `discard` has no outcome to change.
pub fn send_reviewed_payment_request<P>(
    uri: &str,
    reviewed_fee: i64,
    read: impl FnOnce() -> Result<Vec<ProposedOutput>, String>,
    propose: impl FnOnce() -> Result<(P, i64), String>,
    discard: impl FnOnce(P),
    broadcast: impl FnOnce(P) -> WalletSendOutcome,
) -> WalletSendOutcome {
    send_payment_request(
        uri,
        read,
        || propose().map(|(proposal, fee)| (proposal, fee_raised_problem(reviewed_fee, fee))),
        |(proposal, why): (P, Option<String>)| match why {
            Some(why) => {
                discard(proposal);
                WalletSendOutcome {
                    phase: WalletSendPhase::Failed,
                    txid: None,
                    status_message: None,
                    error: Some(why),
                }
            }
            None => broadcast(proposal),
        },
    )
}
