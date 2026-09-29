//! What a screen renders, as records a foreign caller reads directly.
//!
//! An **entry** does not appear here. Entries are the protocol's own wire
//! format (§9), a wallet never inspects one, and they cross this boundary as
//! the JSON text §9.3 canonicalises — which is what a relay carries and what a
//! scanned payload holds. Everything a person is shown is typed.

use std::collections::HashMap;

/// Which of §14.3's three outcomes a send ended in, as a wallet tells
/// `pending_send_after`.
///
/// The wallet sends, so the wallet owns what happened: it calls
/// `payment_entries_for_send` when — and only when — a transaction reached
/// the network, and records nothing otherwise.
#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendEnded {
    /// The transaction reached the network.
    ReachedNetwork,
    /// Nothing was built, or nothing was spent.
    Refused,
    /// Built and signed and not known to have reached the network — or the
    /// send raised, so which way it went is unknown. It may still land.
    Unresolved,
}

/// The send a stored note says is under way from one bill (§14.3). While a
/// wallet holds one, it starts no other send from that bill.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct PendingSendHeld {
    /// True when the note would not read. It blocks exactly as a readable one
    /// does, and nothing can be recorded from it: a person records what they
    /// paid by hand, then clears it.
    pub damaged: bool,
    /// The request handed to the wallet. Empty when `damaged`.
    pub uri: String,
    /// When the send was started, a §9.3 instant. Empty when `damaged`.
    pub at: String,
    /// The transaction, when the wallet named one.
    pub txid: Option<String>,
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
    /// The rate the payment was priced at, when its record states one: what a
    /// payee compares with what arrived before confirming (§9.2).
    pub paid_at_rate: Option<ExchangeRate>,
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
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct FoldedBill {
    pub bill: Bill,
    pub set_aside: Vec<SetAside>,
    pub withdrawn: Vec<String>,
    pub replaced_addresses: Vec<ReplacedAddress>,
    pub identities: Identities,
    /// What each payment record says, by the payment's id: the `record` a
    /// confirmation of it carries (§10.5).
    pub payment_digests: std::collections::HashMap<String, String>,
    /// Who wrote each payment record, by the payment's id (§14.4).
    pub payment_authors: std::collections::HashMap<String, String>,
    /// The entry that introduced each expense, by the expense's own id: what
    /// an amendment or a withdrawal of it targets. The fold's answer, not the
    /// log's, which also holds entries the fold set aside.
    pub expense_entries: std::collections::HashMap<String, String>,
    /// Who wrote each expense, by the expense's own id.
    pub expense_authors: std::collections::HashMap<String, String>,
    /// The entry that recorded each payment, by the payment's id.
    pub payment_entries: std::collections::HashMap<String, String>,
    /// The `setRate` entry whose rate the bill carries.
    pub rate_entry: Option<String>,
    /// Who wrote that `setRate`: the name §14.2 puts beside the rate.
    pub rate_author: Option<String>,
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
    /// `no_address`, `bad_address`, `payout_not_zec` or `unpriceable` (a debt
    /// past what one request can price at the bill's rate). Each needs a
    /// different remedy.
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
    /// Who that unconfirmed money went to. Not `to` when netting rerouted the
    /// debt (§6.3): the payment to confirm, or to take back, is theirs.
    pub paid_to: Vec<String>,
}

/// One output of a payment request: who it pays and what it sends.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct RequestPayment {
    pub to: String,
    pub zatoshi: i64,
}

/// One bill's entries, as a wallet holds them, named by the bill they belong
/// to.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct HeldBill {
    pub bill_id: String,
    pub entries: Vec<String>,
}

/// Money a wallet received in one transaction: the sum of that transaction's
/// outputs to its account, in zatoshi.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct IncomingTransaction {
    pub txid: String,
    pub zatoshi: i64,
}

/// A payment record to this device, and the transaction it names (§14.7).
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct Arrival {
    pub bill_id: String,
    /// What the payee is shown before confirming (§14.2).
    pub payment: PaymentRecord,
    /// The digest a confirmation of `payment` carries as `record` (§10.5).
    pub record: String,
    /// The transaction, lower-cased.
    pub txid: String,
}

/// Records to this device whose transaction arrived (§14.7). Only `arrived`
/// may be confirmed, with `walletReceived`, once the payee has been shown it.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct Arrivals {
    pub arrived: Vec<Arrival>,
    /// Their transaction arrived with less ZEC than they state.
    pub short: Vec<Arrival>,
    /// They state no ZEC, so nothing can be checked.
    pub unstated: Vec<Arrival>,
}

/// What this device and one other participant owe each other in one
/// currency, across every bill that names both.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct Standing {
    /// The other participant. One person is one id across bills only when
    /// the id derives from their key (§10.7).
    pub with_id: String,
    pub currency: String,
    /// What their settlement plans ask them to pay this device.
    pub owed_to_me: i64,
    /// What the plans ask this device to pay them.
    pub owed_by_me: i64,
    /// Recorded to them by this device, not yet confirmed; still owed.
    pub sent_awaiting: i64,
    /// Recorded to this device by them, not yet confirmed.
    pub received_awaiting: i64,
    pub bill_ids: Vec<String>,
}

/// Standings across bills, and the bills left out whole with the §12 code
/// that kept each out.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct Totals {
    pub standings: Vec<Standing>,
    pub uncounted: HashMap<String, String>,
}

/// What a Zcash address is (§8.6).
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct ParsedAddress {
    /// `main`, `test` or `regtest`. A transparent regtest address answers
    /// `test`: the two share lead bytes.
    pub network: String,
    /// `p2pkh`, `p2sh`, `tex`, `sapling` or `unified`.
    pub kind: String,
    /// A Unified Address's typecodes in encoding order; empty otherwise.
    pub receivers: Vec<u32>,
    /// Whether a memo reaches the recipient.
    pub can_receive_memo: bool,
}

/// One payment a wallet is about to make, as its own ZIP 321 reader produced
/// it: an address and an amount.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct ProposedOutput {
    pub address: String,
    pub zatoshi: i64,
}

/// How the payments a wallet is about to sign differ from its request
/// (§14.6). Sign only when both lists are empty.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct ProposalCheck {
    /// Payments the request carries that nothing proposed matches.
    pub missing: Vec<ProposedOutput>,
    /// Proposed payments the request does not carry.
    pub unexpected: Vec<ProposedOutput>,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct Obligation {
    /// None when nothing could be carried.
    pub uri: Option<String>,
    /// What the request sends each recipient, in the order it carries them.
    pub payments: Vec<RequestPayment>,
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
    pub request: Obligation,
    /// The rate `request` was priced at: what a record of the send states as
    /// `paidAtRate` (§9.2).
    pub rate: ExchangeRate,
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
    /// The payout address the provider delivers to: what this quote was
    /// taken for. Compare it with the payee's payout before sending the
    /// deposit — a payout replaced since means a new quote.
    pub recipient: Option<String>,
    pub deposit_memo: Option<String>,
    pub amount_in_zatoshi: i64,
    pub amount_out: String,
    /// The least the recipient receives once slippage is applied.
    pub min_amount_out: Option<String>,
    pub asset: TradableAsset,
    /// A §9.3 instant.
    pub deadline: String,
    pub reference: Option<String>,
}

#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum SwapState {
    AwaitingDeposit,
    Processing,
    /// The provider has begun returning the ZEC; `Failed` follows.
    Refunding,
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
