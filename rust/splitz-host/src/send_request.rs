//! The order every wallet's send of a payment request keeps (§14.3, §14.6).

use splitz_core::host::{check_proposal, ProposedOutput};

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
