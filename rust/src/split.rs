//! The five split methods (SPEC.md §4).
//!
//! Every method produces shares summing exactly to the expense total.
//! Participants are ordered by ascending id (§2.3) before allocation, so the
//! leftover units of §3 land on the same people everywhere.

use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet};

use crate::allocation::{allocate, allocate_evenly};
use crate::error::{code, Result, SplitError};
use crate::money::checked_sum;
use crate::ordering::{sorted_utf8, unique_sorted_utf8};

/// Owed minor units per participant, in ascending id order.
pub type Shares = BTreeMap<String, i64>;

/// True when `value` carries the sign opposite to `total` (§4).
///
/// The rule is sign agreement rather than non-negativity because §10.4 makes a
/// refund an expense with a negative total, and every method must divide one.
fn against(value: i64, total: i64) -> bool {
    if total < 0 {
        value > 0
    } else {
        value < 0
    }
}

fn type_error(what: &str, value: &Value) -> SplitError {
    SplitError::new(
        code::BILL_TYPE_ERROR,
        format!("Expected {what}, got {value}"),
    )
}

fn integer(value: &Value) -> Result<i64> {
    value
        .as_i64()
        .ok_or_else(|| type_error("an integer", value))
}

fn id_list(value: &Value) -> Vec<String> {
    value
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|v| v.as_str())
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
}

/// Every participant id a split names, whatever method it uses.
///
/// Kept beside the split methods so a new method cannot add a place an id
/// hides. The ids are returned rather than checked here: this module knows
/// nothing about which bill a split belongs to.
pub fn split_participants(spec: &Value) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let Some(spec) = spec.as_object() else {
        return out;
    };
    if let Some(among) = spec.get("among").and_then(Value::as_array) {
        out.extend(among.iter().filter_map(Value::as_str).map(str::to_owned));
    }
    for key in ["amounts", "basisPoints", "shareCounts"] {
        if let Some(map) = spec.get(key).and_then(Value::as_object) {
            out.extend(map.keys().cloned());
        }
    }
    if let Some(items) = spec.get("items").and_then(Value::as_array) {
        for item in items {
            if let Some(shared) = item.get("sharedBy").and_then(Value::as_array) {
                out.extend(shared.iter().filter_map(Value::as_str).map(str::to_owned));
            }
        }
    }
    out
}

/// Refuses an id list holding anything but strings (§10.1, §4).
///
/// Dropping a member a reader cannot read reassigns that participant's share
/// to the others: three people splitting 9000 become two paying 4500 each.
pub fn check_id_lists(spec: &Value) -> Result<()> {
    for key in ["among", "sharedBy"] {
        if let Some(list) = spec.get(key).and_then(Value::as_array) {
            if list.iter().any(|v| !v.is_string()) {
                return Err(SplitError::new(
                    code::BILL_TYPE_ERROR,
                    format!("A {key} names participants as strings"),
                ));
            }
        }
    }
    // A JSON object's keys are strings by construction, so `amounts`,
    // `basisPoints` and `shareCounts` need no check here.
    if let Some(items) = spec.get("items").and_then(Value::as_array) {
        for item in items {
            check_id_lists(item)?;
        }
    }
    Ok(())
}

fn int_map(value: &Value) -> Result<BTreeMap<String, i64>> {
    let mut out = BTreeMap::new();
    if let Some(obj) = value.as_object() {
        for (k, v) in obj {
            out.insert(k.clone(), integer(v)?);
        }
    }
    Ok(out)
}

fn zip(ids: &[String], parts: Vec<i64>) -> Shares {
    ids.iter().cloned().zip(parts).collect()
}

/// Resolves `total` into owed minor units per participant.
pub fn split_expense(total: i64, spec: &Value) -> Result<Shares> {
    check_id_lists(spec)?;
    match spec.get("type").and_then(Value::as_str) {
        Some("equal") => equal(total, spec),
        Some("exact") => exact(total, spec),
        Some("percentage") => percentage(total, spec),
        Some("shares") => shares(total, spec),
        Some("itemized") => itemized(total, spec),
        other => Err(SplitError::new(
            code::BILL_UNKNOWN_SPLIT_TYPE,
            format!("No such split method: {other:?}"),
        )),
    }
}

fn equal(total: i64, spec: &Value) -> Result<Shares> {
    let among = unique_sorted_utf8(
        id_list(spec.get("among").unwrap_or(&Value::Null))
            .iter()
            .map(String::as_str),
    );
    if among.is_empty() {
        return Err(SplitError::new(
            code::EMPTY_SPLIT,
            "An equal split names nobody",
        ));
    }
    Ok(zip(&among, allocate_evenly(total, among.len())?))
}

fn exact(total: i64, spec: &Value) -> Result<Shares> {
    let amounts = int_map(spec.get("amounts").unwrap_or(&Value::Null))?;
    let ids = sorted_utf8(amounts.keys().map(String::as_str));
    for id in &ids {
        if against(amounts[id], total) {
            return Err(SplitError::new(
                code::NEGATIVE_SHARE,
                format!("{id} is given a negative share"),
            ));
        }
    }
    let sum = checked_sum(ids.iter().map(|i| amounts[i]), code::AMOUNT_OVERFLOW)?;
    if sum != total {
        return Err(SplitError::new(
            code::EXACT_TOTAL_MISMATCH,
            format!("The stated shares sum to {sum}, not {total}"),
        ));
    }
    Ok(ids.iter().map(|i| (i.clone(), amounts[i])).collect())
}

fn percentage(total: i64, spec: &Value) -> Result<Shares> {
    let points = int_map(spec.get("basisPoints").unwrap_or(&Value::Null))?;
    let ids = sorted_utf8(points.keys().map(String::as_str));
    for id in &ids {
        if points[id] < 0 {
            return Err(SplitError::new(
                code::NEGATIVE_WEIGHT,
                format!("{id} is given negative basis points"),
            ));
        }
    }
    // The overflow check precedes the full-scale check: four values of 2^62
    // sum to zero in a 64-bit integer, and a fifth of 10000 then satisfies it.
    let sum = checked_sum(ids.iter().map(|i| points[i]), code::AMOUNT_OVERFLOW)?;
    if sum != 10_000 {
        return Err(SplitError::new(
            code::PERCENTAGE_NOT_FULL_SCALE,
            format!("Basis points sum to {sum}, not 10000"),
        ));
    }
    let weights: Vec<i64> = ids.iter().map(|i| points[i]).collect();
    Ok(zip(&ids, allocate(total, &weights)?))
}

fn shares(total: i64, spec: &Value) -> Result<Shares> {
    let counts = int_map(spec.get("shareCounts").unwrap_or(&Value::Null))?;
    let ids = sorted_utf8(counts.keys().map(String::as_str));
    let weights: Vec<i64> = ids.iter().map(|i| counts[i]).collect();
    Ok(zip(&ids, allocate(total, &weights)?))
}

fn itemized(total: i64, spec: &Value) -> Result<Shares> {
    let items = spec.get("items").and_then(Value::as_array);
    let items = match items {
        Some(list) if !list.is_empty() => list,
        _ => {
            return Err(SplitError::new(
                code::ITEMIZED_NO_ITEMS,
                "An itemised split lists no items",
            ))
        }
    };

    for item in items {
        // §4.5: an item is an object. Stated rather than left to `get`
        // returning None, so all three refuse one input with one code.
        if !item.is_object() {
            return Err(SplitError::new(
                code::BILL_TYPE_ERROR,
                "An item is an object",
            ));
        }
        if id_list(item.get("sharedBy").unwrap_or(&Value::Null)).is_empty() {
            return Err(SplitError::new(
                code::ITEMIZED_UNASSIGNED_ITEM,
                "An item is assigned to nobody",
            ));
        }
    }

    let extra = match spec.get("extraMinorUnits") {
        None | Some(Value::Null) => 0,
        Some(v) => integer(v)?,
    };
    if against(extra, total) {
        return Err(SplitError::new(
            code::NEGATIVE_SHARE,
            format!("An extra of {extra} pulls against a total of {total}"),
        ));
    }
    for item in items {
        if against(
            integer(item.get("minorUnits").unwrap_or(&Value::Null))?,
            total,
        ) {
            return Err(SplitError::new(
                code::NEGATIVE_SHARE,
                "An item costs less than nothing",
            ));
        }
    }

    // These checks run in the order §4.5 states, so an input failing two of
    // them is refused with the same code everywhere.
    let mut totals: Vec<i64> = Vec::with_capacity(items.len() + 1);
    for item in items {
        totals.push(integer(item.get("minorUnits").unwrap_or(&Value::Null))?);
    }
    totals.push(extra);
    let stated = checked_sum(totals, code::AMOUNT_OVERFLOW)?;
    if stated != total {
        return Err(SplitError::new(
            code::ITEMIZED_TOTAL_MISMATCH,
            format!("Items and extra sum to {stated}, not {total}"),
        ));
    }

    let mut subtotal: BTreeMap<String, i64> = BTreeMap::new();
    for item in items {
        let who = unique_sorted_utf8(
            id_list(item.get("sharedBy").unwrap_or(&Value::Null))
                .iter()
                .map(String::as_str),
        );
        let cost = integer(item.get("minorUnits").unwrap_or(&Value::Null))?;
        let parts = allocate_evenly(cost, who.len())?;
        for (id, part) in who.iter().zip(parts) {
            *subtotal.entry(id.clone()).or_insert(0) += part;
        }
    }

    let ids = sorted_utf8(subtotal.keys().map(String::as_str));
    if extra != 0 {
        // Magnitudes: §3.1 refuses a negative weight and a refund's subtotals
        // are all negative. The proportion is the same either way.
        let weights: Vec<i64> = ids.iter().map(|i| subtotal[i].abs()).collect();
        // Weights that are all zero have no proportion to preserve.
        let extras = if weights.iter().any(|w| *w != 0) {
            allocate(extra, &weights)?
        } else {
            allocate_evenly(extra, ids.len())?
        };
        for (id, part) in ids.iter().zip(extras) {
            *subtotal.get_mut(id).expect("id came from subtotal") += part;
        }
    }
    Ok(ids.iter().map(|i| (i.clone(), subtotal[i])).collect())
}
