//! What a screen renders, as records a foreign caller reads directly.
//!
//! An **entry** does not appear here. Entries are the protocol's own wire
//! format (§9), a wallet never inspects one, and they cross this boundary as
//! the JSON text §9.3 canonicalises — which is what a relay carries and what a
//! scanned payload holds. Everything a person is shown is typed.

use std::collections::HashMap;

/// How a wallet's send ended (§14.3, §15.2).
#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum WalletSendPhase {
    Succeeded,
    PendingBroadcast,
    Failed,
    Aborted,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct WalletSendOutcome {
    pub phase: WalletSendPhase,
    /// Present when and only when `phase` is `Succeeded`.
    pub txid: Option<String>,
    pub status_message: Option<String>,
    pub error: Option<String>,
}

impl From<WalletSendOutcome> for splitz_host::WalletSendOutcome {
    fn from(o: WalletSendOutcome) -> Self {
        splitz_host::WalletSendOutcome {
            phase: match o.phase {
                WalletSendPhase::Succeeded => splitz_host::WalletSendPhase::Succeeded,
                WalletSendPhase::PendingBroadcast => splitz_host::WalletSendPhase::PendingBroadcast,
                WalletSendPhase::Failed => splitz_host::WalletSendPhase::Failed,
                WalletSendPhase::Aborted => splitz_host::WalletSendPhase::Aborted,
            },
            txid: o.txid,
            status_message: o.status_message,
            error: o.error,
        }
    }
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct Payout {
    /// `zec`, `swap` or `cash`.
    pub kind: String,
    pub address: Option<String>,
    pub asset: Option<String>,
    pub chain: Option<String>,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct Participant {
    pub id: String,
    pub name: String,
    /// Absent means a payment request cannot carry an output for this
    /// participant, which §8.5 requires be reported rather than dropped.
    pub pay_to: Option<String>,
    /// The Ed25519 key that alone may write as this participant (§10.7).
    pub identity_key: Option<String>,
    pub payouts: Vec<Payout>,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct Expense {
    pub id: String,
    pub description: String,
    pub paid_by: String,
    /// Minor units of the bill's currency (§2.1).
    pub amount: i64,
    pub currency: String,
    pub at: String,
    /// The §4 payload, as JSON. A wallet builds one with a split form rather
    /// than by hand, so it is not unpacked here.
    pub split_json: String,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct ExchangeRate {
    pub currency: String,
    pub minor_units_per_zec: i64,
    pub at: String,
    pub source: Option<String>,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct PaymentRecord {
    pub id: String,
    pub from: String,
    pub to: String,
    pub amount: i64,
    pub currency: String,
    /// `shieldedZec`, `swap` or `cash`. A label, not a branch.
    pub method: String,
    pub at: String,
    pub zatoshi: Option<i64>,
    pub reference: Option<String>,
    pub note: Option<String>,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct Bill {
    pub id: String,
    pub name: String,
    /// A bill has exactly one currency (§2.4).
    pub currency: String,
    pub split_mode: String,
    pub participants: Vec<Participant>,
    pub expenses: Vec<Expense>,
    pub payments: Vec<PaymentRecord>,
    /// The ids of the payments §10.5 says are confirmed. A payment not here is
    /// a claim and moves no balance.
    pub confirmed_payments: Vec<String>,
    pub rate: Option<ExchangeRate>,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct SetAside {
    pub id: String,
    /// The §12 code. §1 says the code is what a wallet turns into a sentence
    /// for its user, so no sentence crosses here.
    pub code: String,
}

/// An address a rejoin replaced. §13 requires a payer be shown one before
/// settling to it.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct ReplacedAddress {
    pub id: String,
    pub from: Option<String>,
    pub to: Option<String>,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct Identities {
    /// Participant id to the key §10.7 binds to it.
    pub bound: HashMap<String, String>,
    /// Ids two keys each claim. Neither is bound, and a wallet MUST NOT settle
    /// to one without putting it in front of the payer first.
    pub contested: Vec<String>,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct FoldedBill {
    pub bill: Bill,
    pub set_aside: Vec<SetAside>,
    pub withdrawn: Vec<String>,
    pub replaced_addresses: Vec<ReplacedAddress>,
    pub identities: Identities,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct DirectDebt {
    pub from: String,
    pub to: String,
    pub amount: i64,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct Settlement {
    pub from: String,
    pub to: String,
    pub amount: i64,
    /// The original debts this payment discharges (§6.3).
    pub covers: Vec<DirectDebt>,
}

/// A recipient a payment request could not carry (§8.4).
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct Unpayable {
    pub id: String,
    /// `no_address` or `payout_not_zec`. The two need different remedies.
    pub reason: String,
    pub minor_units: i64,
}

/// A debt already paid whose payee has not confirmed (§10.5, §14.4).
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct Awaiting {
    pub to: String,
    /// What the plan still says is owed. An unconfirmed payment does not
    /// reduce it.
    pub owed: i64,
    /// What this payer has already sent. Less than `owed` on a part payment,
    /// and presenting one as the other states something untrue.
    pub paid: i64,
}

/// A debt held back because two keys claim that participant's id (§10.7).
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct Contested {
    pub to: String,
    pub amount: i64,
    /// The address standing on the bill, which may be an impostor's.
    pub address: Option<String>,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct Obligation {
    /// None when nothing could be carried.
    pub uri: Option<String>,
    pub unpayable: Vec<Unpayable>,
    /// What the URI sends. Never present a figure pricing the whole obligation
    /// as this.
    pub carried_minor_units: i64,
    pub withheld_minor_units: i64,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct PayerObligation {
    pub settlements: Vec<Settlement>,
    pub awaiting: Vec<Awaiting>,
    pub contested: Vec<Contested>,
    pub request: Obligation,
}

/// What one entry did (the log, read as a history).
#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum BillEventKind {
    Opened,
    Joined,
    AddressChanged,
    ExpenseAdded,
    ExpenseAmended,
    EntryWithdrawn,
    PaymentRecorded,
    PaymentConfirmed,
    Priced,
    /// An entry kind this reader does not name. Shown rather than hidden.
    Other,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct BillEvent {
    pub entry_id: String,
    pub kind: BillEventKind,
    pub author: String,
    pub at: String,
    pub subject: Option<String>,
    pub amount_minor_units: Option<i64>,
    pub description: Option<String>,
    pub method: Option<String>,
    /// A swap's own identifier. **Not a Zcash txid** (§9.2).
    pub reference: Option<String>,
    pub withdrawn: bool,
    pub refused_code: Option<String>,
    pub confirmed: bool,
    pub applied: bool,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct TradableAsset {
    pub asset_id: String,
    pub symbol: String,
    pub chain: String,
    pub decimals: i32,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct SwapQuote {
    pub deposit_address: String,
    pub deposit_memo: Option<String>,
    pub amount_in_zatoshi: i64,
    pub amount_out: String,
    pub asset: TradableAsset,
    /// A §9.3 instant.
    pub deadline: String,
    pub reference: Option<String>,
}

#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapState {
    AwaitingDeposit,
    Processing,
    /// The provider reports the recipient was paid. **Still not a
    /// confirmation**: §10.5 says only the recipient settles a debt.
    Delivered,
    Failed,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct SwapStatus {
    pub state: SwapState,
    /// On the destination chain. MUST NOT be recorded as the §10.5 payment.
    pub destination_tx_hash: Option<String>,
    pub detail: Option<String>,
}

/// What one sync produced.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct SyncResult {
    pub entry_count: u32,
    pub refused: Vec<SetAside>,
    /// Blobs in the channel that would not open under this bill's key. A
    /// channel where every blob is unopenable is a wrong key, and that looks
    /// identical to a quiet relay unless somebody counts.
    pub unopenable: u32,
}

/// What a settle attempt reported (§14.3).
///
/// Three outcomes, not two. `sent` and `pending` are never both true, and
/// neither being true means nothing was spent. **A pending send may still
/// land**: nothing may be recorded for it and no retry is safe.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct SettleResult {
    pub sent: bool,
    pub pending: bool,
    /// Present when and only when `sent`.
    pub txid: Option<String>,
    pub detail: Option<String>,
    /// Payment entries appended to the log, one per recipient.
    pub recorded: u32,
}
