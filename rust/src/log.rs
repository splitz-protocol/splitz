//! The log (SPEC.md §10).
//!
//! A bill is materialised from an append-only log. Entries are never modified:
//! correcting an expense appends an amendment naming the one it replaces, and
//! removing it appends a withdrawal. There is no state to conflict, only facts
//! to union.

use serde_json::{Map, Value};
use std::collections::{BTreeMap, BTreeSet};

use crate::authority::Identities;
use crate::canonical_json::canonical_json;
use crate::error::{code, Result, SplitError};
use crate::instant::canonical_instant;
use crate::money::check_currency;
use crate::sha256::sha256;
use crate::zip321::base64url;

pub const ENTRY_KINDS: [&str; 9] = [
    "createBill",
    "joinBill",
    "addExpense",
    "amendEntry",
    "voidEntry",
    "recordPayment",
    "confirmPayment",
    "vouchIdentity",
    "setRate",
];

const PAYLOAD_NAMES: [&str; 5] = ["rate", "expense", "payment", "confirmation", "vouch"];

/// The domain separator the bill id digest covers.
pub const BILL_ID_DOMAIN: &str = "splitz-bill-id-v1";

/// The domain separator an entry id's digest covers (§9.5).
///
/// Different from §9.4's so that a `createBill` id and any other entry's id
/// are drawn from different spaces and neither can be presented as the other.
pub const ENTRY_ID_DOMAIN: &str = "splitz-entry-id-v1";

/// The payload each kind carries, and no other.
pub fn payload_for(kind: &str) -> Option<&'static str> {
    match kind {
        "joinBill" => Some("participant"),
        "addExpense" => Some("expense"),
        "recordPayment" => Some("payment"),
        "confirmPayment" => Some("confirmation"),
        "vouchIdentity" => Some("vouch"),
        "setRate" => Some("rate"),
        _ => None,
    }
}

/// Who a confirmation method speaks for, whether it needs a reference, and
/// whether it settles a debt (§10.5).
pub fn confirmation_rule(method: &str) -> Option<(Option<&'static str>, bool, bool)> {
    match method {
        "recipientConfirmed" => Some((Some("to"), false, true)),
        "walletReceived" => Some((Some("to"), false, true)),
        // Names a public transaction any participant can check, so it speaks
        // for nobody in particular.
        "onChain" => Some((None, true, true)),
        // A payer saying they paid is the claim of the record, not evidence.
        "payerAttested" => Some((Some("from"), false, false)),
        _ => None,
    }
}

fn is_b64url_of_length(value: Option<&str>, bytes: usize) -> bool {
    let Some(text) = value else { return false };
    if text.is_empty()
        || !text
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
    {
        return false;
    }
    // Four base64url characters carry three bytes; the tail carries one or two.
    let full = text.len() / 4 * 3;
    let decoded = match text.len() % 4 {
        0 => full,
        2 => full + 1,
        3 => full + 2,
        _ => return false,
    };
    decoded == bytes
}

/// The bill id derived from the entry that opens a bill (§9.4).
///
/// `id`, `sig` and `v` are excluded: the first is the output, the second
/// covers the first, and the third is restated by whichever reader re-encodes
/// the entry.
pub fn derive_bill_id(entry: &Value) -> Result<String> {
    derive_id(BILL_ID_DOMAIN, entry)
}

/// The id of `entry` (§9.5): the digest of the entry with `id`, `sig` and `v`
/// removed.
pub fn derive_entry_id(entry: &Value) -> Result<String> {
    derive_id(ENTRY_ID_DOMAIN, entry)
}

fn derive_id(domain: &str, entry: &Value) -> Result<String> {
    let mut body = Map::new();
    if let Some(obj) = entry.as_object() {
        for (k, v) in obj {
            if k != "id" && k != "sig" && k != "v" {
                body.insert(k.clone(), v.clone());
            }
        }
    }
    let message = format!("{domain}{}", canonical_json(&Value::Object(body))?);
    Ok(base64url(&sha256(message.as_bytes())[..16]))
}

/// Checks an entry before it reaches a log (§10.1).
///
/// An entry carrying more than one payload is refused because the currency
/// fallback and the fold would otherwise read different ones. One carrying
/// none is refused because removing a member makes an entry's canonical
/// encoding sort higher than the same entry with it, so the merge would keep
/// the stripped copy.
/// The members of a payload that name a participant or an entry (§10.1).
fn id_members_of(wanted: &str) -> &'static [&'static str] {
    match wanted {
        "participant" => &["id"],
        "expense" => &["id", "paidBy"],
        "payment" => &["id", "from", "to"],
        "confirmation" => &["paymentId"],
        "vouch" => &["subject"],
        _ => &[],
    }
}

pub fn check_entry(entry: &Value) -> Result<()> {
    if !entry.is_object() {
        return Err(SplitError::new(
            code::BILL_TYPE_ERROR,
            "An entry is an object",
        ));
    }

    let kind = entry.get("kind").and_then(Value::as_str);
    let kind = match kind {
        Some(k) if ENTRY_KINDS.contains(&k) => k,
        other => {
            return Err(SplitError::new(
                code::BILL_UNKNOWN_ENTRY_KIND,
                format!("No such entry kind: {other:?}"),
            ))
        }
    };

    let carried: Vec<&str> = PAYLOAD_NAMES
        .iter()
        .copied()
        .filter(|p| entry.get(*p).is_some())
        .collect();
    if carried.len() > 1 {
        return Err(SplitError::new(
            code::BILL_AMBIGUOUS_ENTRY,
            format!("An entry carries {}", carried.join(" and ")),
        ));
    }

    if let Some(wanted) = payload_for(kind) {
        let Some(payload) = entry.get(wanted) else {
            return Err(SplitError::new(
                code::BILL_MISSING_ENTRY_PAYLOAD,
                format!("A {kind} carries a {wanted}"),
            ));
        };
        // §10.1. Every later pass indexes the payload without re-checking it,
        // and the fold's authorisation pass reads the target's payload to
        // decide who may withdraw an entry — so a scalar payload admitted
        // here makes the entry unwithdrawable and the bill unopenable.
        let Some(payload) = payload.as_object() else {
            return Err(SplitError::new(
                code::BILL_TYPE_ERROR,
                format!("A {wanted} is an object"),
            ));
        };
        for member in id_members_of(wanted) {
            if let Some(value) = payload.get(*member) {
                if !value.is_null() && !value.is_string() {
                    return Err(SplitError::new(
                        code::BILL_TYPE_ERROR,
                        format!("A {wanted} states {member} as a string"),
                    ));
                }
            }
        }
        if let Some(split) = payload.get("split") {
            crate::split::check_id_lists(split)?;
        }
    }
    if let Some(target) = entry.get("targetId") {
        if !target.is_string() {
            return Err(SplitError::new(
                code::BILL_TYPE_ERROR,
                "A targetId is a string",
            ));
        }
    }
    if matches!(kind, "voidEntry" | "amendEntry")
        && entry
            .get("targetId")
            .and_then(Value::as_str)
            .unwrap_or("")
            .is_empty()
    {
        return Err(SplitError::new(
            code::BILL_MISSING_ENTRY_PAYLOAD,
            format!("A {kind} names a target"),
        ));
    }

    let at = entry
        .get("at")
        .and_then(Value::as_str)
        .ok_or_else(|| SplitError::new(code::BILL_TYPE_ERROR, "An entry states an instant"))?;
    canonical_instant(at)?;

    if entry.get("id").and_then(Value::as_str).is_none() {
        return Err(SplitError::new(
            code::BILL_TYPE_ERROR,
            "An entry states its id",
        ));
    }
    if entry.get("author").and_then(Value::as_str).is_none() {
        return Err(SplitError::new(
            code::BILL_TYPE_ERROR,
            "An entry states its author",
        ));
    }

    if kind == "createBill" {
        // Both fields, or the entry is unbound and anybody could claim its id.
        if !is_b64url_of_length(entry.get("creatorKey").and_then(Value::as_str), 32)
            || !is_b64url_of_length(entry.get("nonce").and_then(Value::as_str), 16)
        {
            return Err(SplitError::new(
                code::CREATE_UNBOUND,
                "A create entry carries a 32-byte key and a 16-byte nonce",
            ));
        }
        if entry.get("id").and_then(Value::as_str) != Some(derive_bill_id(entry)?.as_str()) {
            return Err(SplitError::new(
                code::CREATE_ID_NOT_DERIVED,
                "A create entry's id is the digest of the entry",
            ));
        }
    } else if entry.get("id").and_then(Value::as_str) != Some(derive_entry_id(entry)?.as_str()) {
        // §9.5. An id anyone may choose is an id anyone may take: without
        // this, re-pushing a copy of an entry with one member changed
        // displaces the genuine one under §10.2 rule 2.
        return Err(SplitError::new(
            code::ENTRY_ID_NOT_DERIVED,
            "An entry's id is the digest of the entry",
        ));
    }
    Ok(())
}

fn field<'a>(entry: &'a Value, name: &str) -> &'a str {
    entry.get(name).and_then(Value::as_str).unwrap_or("")
}

/// The total order: `at`, then `author`, then `id`, all ascending.
pub fn order_entries(entries: &mut [Value]) {
    entries.sort_by(|a, b| {
        field(a, "at")
            .as_bytes()
            .cmp(field(b, "at").as_bytes())
            .then(
                field(a, "author")
                    .as_bytes()
                    .cmp(field(b, "author").as_bytes()),
            )
            .then(field(a, "id").as_bytes().cmp(field(b, "id").as_bytes()))
            // §10.2. The order is total: a comparator that calls two unequal
            // rows equal leaves them to the host's sort.
            .then_with(|| {
                canonical_json(a)
                    .unwrap_or_default()
                    .as_bytes()
                    .cmp(canonical_json(b).unwrap_or_default().as_bytes())
            })
    });
}

/// Merges logs by set union, keyed by entry id (§10.2).
///
/// §10.1 is applied at ingress: an entry that does not carry the payload its
/// kind uses never enters the union.
///
/// A copy carrying a signature beats one that does not; otherwise the entry
/// whose canonical encoding sorts higher wins. Both parts are functions of the
/// two entries alone, which is what makes union commutative.
pub fn merge_logs(logs: &[Vec<Value>]) -> Result<MergeResult> {
    let mut by_id: BTreeMap<String, Value> = BTreeMap::new();
    let mut refused: Vec<SetAside> = Vec::new();
    for log in logs {
        for entry in log {
            // §10.1 at ingress. Removing a payload member makes an entry sort
            // higher under §9.3, so without this the stripped copy wins rule 2
            // and displaces the genuine entry on every device.
            if let Err(e) = check_entry(entry) {
                refused.push(SetAside {
                    id: field(entry, "id").to_owned(),
                    code: e.code,
                });
                continue;
            }
            let id = field(entry, "id").to_owned();
            match by_id.get(&id) {
                None => {
                    by_id.insert(id, entry.clone());
                }
                Some(held) => {
                    let held_signed = held.get("sig").is_some();
                    let entry_signed = entry.get("sig").is_some();
                    let keep = if held_signed != entry_signed {
                        if held_signed {
                            held.clone()
                        } else {
                            entry.clone()
                        }
                    } else if canonical_json(held)?.as_bytes() >= canonical_json(entry)?.as_bytes()
                    {
                        held.clone()
                    } else {
                        entry.clone()
                    };
                    by_id.insert(id, keep);
                }
            }
        }
    }
    let mut out: Vec<Value> = by_id.into_values().collect();
    order_entries(&mut out);
    // §10.2. Total: two rows sharing an id are ordered by their code.
    refused.sort_by(|a, b| {
        a.id.as_bytes()
            .cmp(b.id.as_bytes())
            .then(a.code.as_bytes().cmp(b.code.as_bytes()))
    });
    Ok(MergeResult {
        merged: out,
        refused,
    })
}

/// A merged log and the entries §10.1 refused at ingress.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MergeResult {
    pub merged: Vec<Value>,
    pub refused: Vec<SetAside>,
}

/// An entry the fold could not apply, and why.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SetAside {
    pub id: String,
    pub code: &'static str,
}

/// An address a rejoin replaced.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReplacedAddress {
    pub id: String,
    pub from: Option<String>,
    pub to: Option<String>,
}

/// Everything folding a log reaches.
#[derive(Debug, Clone, PartialEq)]
pub struct FoldResult {
    pub bill: Value,
    /// Named by the withdrawal rules of §10.8.
    pub creator: String,
    pub replaced_addresses: Vec<ReplacedAddress>,
    /// A withdrawal is absent from the fold by design and is otherwise
    /// indistinguishable from an entry that was never written.
    pub withdrawn: Vec<String>,
    pub set_aside: Vec<SetAside>,
    /// Which key speaks for each participant, and which ids two keys claim
    /// (§10.7). Empty when `fold_log` is given no verifier: §13 makes the
    /// curve operation the host's, so a fold that cannot check a signature
    /// reports no binding and no contest rather than claiming there are none.
    pub identities: Identities,
}

fn split_pool(split: &Value) -> BTreeSet<String> {
    let mut pool = BTreeSet::new();
    if let Some(among) = split.get("among").and_then(Value::as_array) {
        pool.extend(among.iter().filter_map(Value::as_str).map(str::to_owned));
    }
    for key in ["amounts", "basisPoints", "shareCounts"] {
        if let Some(map) = split.get(key).and_then(Value::as_object) {
            pool.extend(map.keys().cloned());
        }
    }
    if let Some(items) = split.get("items").and_then(Value::as_array) {
        for item in items {
            if let Some(shared) = item.get("sharedBy").and_then(Value::as_array) {
                pool.extend(shared.iter().filter_map(Value::as_str).map(str::to_owned));
            }
        }
    }
    pool
}

/// Materialises a bill from `raw_entries` (§10.3).
///
/// An entry that cannot be applied is set aside and reported, never raised as
/// a failure of the whole fold: the log merges by union, so one malformed
/// entry propagates to every device, and aborting on it would leave the bill
/// permanently unopenable.
pub fn fold_log(raw_entries: &[Value], bill_id: Option<&str>) -> Result<FoldResult> {
    fold_log_verified(raw_entries, bill_id, None::<fn(&Value, &str) -> bool>)
}

/// Folds a log, checking signatures with the host's verifier (§10.1, §10.7).
pub fn fold_log_verified(
    raw_entries: &[Value],
    bill_id: Option<&str>,
    verify: Option<impl Fn(&Value, &str) -> bool>,
) -> Result<FoldResult> {
    if raw_entries.is_empty() {
        return Err(SplitError::new(
            code::LOG_EMPTY,
            "A log with no entries opens no bill",
        ));
    }
    // §10.3. An entry that cannot be applied is set aside, never raised as a
    // failure of the whole fold: the log merges by union, so one malformed
    // entry reaches every device, and aborting leaves the bill unopenable —
    // including unopenable to append the withdrawal that would remove it.
    let mut refused_at_ingress: Vec<SetAside> = Vec::new();
    let mut entries: Vec<Value> = Vec::new();
    for entry in raw_entries {
        match check_entry(entry) {
            Ok(()) => entries.push(entry.clone()),
            Err(e) => refused_at_ingress.push(SetAside {
                id: entry
                    .get("id")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_owned(),
                code: e.code,
            }),
        }
    }
    // §10.3. One id names one entry in the fold as in the merge: a re-sent
    // entry would otherwise be applied twice.
    let mut entries = merge_logs(&[entries])?.merged;
    order_entries(&mut entries);

    let mut creates: Vec<&Value> = entries
        .iter()
        .filter(|e| field(e, "kind") == "createBill")
        .filter(|e| bill_id.is_none_or(|want| field(e, "id") == want))
        .collect();
    if let Some(verify) = &verify {
        // §10.1. A host that verifies MUST check a create entry's signature
        // against the creatorKey that same entry states — the one key on a
        // bill that needs no prior acquaintance, because §9.4 binds it to the
        // id.
        creates.retain(|e| {
            if verify(e, field(e, "creatorKey")) {
                true
            } else {
                refused_at_ingress.push(SetAside {
                    id: field(e, "id").to_owned(),
                    code: code::UNAUTHORIZED_ENTRY,
                });
                false
            }
        });
    }
    if creates.is_empty() {
        return Err(SplitError::new(
            code::LOG_NO_CREATE,
            "A log holding no create entry opens no bill",
        ));
    }
    if creates.len() > 1 {
        // Anyone holding the invite can push in a create entry of their own,
        // which §9.4 admits because it is valid for a different bill.
        return Err(SplitError::new(
            code::AMBIGUOUS_CREATE,
            format!(
                "A log holds {} create entries and names no bill",
                creates.len()
            ),
        ));
    }
    let create = creates[0].clone();
    let creator = field(&create, "author").to_owned();

    let currency = field(&create, "currency").to_owned();
    check_currency(&currency)?;

    let mode = create
        .get("splitMode")
        .and_then(Value::as_str)
        .unwrap_or("equal")
        .to_owned();
    if !["equal", "percentage"].contains(&mode.as_str()) {
        return Err(SplitError::new(
            code::BILL_UNKNOWN_SPLIT_MODE,
            format!("No such split mode: \"{mode}\""),
        ));
    }

    let by_id: BTreeMap<String, Value> = entries
        .iter()
        .map(|e| (field(e, "id").to_owned(), e.clone()))
        .collect();
    let mut set_aside: Vec<SetAside> = refused_at_ingress;
    let mut amendments: BTreeMap<String, Value> = BTreeMap::new();
    let mut voided: BTreeSet<String> = BTreeSet::new();

    macro_rules! aside {
        ($entry:expr, $code:expr) => {
            set_aside.push(SetAside {
                id: field($entry, "id").to_owned(),
                code: $code,
            })
        };
    }

    // Amendments: authored by the author of their target, carrying a payload
    // of the target's kind. An amendment replaces its target wholesale, so one
    // carrying no payload silently deletes what it claims to correct.
    for entry in &entries {
        if field(entry, "kind") != "amendEntry" {
            continue;
        }
        let Some(target) = by_id.get(field(entry, "targetId")) else {
            aside!(entry, code::UNKNOWN_ENTRY);
            continue;
        };
        if field(entry, "author") != field(target, "author") {
            aside!(entry, code::UNAUTHORIZED_ENTRY);
            continue;
        }
        if let Some(wanted) = payload_for(field(target, "kind")) {
            if entry.get(wanted).is_none() {
                aside!(entry, code::AMEND_KIND_MISMATCH);
                continue;
            }
        }
        amendments.insert(field(entry, "targetId").to_owned(), entry.clone());
    }

    // Withdrawals, §10.8, in two stages.
    //
    // Authorisation first, because only a withdrawal that is allowed to stand
    // may take another one back. Resolving in force over every withdrawal
    // lets a stranger cancel a legitimate one: the fold would report their
    // entry refused and honour it in the same breath.
    let voids: Vec<Value> = entries
        .iter()
        .filter(|e| field(e, "kind") == "voidEntry")
        .cloned()
        .collect();

    let mut authorised: BTreeMap<String, bool> = BTreeMap::new();
    for entry in &voids {
        let id = field(entry, "id").to_owned();
        let Some(target) = by_id.get(field(entry, "targetId")) else {
            aside!(entry, code::UNKNOWN_ENTRY);
            authorised.insert(id, false);
            continue;
        };
        let kind = field(target, "kind");
        let mut allowed: BTreeSet<String> = BTreeSet::new();
        allowed.insert(field(target, "author").to_owned());
        match kind {
            // An expense is entered by hand and duplicated by accident, and
            // the person who entered it may be asleep.
            "addExpense" => {
                allowed.insert(creator.clone());
            }
            // Withdrawing a payment reopens a debt somebody believed settled,
            // so the creator is not given this.
            "recordPayment" => {
                if let Some(pay) = target.get("payment") {
                    allowed.insert(field(pay, "from").to_owned());
                    allowed.insert(field(pay, "to").to_owned());
                }
            }
            "joinBill" => {
                allowed.clear();
                allowed.insert(creator.clone());
                if let Some(p) = target.get("participant") {
                    allowed.insert(field(p, "id").to_owned());
                }
            }
            _ => {}
        }
        if !allowed.contains(field(entry, "author")) {
            aside!(entry, code::UNAUTHORIZED_ENTRY);
            authorised.insert(id, false);
            continue;
        }
        authorised.insert(id, true);
    }

    // A withdrawal is in force unless a later authorised withdrawal, itself in
    // force, names it. Resolved from the latest backwards, so by the time one
    // is considered every withdrawal that could name it has been decided.
    let mut in_force: BTreeMap<String, bool> = BTreeMap::new();
    for entry in voids.iter().rev() {
        let id = field(entry, "id").to_owned();
        if !authorised.get(&id).copied().unwrap_or(false) {
            in_force.insert(id, false);
            continue;
        }
        let withdrawn = voids.iter().any(|other| {
            field(other, "targetId") == id
                && in_force.get(field(other, "id")).copied().unwrap_or(false)
        });
        in_force.insert(id, !withdrawn);
    }

    for entry in &voids {
        if in_force.get(field(entry, "id")).copied().unwrap_or(false) {
            voided.insert(field(entry, "targetId").to_owned());
        }
    }

    // An amendment whose own entry was withdrawn is discarded with it, so the
    // entry it corrected reads as it was written. Collecting amendments before
    // withdrawals are resolved and applying them afterwards would leave a
    // retracted correction standing: the figure a person took back would be
    // the figure the bill shows.
    amendments.retain(|_, e| !voided.contains(field(e, "id")));

    let effective = |entry: &Value| -> Value {
        amendments
            .get(field(entry, "id"))
            .cloned()
            .unwrap_or_else(|| entry.clone())
    };

    // Taking somebody off the bill. This runs after every other withdrawal is
    // resolved and before the joins are applied: a check made once the person
    // is gone is a check made too late.
    let mut removals: Vec<(String, String)> = Vec::new();
    for entry in &entries {
        if field(entry, "kind") != "voidEntry" || !voided.contains(field(entry, "targetId")) {
            continue;
        }
        let target = &by_id[field(entry, "targetId")];
        if field(target, "kind") != "joinBill" {
            continue;
        }
        let gone = target
            .get("participant")
            .map(|p| field(p, "id").to_owned())
            .unwrap_or_default();
        removals.push((field(entry, "id").to_owned(), gone));
    }
    for (void_id, gone) in removals {
        let mut named = false;
        for other in &entries {
            if voided.contains(field(other, "id")) || field(other, "kind") == "voidEntry" {
                continue;
            }
            let eff = effective(other);
            match field(other, "kind") {
                "addExpense" => {
                    if let Some(ex) = eff.get("expense") {
                        let pool = ex.get("split").map(split_pool).unwrap_or_default();
                        if field(ex, "paidBy") == gone || pool.contains(&gone) {
                            named = true;
                        }
                    }
                }
                "recordPayment" => {
                    if let Some(pay) = eff.get("payment") {
                        if field(pay, "from") == gone || field(pay, "to") == gone {
                            named = true;
                        }
                    }
                }
                "confirmPayment" if field(other, "author") == gone => {
                    named = true;
                }
                _ => {}
            }
            if named {
                break;
            }
        }
        if named {
            // Without this, removing the person who spent the most silently
            // drops every expense they paid for.
            let target_id = field(&by_id[&void_id], "targetId").to_owned();
            voided.remove(&target_id);
            set_aside.push(SetAside {
                id: void_id,
                code: code::PARTICIPANT_STILL_NAMED,
            });
        }
    }

    let live: Vec<Value> = entries
        .iter()
        .filter(|e| !voided.contains(field(e, "id")) && field(e, "kind") != "voidEntry")
        .cloned()
        .collect();

    // §10.1. The latest live setRate decides, by §10.2's order, so the answer
    // is a function of the log and not of which device last spoke.
    let mut rate: Option<Value> = None;
    for entry in &live {
        if field(entry, "kind") != "setRate" {
            continue;
        }
        if let Some(payload) = effective(entry).get("rate") {
            if payload.is_object() {
                rate = Some(payload.clone());
            }
        }
    }

    // Participants in a pass of their own, before anything that references
    // them.
    let mut participants: BTreeMap<String, Value> = BTreeMap::new();
    let mut replaced: Vec<ReplacedAddress> = Vec::new();
    for entry in &live {
        if field(entry, "kind") != "joinBill" {
            continue;
        }
        let eff = effective(entry);
        let Some(p) = eff.get("participant").filter(|p| p.is_object()) else {
            aside!(entry, code::BILL_MISSING_ENTRY_PAYLOAD);
            continue;
        };
        let id = field(p, "id").to_owned();
        if id.is_empty() {
            aside!(entry, code::BILL_MISSING_ENTRY_PAYLOAD);
            continue;
        }
        if participants.contains_key(&id) && field(entry, "author") != id {
            // Without this, one join naming another participant's id and
            // carrying your own address redirects every later settlement.
            aside!(entry, code::UNAUTHORIZED_ENTRY);
            continue;
        }
        if let Some(held) = participants.get(&id) {
            let before = held.get("payTo").and_then(Value::as_str);
            let after = p.get("payTo").and_then(Value::as_str);
            if before != after {
                replaced.push(ReplacedAddress {
                    id: id.clone(),
                    from: before.map(str::to_owned),
                    to: after.map(str::to_owned),
                });
            }
        }
        participants.insert(id, p.clone());
    }

    let mut expenses: Vec<Value> = Vec::new();
    let mut payments: Vec<Value> = Vec::new();
    for entry in &live {
        let eff = effective(entry);
        match field(entry, "kind") {
            "addExpense" => {
                let mut ex = eff["expense"].clone();
                let obj = ex.as_object_mut().ok_or_else(|| {
                    SplitError::new(code::BILL_TYPE_ERROR, "An expense is an object")
                })?;
                // An amount that states no currency is denominated by the
                // fold. One that states another is set aside, never
                // restamped: that would keep the count and change the unit.
                match obj.get("currency").and_then(Value::as_str) {
                    None => {
                        obj.insert("currency".into(), Value::String(currency.clone()));
                    }
                    Some(own) => {
                        check_currency(own)?;
                        if own != currency {
                            aside!(entry, code::CURRENCY_MISMATCH);
                            continue;
                        }
                    }
                }
                if !participants.contains_key(field(&ex, "paidBy")) {
                    aside!(entry, code::UNKNOWN_PARTICIPANT);
                    continue;
                }
                expenses.push(ex);
            }
            "recordPayment" => {
                let mut pay = eff["payment"].clone();
                let author = field(entry, "author");
                // A payment moves both parties' balances, so without this any
                // holder of the invite could clear a debt neither had settled.
                if author != field(&pay, "from") && author != field(&pay, "to") {
                    aside!(entry, code::UNAUTHORIZED_PAYMENT);
                    continue;
                }
                if !participants.contains_key(field(&pay, "from"))
                    || !participants.contains_key(field(&pay, "to"))
                {
                    aside!(entry, code::UNKNOWN_PARTICIPANT);
                    continue;
                }
                if field(&pay, "from") == field(&pay, "to") {
                    aside!(entry, code::SELF_PAYMENT);
                    continue;
                }
                let obj = pay.as_object_mut().ok_or_else(|| {
                    SplitError::new(code::BILL_TYPE_ERROR, "A payment is an object")
                })?;
                match obj.get("currency").and_then(Value::as_str) {
                    None => {
                        obj.insert("currency".into(), Value::String(currency.clone()));
                    }
                    Some(own) => {
                        check_currency(own)?;
                        if own != currency {
                            aside!(entry, code::CURRENCY_MISMATCH);
                            continue;
                        }
                    }
                }
                payments.push(pay);
            }
            _ => {}
        }
    }

    // Confirmations in a pass of their own, once every payment is on the bill:
    // one may arrive before the payment it vouches for, and a single pass
    // would set aside one that is merely early.
    let known: BTreeSet<String> = payments.iter().map(|p| field(p, "id").to_owned()).collect();
    let mut confirmed: BTreeSet<String> = BTreeSet::new();
    for entry in &live {
        if field(entry, "kind") != "confirmPayment" {
            continue;
        }
        let eff = effective(entry);
        let c = eff.get("confirmation").cloned().unwrap_or(Value::Null);
        let Some((speaks_for, needs_reference, settles)) = confirmation_rule(field(&c, "method"))
        else {
            aside!(entry, code::BILL_UNKNOWN_CONFIRMATION_METHOD);
            continue;
        };
        let payment_id = field(&c, "paymentId").to_owned();
        if !known.contains(&payment_id) {
            aside!(entry, code::UNKNOWN_PAYMENT);
            continue;
        }
        if !participants.contains_key(field(entry, "author")) {
            aside!(entry, code::UNKNOWN_PARTICIPANT);
            continue;
        }
        let pay = payments
            .iter()
            .find(|p| field(p, "id") == payment_id)
            .expect("checked above");
        if let Some(role) = speaks_for {
            // A confirmation's whole weight is in who gave it, so a method
            // anyone may claim is a method that says nothing.
            if field(entry, "author") != field(pay, role) {
                aside!(entry, code::UNAUTHORIZED_CONFIRMATION);
                continue;
            }
        }
        if needs_reference && field(&c, "reference").is_empty() {
            // One that says a payment is on a chain without saying where
            // contains no chain.
            aside!(entry, code::CONFIRMATION_MISSING_REFERENCE);
            continue;
        }
        if settles {
            confirmed.insert(payment_id);
        }
    }

    // §10.2. Total: two rows sharing an id are ordered by their code.
    set_aside.sort_by(|a, b| {
        a.id.as_bytes()
            .cmp(b.id.as_bytes())
            .then(a.code.as_bytes().cmp(b.code.as_bytes()))
    });

    let mut bill = serde_json::json!({
        "v": crate::serialization::BILL_VERSION,
        "id": field(&create, "id"),
        "name": create.get("name").and_then(Value::as_str).unwrap_or(""),
        "currency": currency,
        "splitMode": mode,
        "participants": participants.values().cloned().collect::<Vec<_>>(),
        "expenses": expenses,
        "payments": payments,
        "confirmedPayments": confirmed.iter().cloned().collect::<Vec<_>>(),
    });
    if let Some(rate) = rate {
        bill.as_object_mut()
            .expect("an object")
            .insert("rate".into(), rate);
    }

    // §10.7, over the same entry set the bill was materialised from. Without
    // a verifier nothing can be decided, and nothing is claimed.
    let identities = match &verify {
        Some(verify) => crate::authority::resolve_identities(&entries, &create, verify),
        None => Identities::default(),
    };

    Ok(FoldResult {
        bill,
        creator,
        replaced_addresses: replaced,
        withdrawn: voided.into_iter().collect(),
        set_aside,
        identities,
    })
}
