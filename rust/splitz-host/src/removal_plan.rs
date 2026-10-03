//! What taking somebody off a bill needs first, and what of it one device can
//! do (§10.3, §10.8).
//!
//! A `voidEntry` of somebody's `joinBill` is refused with
//! `participant_still_named` while any surviving entry names them, and a
//! refused withdrawal is still written and synced. [`plan_removal`] answers
//! the question before anything is written: which expenses this device can
//! take them out of, and what else still names them.

use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet, HashMap};

use splitz_core::host::FoldedBill;
use splitz_core::{payload_for, Expense};

/// One expense to write again without the person, withdrawing `entry_id`.
#[derive(Debug, Clone, PartialEq)]
pub struct RemovalEdit {
    /// The `addExpense` entry the restated expense replaces.
    pub entry_id: String,
    /// The expense as the plan read it. What is written again is this, under
    /// `split`: payer, amount and description come from the same reading.
    pub seen: Expense,
    /// Who wrote the expense being withdrawn. The expense written in its
    /// place is the restating device's, and only its author may correct an
    /// expense (§10.4).
    pub author: Option<String>,
    /// `seen`'s split without the person, the others sharing what was theirs.
    pub split: Value,
}

/// Why an entry still names somebody once a plan's edits are written.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemovalBlock {
    /// An expense naming them that the fold does not apply. Its description
    /// is the one written in the entry.
    Unapplied,
    /// They paid for the expense: nobody else can be its payer.
    PaidFor,
    /// The expense was written by somebody other than this device, and this
    /// device did not open the bill (§10.8). [`RemovalBlocker::author`] can.
    AddedByAnother,
    /// Taking them out of the split needs a choice only a person can make.
    SplitByHand,
    /// A payment from or to them is on the bill.
    Payment,
    /// They confirmed a payment.
    Confirmation,
}

/// One entry that still names somebody once a plan's edits are written.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemovalBlocker {
    pub block: RemovalBlock,
    /// The entry that names them.
    pub entry_id: String,
    /// The expense's description, empty when it has none, and empty for a
    /// payment or a confirmation.
    pub description: String,
    /// Who wrote the expense, for [`RemovalBlock::AddedByAnother`]; `None`
    /// when the fold names no author.
    pub author: Option<String>,
    /// For [`RemovalBlock::Payment`]: whether they are its payer. False when
    /// they are only its payee.
    pub from_them: bool,
}

impl RemovalBlocker {
    fn new(block: RemovalBlock, entry_id: &str) -> Self {
        Self {
            block,
            entry_id: entry_id.to_owned(),
            description: String::new(),
            author: None,
            from_them: false,
        }
    }
}

/// What taking somebody off a bill needs, as one device sees it.
#[derive(Debug, Clone, PartialEq)]
pub struct RemovalPlan {
    /// Expenses this device can take them out of.
    pub edits: Vec<RemovalEdit>,
    /// What still names them once `edits` are written.
    pub blockers: Vec<RemovalBlocker>,
    /// Every `joinBill` still stating them, in log order: the entries a
    /// `voidEntry` must withdraw, all of them, to take them off. Each change
    /// to how somebody is paid restates their record in another join, and one
    /// left standing keeps them on the bill.
    pub joins: Vec<String>,
}

impl RemovalPlan {
    /// Whether any entry still in force names them.
    pub fn names_them(&self) -> bool {
        !self.edits.is_empty() || !self.blockers.is_empty()
    }

    /// Whether `other` writes exactly what this does and is held back by the
    /// same things: what a person confirmed is still what would be written.
    ///
    /// An edit is compared by its entry, the expense's id, payer, amount,
    /// description and split, and the split it would write; the instant and
    /// currency of the reading are not part of what is written again.
    pub fn same_as(&self, other: &RemovalPlan) -> bool {
        let edit = |e: &RemovalEdit| {
            (
                e.entry_id.clone(),
                e.seen.id.clone(),
                e.seen.paid_by.clone(),
                e.seen.amount,
                e.seen.description.clone(),
                e.seen.split.clone(),
                e.split.clone(),
            )
        };
        self.edits.len() == other.edits.len()
            && self
                .edits
                .iter()
                .zip(&other.edits)
                .all(|(a, b)| edit(a) == edit(b))
            && self.blockers == other.blockers
            && self.joins == other.joins
    }
}

/// `split` without `id` in it, the others sharing what was theirs, or `None`
/// when that leaves nobody or needs a choice only a person can make: exact
/// amounts and percentages must still add up, an item they alone had
/// belongs to nobody else, and shares that leave nobody a share divide
/// nothing (§4.4).
pub fn split_without(split: &Value, id: &str) -> Option<Value> {
    let drop = |ids: Option<&Value>| -> Vec<Value> {
        ids.and_then(Value::as_array)
            .map(|ids| {
                ids.iter()
                    .filter(|x| x.as_str() != Some(id))
                    .cloned()
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut out = split.as_object().cloned().unwrap_or_default();
    match split.get("type").and_then(Value::as_str) {
        Some("equal") => {
            let among = drop(split.get("among"));
            if among.is_empty() {
                return None;
            }
            out.insert("among".into(), Value::Array(among));
        }
        Some("shares") => {
            let mut counts = split
                .get("shareCounts")
                .and_then(Value::as_object)
                .cloned()
                .unwrap_or_default();
            counts.remove(id);
            // A count that is not an integer counts for nothing; the sum wraps
            // at 64 bits, which a split the fold applied never reaches (§4.4).
            let total = counts
                .values()
                .fold(0i64, |sum, n| sum.wrapping_add(n.as_i64().unwrap_or(0)));
            if total <= 0 {
                return None;
            }
            out.insert("shareCounts".into(), Value::Object(counts));
        }
        Some("itemized") => {
            let mut items = Vec::new();
            let held = split.get("items").and_then(Value::as_array);
            for raw in held.into_iter().flatten() {
                let mut item: Map<String, Value> = raw.as_object().cloned().unwrap_or_default();
                let had = item.get("sharedBy").and_then(Value::as_array);
                let named = had.is_some_and(|h| h.iter().any(|x| x.as_str() == Some(id)));
                let left = drop(item.get("sharedBy"));
                if left.is_empty() && named {
                    return None;
                }
                item.insert("sharedBy".into(), Value::Array(left));
                items.push(Value::Object(item));
            }
            out.insert("items".into(), Value::Array(items));
        }
        _ => return None,
    }
    Some(Value::Object(out))
}

/// Whether a decoded split names `id` under the member its `type` reads.
fn names(split: &Value, id: &str) -> bool {
    let listed = |ids: Option<&Value>| {
        ids.and_then(Value::as_array)
            .is_some_and(|ids| ids.iter().any(|x| x.as_str() == Some(id)))
    };
    let keyed = |figures: Option<&Value>| {
        figures
            .and_then(Value::as_object)
            .is_some_and(|f| f.contains_key(id))
    };
    match split.get("type").and_then(Value::as_str) {
        Some("equal") => listed(split.get("among")),
        Some("exact") => keyed(split.get("amounts")),
        Some("percentage") => keyed(split.get("basisPoints")),
        Some("shares") => keyed(split.get("shareCounts")),
        Some("itemized") => split
            .get("items")
            .and_then(Value::as_array)
            .is_some_and(|items| {
                items
                    .iter()
                    .any(|item| item.is_object() && listed(item.get("sharedBy")))
            }),
        _ => false,
    }
}

static EMPTY: std::sync::LazyLock<Map<String, Value>> = std::sync::LazyLock::new(Map::new);

/// `value` when it is an object, otherwise an empty one.
fn object(value: Option<&Value>) -> &Map<String, Value> {
    value.and_then(Value::as_object).unwrap_or(&EMPTY)
}

/// `member` of `value`, with a JSON `null` read as absent.
fn member<'a>(value: &'a Map<String, Value>, name: &str) -> Option<&'a Value> {
    value.get(name).filter(|v| !v.is_null())
}

/// Whether `value`'s `name` is the string `id`.
fn is(value: &Map<String, Value>, name: &str, id: &str) -> bool {
    value.get(name).and_then(Value::as_str) == Some(id)
}

/// Whether an expense payload as a peer wrote it names `id`, read the way
/// §10.8's check reads it: as payer, or under any member of its split,
/// whatever its `type` says.
fn expense_names(expense: &Map<String, Value>, id: &str) -> bool {
    if is(expense, "paidBy", id) {
        return true;
    }
    let split = object(expense.get("split"));
    let list = |v: Option<&Value>| -> Vec<Value> {
        v.and_then(Value::as_array).cloned().unwrap_or_default()
    };
    let keyed = |v: Option<&Value>| {
        v.and_then(Value::as_object)
            .is_some_and(|m| m.contains_key(id))
    };
    let contains = |v: &[Value]| v.iter().any(|x| x.as_str() == Some(id));
    contains(&list(split.get("among")))
        || keyed(split.get("amounts"))
        || keyed(split.get("basisPoints"))
        || keyed(split.get("shareCounts"))
        || list(split.get("items"))
            .iter()
            .any(|item| item.is_object() && contains(&list(item.get("sharedBy"))))
}

/// What taking `id` off a bill needs, as seen from `me` (§10.8).
///
/// `folded` is `log` folded, and `creator_id` the author of the bill's
/// create. `log` is read in the order given. An entry named by
/// `folded.withdrawn` names nobody.
///
/// §10.8 counts somebody as named by every entry still in force — an expense
/// or payment the fold set aside included — and by an amended entry when
/// either the amendment or the entry it corrects names them, so both are read
/// here; reading the folded bill alone tells a person somebody edited out of
/// an expense is on nothing, and the removal is then refused.
///
/// An expense becomes an edit when the fold applies it, they did not pay for
/// it, `me` wrote it or opened the bill, and [`split_without`] can take them
/// out of it. Every other entry naming them is a [`RemovalBlocker`], in log
/// order. One reading per entry id: §10.2's union keeps copies of an id under
/// different signatures, and an expense restated once per copy would be on
/// the bill twice.
pub fn plan_removal(
    folded: &FoldedBill,
    creator_id: &str,
    log: &[Value],
    id: &str,
    me: &str,
) -> RemovalPlan {
    let gone: BTreeSet<&str> = folded.withdrawn.iter().map(String::as_str).collect();
    let mut by_id: HashMap<&str, &Map<String, Value>> = HashMap::new();
    for e in log {
        if let Some(entry_id) = e.get("id").and_then(Value::as_str) {
            by_id.insert(entry_id, object(Some(e)));
        }
    }

    // The amendment §10.4 applies to each entry: the last in log order whose
    // author wrote its target, carrying the target's kind and subject — and
    // none when that one is withdrawn, since an earlier one does not stand in
    // for it (§10.8).
    let mut amended: BTreeMap<&str, &Map<String, Value>> = BTreeMap::new();
    for e in log.iter().map(|e| object(Some(e))) {
        if !is(e, "kind", "amendEntry") {
            continue;
        }
        let Some(target_id) = e.get("targetId").and_then(Value::as_str) else {
            continue;
        };
        let Some(target) = by_id.get(target_id) else {
            continue;
        };
        if member(e, "author") != member(target, "author") {
            continue;
        }
        let Some(name) = target
            .get("kind")
            .and_then(Value::as_str)
            .and_then(payload_for)
        else {
            continue;
        };
        if !e.get(name).is_some_and(Value::is_object) {
            continue;
        }
        if member(object(e.get(name)), "id") != member(object(target.get(name)), "id") {
            continue;
        }
        amended.insert(target_id, e);
    }
    amended.retain(|_, e| {
        !e.get("id")
            .and_then(Value::as_str)
            .is_some_and(|a| gone.contains(a))
    });
    let readings = |e: &Map<String, Value>, name: &str, entry_id: &str| {
        let corrected = amended.get(entry_id).and_then(|a| a.get(name));
        [object(e.get(name)).clone(), object(corrected).clone()]
    };

    let mut edits = Vec::new();
    let mut blockers = Vec::new();
    let mut joins = Vec::new();
    let mut read: BTreeSet<&str> = BTreeSet::new();
    for entry in log.iter().map(|e| object(Some(e))) {
        let Some(entry_id) = entry.get("id").and_then(Value::as_str) else {
            continue;
        };
        if gone.contains(entry_id) || !read.insert(entry_id) {
            continue;
        }
        match entry.get("kind").and_then(Value::as_str) {
            Some("addExpense") => {
                if !readings(entry, "expense", entry_id)
                    .iter()
                    .any(|x| expense_names(x, id))
                {
                    continue;
                }
                let Some(e) = folded.bill.expenses.iter().find(|x| {
                    folded.expense_entries.get(&x.id).map(String::as_str) == Some(entry_id)
                }) else {
                    let written = object(entry.get("expense"))
                        .get("description")
                        .and_then(Value::as_str)
                        .unwrap_or("");
                    blockers.push(RemovalBlocker {
                        description: written.to_owned(),
                        ..RemovalBlocker::new(RemovalBlock::Unapplied, entry_id)
                    });
                    continue;
                };
                if e.paid_by == id {
                    blockers.push(RemovalBlocker {
                        description: e.description.clone(),
                        ..RemovalBlocker::new(RemovalBlock::PaidFor, entry_id)
                    });
                    continue;
                }
                let author = folded.expense_authors.get(&e.id).cloned();
                // §10.8: an expense's author or the bill's creator may
                // withdraw it.
                if author.as_deref() != Some(me) && me != creator_id {
                    blockers.push(RemovalBlocker {
                        description: e.description.clone(),
                        author,
                        ..RemovalBlocker::new(RemovalBlock::AddedByAnother, entry_id)
                    });
                    continue;
                }
                // Named only by the entry it corrects: written again as it
                // reads now.
                let split = if names(&e.split, id) {
                    split_without(&e.split, id)
                } else {
                    Some(e.split.clone())
                };
                let Some(split) = split else {
                    blockers.push(RemovalBlocker {
                        description: e.description.clone(),
                        ..RemovalBlocker::new(RemovalBlock::SplitByHand, entry_id)
                    });
                    continue;
                };
                edits.push(RemovalEdit {
                    entry_id: entry_id.to_owned(),
                    seen: e.clone(),
                    author,
                    split,
                });
            }
            Some("recordPayment") => {
                let payment = readings(entry, "payment", entry_id);
                let payer = payment.iter().any(|x| is(x, "from", id));
                let payee = payment.iter().any(|x| is(x, "to", id));
                if payer || payee {
                    blockers.push(RemovalBlocker {
                        from_them: payer,
                        ..RemovalBlocker::new(RemovalBlock::Payment, entry_id)
                    });
                }
            }
            Some("confirmPayment") if is(entry, "author", id) => {
                blockers.push(RemovalBlocker::new(RemovalBlock::Confirmation, entry_id));
            }
            Some("joinBill") if is(object(entry.get("participant")), "id", id) => {
                joins.push(entry_id.to_owned());
            }
            _ => {}
        }
    }
    RemovalPlan {
        edits,
        blockers,
        joins,
    }
}
