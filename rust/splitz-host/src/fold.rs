//! Folding a log whose signatures have to be checked first.

use serde_json::Value;
use splitz_core::host::{BillLog, FoldedBill, SignEntry};
use splitz_core::SplitError;

use crate::signing::Signer;
use crate::wallet::SplitsWallet;
use crate::wallet_bill_host::WalletBillHost;

/// What a fold that had to check signatures can fail with.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FoldFailure {
    /// The log itself was refused (§10.3).
    Refused(SplitError),
    /// The fold asked a signature question nobody had answered.
    ///
    /// Not an ordinary refusal. It means [`Signer::prepare`] no longer
    /// anticipates every pair §10.7 asks about, so the identities the fold
    /// reported are wrong in a direction that looks exactly like "no
    /// signature" — the quiet failure this two-pass exists to avoid.
    Unanswered(Vec<String>),
}

impl std::fmt::Display for FoldFailure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FoldFailure::Refused(e) => write!(f, "{e}"),
            FoldFailure::Unanswered(pairs) => write!(
                f,
                "the fold asked about {} (entry, key) pair(s) that were never verified",
                pairs.len()
            ),
        }
    }
}

impl std::error::Error for FoldFailure {}

/// Folds `entries` with signatures checked.
///
/// Two passes, and not because anything here is asynchronous: §10.7 requires
/// one verifier to answer one question the same way every time it is asked, on
/// every device. Verifying every pair up front and folding against a pure
/// lookup is what makes that true by construction rather than by inspection.
///
/// Pass `seed` when this device should sign what it writes; it changes nothing
/// about the fold, which only ever verifies.
pub fn fold_verified(
    wallet: &dyn SplitsWallet,
    entries: &[Value],
    seed: Option<&[u8]>,
) -> Result<FoldedBill, FoldFailure> {
    let signer = Signer;
    let verified = signer.prepare(entries.iter());

    let sign_closure;
    let sign: Option<SignEntry<'_>> = match seed {
        None => None,
        Some(seed) => {
            let owned = seed.to_vec();
            sign_closure = move |message: &[u8]| {
                signer
                    .sign(&owned, message)
                    .expect("an identity seed is 32 bytes")
            };
            Some(&sign_closure)
        }
    };

    let verify = |entry: &Value, key: &str| verified.verify(entry, key);
    let mut host = WalletBillHost::new(wallet).verifying_with(&verify);
    if let Some(sign) = sign {
        host = host.signing_with(sign);
    }

    // The fold is attempted, and the unanswered check runs whether or not it
    // succeeded. An unanswered pair makes a *failure* as untrustworthy as a
    // result: §10.3 drops a create whose signature does not verify, so a
    // question nobody answered turns into `log_no_create` — a refusal that
    // names the log and says nothing about the verifier that caused it.
    let folded = BillLog::with_entries(&host, entries.to_vec()).fold();
    let unanswered = verified.unanswered();
    if !unanswered.is_empty() {
        return Err(FoldFailure::Unanswered(unanswered));
    }
    folded.map_err(FoldFailure::Refused)
}

/// Folds `entries` without checking any signature.
///
/// For a device that holds no identity yet. §10.7 then binds no key and
/// reports no contest, which is a different claim from reporting that nothing
/// is contested — and is the honest one here.
pub fn fold_unverified(
    wallet: &dyn SplitsWallet,
    entries: &[Value],
) -> Result<FoldedBill, SplitError> {
    let host = WalletBillHost::new(wallet);
    BillLog::with_entries(&host, entries.to_vec()).fold()
}
