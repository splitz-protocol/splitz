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

/// A transaction the wallet built itself: what a person's word that a send
/// left nothing is checked against (§14.3).
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct OwnTransaction {
    /// The transaction's id, as the wallet reports it.
    pub txid: String,
    /// When the wallet created it, a §9.3 instant.
    pub created: String,
    /// What it sent out of the account, in zatoshi: the balance it took less
    /// its fee. `None` when the wallet cannot say, which makes it one that
    /// may be any send.
    pub sent: Option<i64>,
}

/// Why a person may not say a send left nothing in the wallet (§14.3).
#[derive(uniffi::Enum, Debug, Clone, PartialEq, Eq)]
pub enum UnsentClaimRefusal {
    /// The wallet is still sending a transaction, and it may be this one.
    StillSending,
    /// The wallet built `txid` at or after the note was written.
    BuiltSince { txid: String },
}

/// Where a transaction the wallet holds stands.
#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionState {
    /// In a block: it went through.
    Mined,
    /// Not mined and not expired: the wallet may still broadcast it.
    Waiting,
    /// Expired unmined: it can no longer go through.
    Expired,
    /// The history could not be read: it may be in any state above.
    Unread,
}

/// Why a person may not clear a pending-send note that names its transaction
/// (§14.3).
#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamedSendRefusal {
    /// The wallet still holds the transaction and may broadcast it.
    Waiting,
    /// The wallet shows it went through: record it rather than clear it.
    Mined,
    /// The wallet's history could not be read, so nothing says the
    /// transaction can no longer land.
    Unread,
}

/// Why this device may not withdraw its own record of a shielded payment
/// (§14.4).
#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnPaymentWithdrawal {
    /// The wallet shows the transaction the record names went through.
    Mined,
    /// The wallet still holds that transaction and may send it.
    Waiting,
    /// The wallet's history could not be read, so nothing says the
    /// transaction can no longer reach the payee.
    Unread,
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
    /// Who opened the bill: the author of the one `createBill` the fold kept
    /// (§10.1). Decide the creator's powers (§10.4, §10.8) from this, never
    /// from whichever create a log lists first.
    pub creator_id: String,
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
    /// The creator's close the bill is closed by (§10.9), or none while open.
    pub close_entry: Option<String>,
    /// The digest of the expenses as they stand (§10.9): what a close written
    /// now covers.
    pub closed_over: String,
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

/// The refunds behind a settlement's unexplained part (`refunds_behind`).
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct RefundsBehind {
    /// What the refunds move onto the payer, in the bill's minor units.
    pub refunded: i64,
    /// Who wrote them, sorted; empty for an expense with no known author.
    pub authors: Vec<String>,
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
    /// What other payers have sent `to` and is waiting to be confirmed, when
    /// that is why the debt is held (§14.4). Zero otherwise.
    pub others_paid: i64,
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
    /// The text memos it carried to this account; `None` when the wallet
    /// cannot say. Empty is an answer: it carried none (§14.7).
    #[uniffi(default = None)]
    pub memos: Option<Vec<String>>,
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
    /// Records naming a transaction that records from another payer also
    /// name. None is proposed; the payee settles which it pays first.
    pub disputed: Vec<Arrival>,
    /// Their ZEC, at the bill's rate, is worth under 95% of what they settle,
    /// or the bill has no rate in their currency. None is proposed.
    pub underpriced: Vec<Arrival>,
    /// Their transaction's memos, read by the wallet, name another bill or
    /// none. None is proposed (§14.7).
    pub unbound: Vec<Arrival>,
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

/// Which of §14.2's facts a review finding is about.
#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReviewRule {
    /// Every recipient the request cannot carry, with the reason (§8.5).
    Unpayable,
    /// Every pay-to address the fold recorded as replaced (§10.3).
    ReplacedAddress,
    /// Every debt with a payment recorded and not yet confirmed (§10.5,
    /// §14.4).
    Awaiting,
    /// Every recipient paid by a preference other than their first (§14.8).
    LowerPreference,
    /// Every recipient paid more than the debts the bill records explain
    /// (§6).
    Unexplained,
    /// The rate the request was priced at.
    Rate,
    /// The ZEC amount and address of every output.
    Output,
    /// The ZEC a payment record says was sent, shown to its payee.
    PayeeZec,
    /// The rate a payment record was priced at, shown to its payee.
    PayeeRate,
    /// A payment record's reference, shown to its payee.
    PayeeReference,
}

/// Bytes from the platform's cryptographically secure generator. A record
/// rather than a bare byte string, so every generator carries it alike.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct RandomBytes {
    pub bytes: Vec<u8>,
}

/// A secret only the wallet's owner holds, such as bytes derived from the
/// mnemonic and passphrase. A record rather than a bare byte string, so every
/// generator carries it alike: one that writes a bare `Vec<u8>` without its
/// length prefix reads the first bytes as a length, and the same mnemonic
/// derives another identity in that language than in the others.
#[derive(uniffi::Record, Clone, PartialEq, Eq)]
pub struct SecretBytes {
    pub bytes: Vec<u8>,
}

/// An invite's expiry against the caller's clock (§11.1). A record rather than
/// a bare flag, so every generator carries it alike.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct InviteExpiry {
    /// The Unix time in seconds the invite states, when it states one.
    pub expiry: Option<i64>,
    /// Whether that is before the clock the caller passed.
    pub expired: bool,
}

/// What a peer lacks, and whether it fits one scanned square (§14.5).
///
/// Three states: `missing` 0 is nothing missing; `uri` is the square carrying
/// the entries the peer lacks; `too_big_code` is the §12 refusal when they
/// will not fit one square (`payload_too_large` at the cap), which needs a
/// relay. A record rather than an enum, so every generator carries it alike.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct Delta {
    /// How many entries the peer lacks.
    pub missing: u64,
    pub uri: Option<String>,
    pub too_big_code: Option<String>,
}

/// One fact §14.2 requires that a review screen's text does not show.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct ReviewFinding {
    pub rule: ReviewRule,
    /// The fact, in words: whose, and which part of it.
    pub fact: String,
    /// The text looked for and not found.
    pub expected: String,
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
    /// The creator closed the bill for settling (§10.9).
    ClosedForSettling,
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
    /// For a restatement (§10.8), the participant it takes off.
    pub taken_off: Option<String>,
    /// For a restatement, the one participant who takes over `taken_off`'s
    /// part, when exactly one does: a merge (§14.11).
    pub moved_to: Option<String>,
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

/// Why a swap's deposit may not be sent (§15.7). Each is a refusal a payer
/// fixes by getting a new quote, or by waiting, as its words say.
#[derive(uniffi::Enum, Debug, Clone, PartialEq, Eq)]
pub enum SwapSendRefusal {
    /// The quote's deadline has passed.
    Expired,
    /// The deposit needs a memo, and a payment request carries none.
    NeedsMemo,
    /// The payee no longer declares the payout the quote was asked for.
    PayoutGone,
    /// A payment this payer sent and nobody has confirmed covers the debt
    /// (§14.4); `paid_to` must confirm it.
    Held { paid_to: Vec<String> },
    /// The bill no longer says this payer owes the quoted amount.
    NotOwed,
    /// The payee's payout no longer names the quote's `recipient`.
    RecipientChanged,
    /// The payee's payout no longer names the asset and chain the quote buys.
    AssetChanged,
    /// The bill's rate no longer converts the debt to the quote's ZEC.
    RateChanged,
}

#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct SwapStatus {
    pub state: SwapState,
    /// On the destination chain. MUST NOT be recorded as the §10.5 payment.
    pub destination_tx_hash: Option<String>,
    pub detail: Option<String>,
}

/// One expense to write again without the person, withdrawing `entry_id`
/// (§10.8).
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct RemovalEdit {
    /// The `addExpense` entry the restated expense replaces.
    pub entry_id: String,
    /// The expense as the plan read it: payer, amount and description of
    /// what is written again.
    pub seen: Expense,
    /// Who wrote the expense being withdrawn. The expense written in its
    /// place is the restating device's (§10.4).
    pub author: Option<String>,
    /// `seen`'s split without the person, as JSON.
    pub split_json: String,
    /// The amendment applied to `entry_id` when the plan read it, or none.
    /// The restatement names it, and is set aside when the expense has been
    /// corrected since (§10.8).
    pub basis: Option<String>,
    /// The payer written in place of `seen`'s, or none to keep it. Only a
    /// merge changes the payer.
    pub paid_by: Option<String>,
}

/// Why an entry still names somebody once a plan's edits are written.
#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemovalBlock {
    /// An expense naming them that the fold does not apply.
    Unapplied,
    /// They paid for the expense.
    PaidFor,
    /// Written by somebody else, on a bill this device did not open;
    /// `author` can take them out of it.
    AddedByAnother,
    /// Taking them out of the split needs a choice only a person can make.
    SplitByHand,
    /// A payment from or to them is on the bill.
    Payment,
    /// They confirmed a payment.
    Confirmation,
}

/// One entry that still names somebody once a plan's edits are written.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct RemovalBlocker {
    pub block: RemovalBlock,
    pub entry_id: String,
    /// The expense's description; empty when it has none, and for a payment
    /// or a confirmation.
    pub description: String,
    /// Who wrote the expense, for `AddedByAnother`.
    pub author: Option<String>,
    /// For `Payment`: whether they are its payer rather than its payee.
    pub from_them: bool,
}

/// Whether a removal plan a person agreed to is still what would be written
/// (§10.8).
#[derive(uniffi::Enum, Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemovalPlanStanding {
    /// It writes exactly what was agreed to and is held back by the same
    /// entries.
    Stands,
    /// The bill moved: plan again and ask again before writing anything.
    Changed,
}

/// What taking somebody off a bill needs, as one device sees it (§10.8).
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct RemovalPlan {
    /// Expenses this device can take them out of.
    pub edits: Vec<RemovalEdit>,
    /// What still names them once `edits` are written. Empty with `edits`
    /// when nothing names them, and the `voidEntry` of their joins applies.
    pub blockers: Vec<RemovalBlocker>,
    /// Every `joinBill` still stating them, in log order: write a `voidEntry`
    /// for each. One left standing keeps them on the bill.
    pub joins: Vec<String>,
    /// Whether the device planning may withdraw `joins`: only the bill's
    /// creator or the person themselves may (§10.8).
    pub may_withdraw_joins: bool,
    /// Whether writing `edits` and withdrawing `joins` takes them off the
    /// bill: `blockers` is empty and `may_withdraw_joins` holds. Offer the
    /// `edits` only when this holds (§10.8). Not read back by
    /// `same_removal_plan`.
    pub complete: bool,
}

/// How much more one participant owes once a plan's edits are written, in
/// the bill's minor units (`removal_share_changes`).
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct ShareChange {
    pub participant_id: String,
    /// Positive for taking on a share; minus their share for the person
    /// taken out.
    pub minor_units: i64,
}
