//! The log, read as a history a person can follow.
//!
//! A folded bill says what is true now; it does not say what happened. The log
//! does, and it is the only place three things are visible at all: an entry
//! somebody withdrew, an entry the fold refused and its code, and a payment
//! that has been claimed but not confirmed.
//!
//! Deriving this is protocol work rather than wallet work — it reads entries
//! and a folded bill and nothing else — so it lives here and every wallet gets
//! the same history.

use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

use serde_json::Value;
use splitz_core::host::FoldedBill;
use splitz_core::{Bill, PaymentRecord, SetAside};

/// What one entry did.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
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
    /// The creator closed the bill for settling (§10.9). A withdrawal of it,
    /// which reopens the bill, is an `EntryWithdrawn` naming it.
    ClosedForSettling,
    /// An entry kind this reader does not name. Shown rather than hidden: an
    /// entry that vanished silently is indistinguishable from one that was
    /// never sent.
    Other,
}

/// One line of a bill's history.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BillEvent {
    pub entry_id: String,
    pub kind: BillEventKind,
    /// Who wrote the entry. §10.7 binds this to a key; it is not a display
    /// name.
    pub author: String,
    /// §9.3's canonical instant. Fixed width, so lexicographic order is
    /// chronological order and a screen sorts on the string.
    pub at: String,
    /// Who or what the entry is about, where that differs from `author` — the
    /// payee of a payment, the participant a vouch names, the payment a
    /// confirmation confirms, and the entry an amendment or a withdrawal
    /// targets, by its entry id.
    pub subject: Option<String>,
    /// Minor units of the bill's currency (§2.1).
    pub amount_minor_units: Option<i64>,
    pub description: Option<String>,
    /// `shieldedZec`, `swap` or `cash` for a payment (§9.2).
    pub method: Option<String>,
    /// A swap's own identifier. **Not a Zcash txid** — a screen that renders
    /// it as one is wrong for every swap (§9.2).
    pub reference: Option<String>,
    /// Whether §10.8 withdrew this entry. It stays in the history: removing it
    /// would leave a reader unable to see that it was ever written.
    pub withdrawn: bool,
    /// Set when the fold would not apply this entry (§10.3).
    ///
    /// The §12 code, which is what a wallet turns into a sentence for its user
    /// (§1). This crate writes no such sentence.
    pub refused_code: Option<String>,
    /// For [`BillEventKind::PaymentRecorded`]: whether §10.5 has settled it.
    ///
    /// **A recorded payment is a claim.** Presenting an unconfirmed one as
    /// settled tells a payer a debt is discharged that the payee has never
    /// agreed was paid.
    pub confirmed: bool,
    /// For a restatement (§10.8) — the creator rewriting somebody else's
    /// expense while taking a person off — the participant it takes off.
    pub taken_off: Option<String>,
    /// For a restatement, the one participant who takes over `taken_off`'s
    /// part — paying in their place, or holding their share — when exactly
    /// one does: a merge (§14.11). None when the part is spread over the rest.
    pub moved_to: Option<String>,
}

impl BillEvent {
    /// Whether this entry took effect at all.
    pub fn applied(&self) -> bool {
        !self.withdrawn && self.refused_code.is_none()
    }
}

/// Reads `entries` as a history, newest first.
///
/// `bill`, `set_aside` and `withdrawn` supply what the log alone cannot: which
/// entries the fold set aside, which were withdrawn, and which payments are
/// confirmed.
pub fn activity_of(
    entries: &[Value],
    bill: &Bill,
    set_aside: &[SetAside],
    withdrawn: &[String],
) -> Vec<BillEvent> {
    let gone: HashSet<&str> = withdrawn.iter().map(String::as_str).collect();

    // A join written again is how somebody amends their own record, and the
    // one amendment that moves money is a new address (§13). Telling the two
    // apart needs the order the entries arrived in, so it is decided here
    // rather than inside `event`.
    let mut joined: BTreeSet<String> = BTreeSet::new();
    let mut events = Vec::with_capacity(entries.len());
    // One line per entry, not per copy: section 10.2's union keeps copies of
    // an id under different signatures, and section 9.5 makes them agree in
    // every member a line shows. A copy read twice doubles an expense and
    // turns a join into a changed address.
    let mut seen: HashSet<&str> = HashSet::new();
    let mut by_id: HashMap<&str, &Value> = HashMap::new();
    for entry in entries {
        if let Some(id) = entry.get("id").and_then(Value::as_str) {
            by_id.entry(id).or_insert(entry);
        }
    }
    for entry in entries {
        if !seen.insert(entry.get("id").and_then(Value::as_str).unwrap_or_default()) {
            continue;
        }
        let mut rejoined = false;
        if entry.get("kind").and_then(Value::as_str) == Some("joinBill") {
            let participant = entry.get("participant");
            let id = participant
                .and_then(|p| p.get("id"))
                .and_then(Value::as_str);
            let carries_address = participant
                .and_then(|p| p.get("payTo"))
                .is_some_and(|a| !a.is_null());
            if let Some(id) = id {
                rejoined = carries_address && !joined.insert(id.to_owned());
            }
        }
        events.push(event(
            entry,
            set_aside,
            &gone,
            &bill.confirmed_payments,
            &by_id,
            rejoined,
        ));
    }
    // Newest first, and no second ordering rule: §10.2 already fixes the order
    // of a log, `entries` arrives in it, and every device agrees on it.
    // Sorting again here — on the instant, with some tie-break of this file's
    // own — would be a second answer to a question the protocol has settled,
    // and two devices could disagree about a history they hold identically.
    events.reverse();
    events
}

fn text(entry: &Value, key: &str) -> Option<String> {
    entry.get(key).and_then(Value::as_str).map(str::to_owned)
}

fn member(entry: &Value, key: &str, name: &str) -> Option<String> {
    entry
        .get(key)
        .filter(|v| v.is_object())
        .and_then(|o| o.get(name))
        .and_then(Value::as_str)
        .map(str::to_owned)
}

fn number(entry: &Value, key: &str, name: &str) -> Option<i64> {
    entry
        .get(key)
        .filter(|v| v.is_object())
        .and_then(|o| o.get(name))
        .and_then(Value::as_i64)
}

/// Who a restatement takes off `before`, and the one participant who takes
/// over their part, or none for either when the two do not say.
///
/// Taken off: the one id `before` names and `after` does not. Taken over by:
/// the one other id whose part differs — paying in their place, named where
/// they were, or holding a larger figure.
fn moved(before: &Value, after: &Value) -> Option<(String, Option<String>)> {
    let was = parts(before);
    let now = parts(after);
    let gone: Vec<&String> = was.keys().filter(|id| !now.contains_key(*id)).collect();
    let [gone] = gone.as_slice() else {
        return None;
    };
    let changed: Vec<&String> = now
        .iter()
        .filter(|(id, part)| *id != *gone && was.get(*id) != Some(part))
        .map(|(id, _)| id)
        .collect();
    let to = match changed.as_slice() {
        [one] => Some((*one).clone()),
        _ => None,
    };
    Some(((*gone).clone(), to))
}

/// Each id `expense` names, with what it names them for: paying, and their
/// place in every list and figure of the split.
fn parts(expense: &Value) -> BTreeMap<String, String> {
    let mut parts: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut add = |id: Option<&str>, part: String| {
        if let Some(id) = id {
            parts.entry(id.to_owned()).or_default().push(part);
        }
    };
    add(
        expense.get("paidBy").and_then(Value::as_str),
        "paidBy".into(),
    );
    if let Some(split) = expense.get("split").filter(|v| v.is_object()) {
        if let Some(among) = split.get("among").and_then(Value::as_array) {
            for id in among {
                add(id.as_str(), "among".into());
            }
        }
        for member in ["amounts", "basisPoints", "shareCounts"] {
            if let Some(figures) = split.get(member).and_then(Value::as_object) {
                for (id, value) in figures {
                    add(Some(id), format!("{member}={value}"));
                }
            }
        }
        if let Some(items) = split.get("items").and_then(Value::as_array) {
            for (i, item) in items.iter().enumerate() {
                if let Some(shared_by) = item.get("sharedBy").and_then(Value::as_array) {
                    for id in shared_by {
                        add(id.as_str(), format!("item{i}"));
                    }
                }
            }
        }
    }
    parts
        .into_iter()
        .map(|(id, mut p)| {
            p.sort();
            (id, p.join(","))
        })
        .collect()
}

fn event(
    entry: &Value,
    set_aside: &[SetAside],
    withdrawn: &HashSet<&str>,
    confirmed: &BTreeSet<String>,
    by_id: &HashMap<&str, &Value>,
    rejoined: bool,
) -> BillEvent {
    let id = text(entry, "id").unwrap_or_default();
    let refused = set_aside
        .iter()
        .find(|s| s.id == id)
        .map(|s| s.code.to_owned());

    let mut built = BillEvent {
        entry_id: id.clone(),
        kind: BillEventKind::Other,
        author: text(entry, "author").unwrap_or_default(),
        at: text(entry, "at").unwrap_or_default(),
        subject: None,
        amount_minor_units: None,
        description: None,
        method: None,
        reference: None,
        withdrawn: withdrawn.contains(id.as_str()),
        refused_code: refused,
        confirmed: false,
        taken_off: None,
        moved_to: None,
    };

    match entry.get("kind").and_then(Value::as_str) {
        Some("createBill") => {
            built.kind = BillEventKind::Opened;
            built.description = member(entry, "bill", "name");
        }
        Some("joinBill") => {
            // §13 requires a payer be shown a changed address before settling
            // to it, so it is its own event rather than a second "joined".
            built.kind = if rejoined {
                BillEventKind::AddressChanged
            } else {
                BillEventKind::Joined
            };
            built.subject = member(entry, "participant", "id");
            built.description = member(entry, "participant", "name");
        }
        // A restatement names the entry it replaces (§10.8): a correction of
        // that expense, not a second one.
        Some("addExpense") if entry.get("targetId").is_some_and(Value::is_string) => {
            let restates = text(entry, "targetId");
            built.kind = BillEventKind::ExpenseAmended;
            built.amount_minor_units = number(entry, "expense", "amount");
            built.description = member(entry, "expense", "description");
            let before = restates
                .as_deref()
                .and_then(|t| by_id.get(t))
                .and_then(|e| e.get("expense"))
                .filter(|v| v.is_object());
            if let (Some(before), Some(after)) =
                (before, entry.get("expense").filter(|v| v.is_object()))
            {
                if let Some((off, to)) = moved(before, after) {
                    built.taken_off = Some(off);
                    built.moved_to = to;
                }
            }
            built.subject = restates;
        }
        Some("addExpense") => {
            built.kind = BillEventKind::ExpenseAdded;
            built.subject = member(entry, "expense", "paidBy");
            built.amount_minor_units = number(entry, "expense", "amount");
            built.description = member(entry, "expense", "description");
        }
        // Both name the entry they act on by its id, at the top level (§10.4,
        // §10.8): a reader without it can say something was changed or
        // withdrawn, and not what.
        Some("amendEntry") => {
            built.kind = BillEventKind::ExpenseAmended;
            built.subject = text(entry, "targetId");
            built.amount_minor_units = number(entry, "expense", "amount");
            built.description = member(entry, "expense", "description");
        }
        Some("voidEntry") => {
            built.kind = BillEventKind::EntryWithdrawn;
            built.subject = text(entry, "targetId");
        }
        Some("recordPayment") => {
            built.kind = BillEventKind::PaymentRecorded;
            built.subject = member(entry, "payment", "to");
            built.amount_minor_units = number(entry, "payment", "amount");
            built.method = member(entry, "payment", "method");
            built.reference = member(entry, "payment", "reference");
            built.confirmed =
                member(entry, "payment", "id").is_some_and(|id| confirmed.contains(&id));
        }
        Some("confirmPayment") => {
            built.kind = BillEventKind::PaymentConfirmed;
            built.subject = member(entry, "confirmation", "paymentId");
            built.method = member(entry, "confirmation", "method");
            built.reference = member(entry, "confirmation", "reference");
        }
        Some("setRate") => {
            built.kind = BillEventKind::Priced;
            built.amount_minor_units = number(entry, "rate", "minorUnitsPerZec");
            built.description = member(entry, "rate", "source");
        }
        Some("closeBill") => {
            built.kind = BillEventKind::ClosedForSettling;
        }
        _ => {}
    }
    built
}

/// Who this device writes a confirmation of `payment` as, or none when it
/// may not confirm it (§10.5, §14.11).
///
/// The payee confirms their own payment. The creator also confirms one paid
/// to somebody they added by hand — a participant who states no key and has
/// bound none (§10.7) — written as that person and unsigned, as their join
/// was. Nobody else on the bill can, so without this a payment to somebody
/// who never joins from a device of their own, or who joined under a key of
/// their own and so under another id, never settles.
///
/// Never the payer: a creator who paid somebody they added would otherwise
/// settle the debt by asserting twice that they paid it.
pub fn confirmer_for(folded: &FoldedBill, payment: &PaymentRecord, me: &str) -> Option<String> {
    if payment.to == me {
        return Some(me.to_owned());
    }
    if me != folded.creator_id || payment.from == me {
        return None;
    }
    let payee = folded.bill.participant(&payment.to)?;
    if payee.identity_key.is_some() || folded.identities.bound.contains_key(&payment.to) {
        return None;
    }
    Some(payment.to.clone())
}

/// The payments this device may confirm, newest first: those
/// [`confirmer_for`] names a confirmer for, not yet confirmed.
pub fn awaiting_confirmation_for(folded: &FoldedBill, me: &str) -> Vec<PaymentRecord> {
    let mut mine: Vec<PaymentRecord> = folded
        .bill
        .payments
        .iter()
        .filter(|p| {
            !folded.bill.confirmed_payments.contains(&p.id)
                && confirmer_for(folded, p, me).is_some()
        })
        .cloned()
        .collect();
    mine.sort_by(|a, b| b.at.cmp(&a.at));
    mine
}

/// The payments this device may confirm, newest first (§10.5).
///
/// **Only the payee confirms.** A payer who could confirm their own payment
/// would settle a debt by asserting twice that they paid it, which is the one
/// thing a confirmation exists to prevent.
pub fn awaiting_confirmation_by(bill: &Bill, me: &str) -> Vec<PaymentRecord> {
    let mut mine: Vec<PaymentRecord> = bill
        .payments
        .iter()
        .filter(|p| p.to == me && !bill.confirmed_payments.contains(&p.id))
        .cloned()
        .collect();
    mine.sort_by(|a, b| b.at.cmp(&a.at));
    mine
}
