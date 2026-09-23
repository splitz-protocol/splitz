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
use crate::money::{check_currency, checked_add, checked_sub, is_currency};
use crate::sha256::sha256;
use crate::zip321::{base64url, unbase64url};

pub const ENTRY_KINDS: [&str; 8] = [
    "createBill",
    "joinBill",
    "addExpense",
    "amendEntry",
    "voidEntry",
    "recordPayment",
    "confirmPayment",
    "setRate",
];

const PAYLOAD_NAMES: [&str; 4] = ["rate", "expense", "payment", "confirmation"];

/// Every member that is a payload, including the one no kind lists as
/// ambiguous (§10.1).
///
/// `PAYLOAD_NAMES` drives the ambiguity check and omits `participant`; the
/// type check at ingress must not, because an `amendEntry` may carry any of
/// them and every later pass indexes what it finds.
const PAYLOAD_MEMBERS: [&str; 5] = ["rate", "expense", "payment", "confirmation", "participant"];

/// The domain separator the bill id digest covers.
pub const BILL_ID_DOMAIN: &str = "splitz-bill-id-v1";

/// The domain separator an entry id's digest covers (§9.5).
///
/// Different from §9.4's so that a `createBill` id and any other entry's id
/// are drawn from different spaces and neither can be presented as the other.
pub const ENTRY_ID_DOMAIN: &str = "splitz-entry-id-v1";

/// §10.4: the member naming what each kind of entry is about, which an
/// amendment may not change.
fn amended_subject(kind: &str) -> Option<(&'static str, &'static str)> {
    match kind {
        "joinBill" => Some(("participant", "id")),
        "addExpense" => Some(("expense", "id")),
        "recordPayment" => Some(("payment", "id")),
        "confirmPayment" => Some(("confirmation", "paymentId")),
        _ => None,
    }
}

/// The payload each kind carries, and no other.
pub fn payload_for(kind: &str) -> Option<&'static str> {
    match kind {
        "joinBill" => Some("participant"),
        "addExpense" => Some("expense"),
        "recordPayment" => Some("payment"),
        "confirmPayment" => Some("confirmation"),
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
        // The recipient saying they saw the transaction land; a shielded
        // payment is visible to nobody else.
        "onChain" => Some((Some("to"), true, true)),
        // A payer saying they paid is the claim of the record, not evidence.
        "payerAttested" => Some((Some("from"), false, false)),
        _ => None,
    }
}

/// Whether `value` is the canonical unpadded base64url of `bytes` bytes (§9.4).
fn is_b64url_of_length(value: Option<&str>, bytes: usize) -> bool {
    value
        .filter(|text| !text.is_empty())
        .and_then(unbase64url)
        .is_some_and(|raw| raw.len() == bytes)
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

/// The members of a payload that name a participant or an entry (§10.1).
fn id_members_of(wanted: &str) -> &'static [&'static str] {
    match wanted {
        "participant" => &["id"],
        "expense" => &["id", "paidBy"],
        "payment" => &["id", "from", "to"],
        "confirmation" => &["paymentId"],
        _ => &[],
    }
}

/// Checks an entry before it reaches a log (§10.1).
///
/// An entry carrying more than one payload is refused because the currency
/// fallback and the fold would otherwise read different ones. One carrying
/// none is refused because removing a member makes an entry's canonical
/// encoding sort higher than the same entry with it, so the merge would keep
/// the stripped copy.
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

    // §10.1. A signature is a string or absent. Anything else is a third state
    // §10.2 would have to rank, and `null` sorts above every string, so it
    // would win every merge it entered.
    if entry.get("sig").is_some_and(|s| !s.is_string()) {
        return Err(SplitError::new(
            code::BILL_TYPE_ERROR,
            "A signature is a string",
        ));
    }
    // §10.1. `v` sits outside the id (§9.5), so a copy with any value keeps
    // the honest id; one that is not an integer would reach the canonical
    // encoding the merge and the order compare, and stop there.
    if entry
        .get("v")
        .is_some_and(|v| !v.as_u64().is_some_and(|n| n >= 1))
    {
        return Err(SplitError::new(
            code::BILL_TYPE_ERROR,
            "An entry version is an integer of 1 or more",
        ));
    }

    // §10.1. An entry arriving over a relay (§11.3) never passes §11.2's cap,
    // and every pass below walks it — deriving its id encodes it. A depth
    // nobody bounded is a stack the peer chose.
    if !crate::invite::within_depth(entry, crate::invite::MAX_DOCUMENT_DEPTH) {
        return Err(SplitError::new(
            code::BILL_TYPE_ERROR,
            "An entry nests deeper than the document limit",
        ));
    }

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
    // Every payload an entry carries is an object, whatever the kind names.
    // An amendEntry carries the payload it replaces and no kind declares it,
    // so without this a scalar reaches the fold and is indexed there.
    for name in PAYLOAD_MEMBERS {
        if let Some(value) = entry.get(name) {
            if !value.is_object() {
                return Err(SplitError::new(
                    code::BILL_TYPE_ERROR,
                    format!("A {name} is an object"),
                ));
            }
        }
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
        // The fold copies these into the bill document without re-reading
        // them, so they are decided here rather than at decode, where the
        // whole bill would be unopenable instead of this entry refused.
        if let Some(name) = entry.get("name") {
            if !name.is_string() {
                return Err(SplitError::new(
                    code::BILL_TYPE_ERROR,
                    "A bill states its name as a string",
                ));
            }
        }
        match entry.get("currency").and_then(Value::as_str) {
            Some(c) => crate::money::check_currency(c)?,
            None => {
                return Err(SplitError::new(
                    code::BILL_BAD_CURRENCY,
                    "A bill states its currency",
                ))
            }
        }
        // §9.1. An optional scalar does not read `null` as absent.
        let mode = match entry.get("splitMode") {
            None => "equal",
            Some(v) => v.as_str().ok_or_else(|| {
                SplitError::new(code::BILL_TYPE_ERROR, "A split mode is a string")
            })?,
        };
        if !crate::serialization::SPLIT_MODES.contains(&mode) {
            return Err(SplitError::new(
                code::BILL_UNKNOWN_SPLIT_MODE,
                format!("No such split mode: \"{mode}\""),
            ));
        }
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

/// Merges logs by set union, keyed by entry id and signature (§10.2).
///
/// §10.1 is applied at ingress: an entry that does not carry the payload its
/// kind uses never enters the union.
///
/// A signed copy beats an unsigned one. Two signed copies with different
/// signatures are both kept: §9.5's digest does not cover `sig`, so nothing in
/// the pair says which is the author's, and resolving it by order would hand
/// the id to whichever signature sorts higher. The fold decides with the key
/// (§10.3). Copies sharing a signature, or all unsigned, resolve to the one
/// whose canonical encoding sorts higher. Every rule is a function of the
/// copies alone, which is what makes union commutative.
pub fn merge_logs(logs: &[Vec<Value>]) -> Result<MergeResult> {
    // Id, then signature (None when unsigned), to the copy held.
    let mut copies: BTreeMap<String, BTreeMap<Option<String>, Value>> = BTreeMap::new();
    let mut refused: Vec<SetAside> = Vec::new();
    for log in logs {
        for entry in log {
            // §10.1 at ingress. Removing a payload member makes an entry sort
            // higher under §9.3, so without this the stripped copy wins rule 3
            // and displaces the genuine entry on every device.
            if let Err(e) = check_entry(entry) {
                refused.push(SetAside {
                    id: field(entry, "id").to_owned(),
                    code: e.code,
                });
                continue;
            }
            let held = copies.entry(field(entry, "id").to_owned()).or_default();
            let sig = entry.get("sig").and_then(Value::as_str).map(str::to_owned);
            let replace = match held.get(&sig) {
                None => true,
                Some(other) => {
                    canonical_json(entry)?.as_bytes() > canonical_json(other)?.as_bytes()
                }
            };
            if replace {
                held.insert(sig, entry.clone());
            }
        }
    }
    let mut out: Vec<Value> = Vec::new();
    for held in copies.into_values() {
        let signed: Vec<Value> = held
            .iter()
            .filter(|(sig, _)| sig.is_some())
            .map(|(_, e)| e.clone())
            .collect();
        if signed.is_empty() {
            out.extend(held.into_values());
        } else {
            out.extend(signed);
        }
    }
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

/// Where a participant is paid (§10.3 step 4): the address of their first
/// payout when they declare any, their payTo otherwise, or none. A value the
/// decoder never read is taken as none.
fn destination(participant: &Value) -> Option<String> {
    let address = match participant.get("payouts").and_then(Value::as_array) {
        Some(payouts) if !payouts.is_empty() => payouts[0].get("address"),
        _ => participant.get("payTo"),
    };
    address.and_then(Value::as_str).map(str::to_owned)
}

/// The copy whose canonical encoding sorts highest (§10.2 rule 3).
fn highest<'a>(copies: &[&'a Value]) -> Result<&'a Value> {
    let mut best = copies[0];
    for &c in &copies[1..] {
        if canonical_json(c)?.as_bytes() > canonical_json(best)?.as_bytes() {
            best = c;
        }
    }
    Ok(best)
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
    /// Who wrote each payment record on the bill, by the payment's id. Not
    /// part of the bill document, which restates neither author nor instant
    /// (§10.5). §14.4 withholds only for a record the payer wrote.
    pub payment_authors: BTreeMap<String, String>,
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
    // §10.3. Every copy the merge kept, several under one id when their
    // signatures differ.
    let copies = merge_logs(&[entries])?.merged;

    let mut creates: Vec<&Value> = copies
        .iter()
        .filter(|e| field(e, "kind") == "createBill")
        .filter(|e| bill_id.is_none_or(|want| field(e, "id") == want))
        .collect();
    if let Some(verify) = &verify {
        // §10.1. A host that verifies MUST check a create entry's signature
        // against the creatorKey that same entry states — the one key on a
        // bill that needs no prior acquaintance, because §9.4 binds it to the
        // id. A create is refused only when no copy of it verifies.
        let all: BTreeSet<String> = creates.iter().map(|e| field(e, "id").to_owned()).collect();
        creates.retain(|e| verify(e, field(e, "creatorKey")));
        let kept: BTreeSet<String> = creates.iter().map(|e| field(e, "id").to_owned()).collect();
        for id in all.difference(&kept) {
            refused_at_ingress.push(SetAside {
                id: id.clone(),
                code: code::UNAUTHORIZED_ENTRY,
            });
        }
    }
    let create_ids: BTreeSet<&str> = creates.iter().map(|e| field(e, "id")).collect();
    if create_ids.is_empty() {
        return Err(SplitError::new(
            code::LOG_NO_CREATE,
            "A log holding no create entry opens no bill",
        ));
    }
    if create_ids.len() > 1 {
        // Anyone holding the invite can push in a create entry of their own,
        // which §9.4 admits because it is valid for a different bill.
        return Err(SplitError::new(
            code::AMBIGUOUS_CREATE,
            format!(
                "A log holds {} create entries and names no bill",
                create_ids.len()
            ),
        ));
    }
    let create = highest(&creates)?.clone();

    // §10.7, over every copy: a withdrawal does not undo a claim, and a copy
    // nobody applies is still evidence that was made. Without a verifier
    // nothing can be decided, and nothing is claimed.
    let identities = match &verify {
        Some(verify) => crate::authority::resolve_identities(&copies, &create, verify),
        None => Identities::default(),
    };

    // §10.3. One id names one entry: a re-sent entry would otherwise be
    // applied twice, and one expense sent twice doubles what everybody owes.
    // An author with a key is spoken for only by a copy that verifies
    // against it.
    let mut groups: BTreeMap<&str, Vec<&Value>> = BTreeMap::new();
    for e in &copies {
        groups.entry(field(e, "id")).or_default().push(e);
    }
    let mut entries: Vec<Value> = Vec::new();
    for (id, group) in groups {
        let first = group[0];
        let mut candidates = group.clone();
        if let Some(verify) = &verify {
            let key = if field(first, "kind") == "createBill" {
                first
                    .get("creatorKey")
                    .and_then(Value::as_str)
                    .map(str::to_owned)
            } else {
                identities.bound.get(field(first, "author")).cloned()
            };
            if let Some(key) = key {
                candidates.retain(|e| verify(e, &key));
                if candidates.is_empty() {
                    refused_at_ingress.push(SetAside {
                        id: id.to_owned(),
                        code: code::UNAUTHORIZED_ENTRY,
                    });
                    continue;
                }
            }
        }
        entries.push(highest(&candidates)?.clone());
    }
    order_entries(&mut entries);
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
        // §10.4. The id the target is about stays: renaming it makes a
        // different entry the §10.8 checks never read.
        if let Some((payload, member)) = amended_subject(field(target, "kind")) {
            if entry[payload].get(member) != target[payload].get(member) {
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

    // A withdrawal is in force unless an authorised withdrawal naming it is
    // itself in force. `at` plays no part: an id is the digest of its entry,
    // so a withdrawal can only name one that existed when it was written, and
    // the chains are acyclic. Resolved by what names what, from the entries
    // nothing names inwards.
    let mut naming: BTreeMap<String, Vec<String>> = BTreeMap::new();
    for entry in &voids {
        naming
            .entry(field(entry, "targetId").to_owned())
            .or_default()
            .push(field(entry, "id").to_owned());
    }
    let mut in_force: BTreeMap<String, bool> = BTreeMap::new();
    for start in &voids {
        let mut stack: Vec<(String, bool)> = vec![(field(start, "id").to_owned(), false)];
        while let Some((id, expanded)) = stack.pop() {
            if in_force.contains_key(&id) {
                continue;
            }
            if !authorised.get(&id).copied().unwrap_or(false) {
                in_force.insert(id, false);
                continue;
            }
            let namers = naming.get(&id).cloned().unwrap_or_default();
            let pending: Vec<String> = namers
                .iter()
                .filter(|w| !in_force.contains_key(*w))
                .cloned()
                .collect();
            if !pending.is_empty() && !expanded {
                stack.push((id, true));
                stack.extend(pending.into_iter().map(|w| (w, false)));
                continue;
            }
            let withdrawn = namers
                .iter()
                .any(|w| in_force.get(w).copied().unwrap_or(false));
            in_force.insert(id, !withdrawn);
        }
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
        if identities.bound.contains_key(&id) && field(entry, "author") != id {
            // §10.7. A bound participant's record is theirs to create as well
            // as to change, so a payout cannot be redirected to somebody who
            // never joined by an entry of their own.
            aside!(entry, code::UNAUTHORIZED_ENTRY);
            continue;
        }
        if participants.contains_key(&id) && field(entry, "author") != id {
            // Without this, one join naming another participant's id and
            // carrying your own address redirects every later settlement.
            aside!(entry, code::UNAUTHORIZED_ENTRY);
            continue;
        }
        // The decoder decides what a participant is, here rather than once
        // the document is assembled: a member it would refuse sets this entry
        // aside (§10.3) instead of making the whole bill undecodable.
        if let Err(e) = crate::serialization::decode_participant(p) {
            aside!(entry, e.code);
            continue;
        }
        // §10.3 step 4. The destination this record replaces: the one held
        // for the participant, or, for the first record, the one the join was
        // written with before an amendment changed it.
        let before = if let Some(held) = participants.get(&id) {
            destination(held)
        } else if amendments.contains_key(field(entry, "id")) {
            entry.get("participant").and_then(destination)
        } else {
            destination(p)
        };
        let after = destination(p);
        if before != after {
            replaced.push(ReplacedAddress {
                id: id.clone(),
                from: before,
                to: after,
            });
        }
        participants.insert(id, p.clone());
    }

    // §10.1. The latest live setRate by a participant decides, by §10.2's
    // order, so the answer is a function of the log and not of which device
    // last spoke. Decided after the participants, because only they may set
    // it.
    let mut rate: Option<Value> = None;
    for entry in &live {
        if field(entry, "kind") != "setRate" {
            continue;
        }
        if !participants.contains_key(field(entry, "author")) {
            aside!(entry, code::UNKNOWN_PARTICIPANT);
            continue;
        }
        let payload = effective(entry).get("rate").cloned().unwrap_or(Value::Null);
        if let Err(e) = crate::serialization::decode_rate(&payload) {
            aside!(entry, e.code);
            continue;
        }
        rate = Some(payload);
    }

    let mut expenses: Vec<Value> = Vec::new();
    let mut payments: Vec<Value> = Vec::new();
    let mut payment_authors: BTreeMap<String, String> = BTreeMap::new();
    // §5.1's balances, formed as this pass applies each entry and in the order
    // §5.1 forms them, so a bill this fold returns always has balances §2.2
    // can hold. An entry whose effect would carry one out of range is set
    // aside, deterministically and in log order, rather than left to make §5
    // refuse the whole bill.
    let mut running: BTreeMap<String, i64> =
        participants.keys().map(|id| (id.clone(), 0)).collect();
    let mut pair_total: BTreeMap<(String, String), i64> = BTreeMap::new();
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
                // §9.1 falls back only when the member is ABSENT. `as_str`
                // alone cannot tell absent from present-and-not-a-string, and
                // a present value that is not a currency is an entry that
                // cannot be applied: §10.3 sets those aside.
                match obj.get("currency") {
                    None => {
                        obj.insert("currency".into(), Value::String(currency.clone()));
                    }
                    Some(own) => match own.as_str() {
                        Some(own) if is_currency(own) => {
                            if own != currency {
                                aside!(entry, code::CURRENCY_MISMATCH);
                                continue;
                            }
                        }
                        _ => {
                            aside!(entry, code::BILL_BAD_CURRENCY);
                            continue;
                        }
                    },
                }
                if !participants.contains_key(field(&ex, "paidBy")) {
                    aside!(entry, code::UNKNOWN_PARTICIPANT);
                    continue;
                }
                let ids: std::collections::BTreeSet<String> =
                    participants.keys().cloned().collect();
                // §4 is what turns an expense into what each person owes,
                // and §5 runs it downstream of this fold. An expense whose
                // split §4 refuses cannot be applied, so it is set aside here
                // rather than raising out of `net_balances` once the bill is
                // already built.
                let applied =
                    crate::serialization::decode_expense(&ex, &currency, &ids).and_then(|d| {
                        let shares = crate::split::split_expense(d.amount, &d.split)?;
                        let mut moved = running.clone();
                        let payer = moved.get_mut(&d.paid_by).expect("checked above");
                        *payer = checked_add(*payer, d.amount, code::AMOUNT_OVERFLOW)?;
                        for (id, owed) in &shares {
                            let held = moved.get_mut(id).expect("decoded against the bill");
                            *held = checked_sub(*held, *owed, code::AMOUNT_OVERFLOW)?;
                        }
                        Ok(moved)
                    });
                match applied {
                    Ok(moved) => running = moved,
                    Err(e) => {
                        aside!(entry, e.code);
                        continue;
                    }
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
                // §9.1 falls back only when the member is ABSENT. `as_str`
                // alone cannot tell absent from present-and-not-a-string, and
                // a present value that is not a currency is an entry that
                // cannot be applied: §10.3 sets those aside.
                match obj.get("currency") {
                    None => {
                        obj.insert("currency".into(), Value::String(currency.clone()));
                    }
                    Some(own) => match own.as_str() {
                        Some(own) if is_currency(own) => {
                            if own != currency {
                                aside!(entry, code::CURRENCY_MISMATCH);
                                continue;
                            }
                        }
                        _ => {
                            aside!(entry, code::BILL_BAD_CURRENCY);
                            continue;
                        }
                    },
                }
                let ids: std::collections::BTreeSet<String> =
                    participants.keys().cloned().collect();
                if let Err(e) = crate::serialization::decode_payment(&pay, &currency, &ids) {
                    aside!(entry, e.code);
                    continue;
                }
                // §10.5: a confirmation names one record, and a method that
                // speaks for the payment's `to` is checked against that
                // record's `to`. Two records under one id name a payee
                // ambiguously, so one recipient's confirmation would settle a
                // debt another never vouched for. The first record stands and
                // the second is refused; one transaction paying several people
                // carries the transaction in `reference`, not in the id.
                let pay_id = field(&pay, "id").to_owned();
                if payments.iter().any(|p| field(p, "id") == pay_id) {
                    aside!(entry, code::DUPLICATE_PAYMENT);
                    continue;
                }
                // What one participant has recorded paying another, confirmed
                // or not, stays in range: §14.4 sums the unconfirmed part.
                let pair = (field(&pay, "from").to_owned(), field(&pay, "to").to_owned());
                let amount = pay["amount"].as_i64().unwrap_or(0);
                let held = pair_total.get(&pair).copied().unwrap_or(0);
                match checked_add(held, amount, code::AMOUNT_OVERFLOW) {
                    Ok(total) => {
                        pair_total.insert(pair, total);
                    }
                    Err(e) => {
                        aside!(entry, e.code);
                        continue;
                    }
                }
                if let Some(id) = pay.get("id").and_then(Value::as_str) {
                    payment_authors.insert(id.to_owned(), field(entry, "author").to_owned());
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
    let mut confirmed_by: BTreeMap<String, Vec<String>> = BTreeMap::new();
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
        // A non-empty STRING, not merely something `field` renders empty. A
        // number or a list here is not a transaction id, and reading
        // "present" three different ways settles a debt on one device and
        // leaves it open on another.
        let has_reference = c
            .get("reference")
            .and_then(Value::as_str)
            .is_some_and(|r| !r.is_empty());
        if needs_reference && !has_reference {
            // One that says a payment is on a chain without saying where
            // contains no chain.
            aside!(entry, code::CONFIRMATION_MISSING_REFERENCE);
            continue;
        }
        if settles {
            confirmed_by
                .entry(payment_id.clone())
                .or_default()
                .push(field(entry, "id").to_owned());
            confirmed.insert(payment_id);
        }
    }

    // Confirmed payments move balances in the order the bill lists them
    // (§5.1). One that would carry a balance out of range stays unconfirmed,
    // and every confirmation that settled it is set aside.
    for pay in &payments {
        let id = field(pay, "id");
        if !confirmed.contains(id) {
            continue;
        }
        let amount = pay["amount"].as_i64().unwrap_or(0);
        let (from, to) = (field(pay, "from"), field(pay, "to"));
        let moved = checked_add(running[from], amount, code::AMOUNT_OVERFLOW)
            .and_then(|f| checked_sub(running[to], amount, code::AMOUNT_OVERFLOW).map(|t| (f, t)));
        match moved {
            Ok((f, t)) => {
                running.insert(from.to_owned(), f);
                running.insert(to.to_owned(), t);
            }
            Err(e) => {
                confirmed.remove(id);
                for entry_id in &confirmed_by[id] {
                    set_aside.push(SetAside {
                        id: entry_id.clone(),
                        code: e.code,
                    });
                }
            }
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

    Ok(FoldResult {
        bill,
        creator,
        replaced_addresses: replaced,
        withdrawn: voided.into_iter().collect(),
        set_aside,
        identities,
        payment_authors,
    })
}
