//! §2.2 and §9.3 at entry ingress: what a number's text reads as.

use serde_json::Value;
use splitz_core::{canonical_json, check_entry, code, derive_entry_id};

/// An expense entry whose note is `note`, written as raw JSON text so the
/// number reaches the reader as a peer wrote it.
fn entry_with_note(note: &str) -> Value {
    let text = format!(
        r#"{{"v":1,"id":"x","author":"ana","kind":"addExpense","at":"2026-10-28T19:03:00.000Z","expense":{{"id":"x1","paidBy":"ana","amount":1,"at":"2026-10-28T19:03:00.000Z","split":{{"type":"equal","among":["ana"]}},"note":{note}}}}}"#
    );
    let mut entry: Value = serde_json::from_str(&text).expect("the text is JSON");
    // A number §9.3 cannot encode has no id to derive; §10.1 refuses it before
    // the id is read, so the written one stands.
    if let Ok(id) = derive_entry_id(&entry) {
        entry["id"] = Value::from(id);
    }
    entry
}

#[test]
fn minus_zero_is_the_integer_zero_every_reader_reads() {
    // Dart's and Python's readers take `-0` as the integer 0, so an entry a
    // peer wrote with it applies there. Read here as a float, it would be
    // refused, and one log would fold to two bills.
    let minus = entry_with_note("-0");
    let plain = entry_with_note("0");
    check_entry(&minus).expect("-0 is admitted as 0");
    assert_eq!(
        minus["id"], plain["id"],
        "one entry, one id, whichever way zero was spelled"
    );
    assert_eq!(
        canonical_json(&minus).unwrap(),
        canonical_json(&plain).unwrap()
    );
}

#[test]
fn a_fraction_and_a_number_past_64_bits_are_refused_by_what_they_are() {
    let refused = |note: &str| check_entry(&entry_with_note(note)).unwrap_err().code;
    assert_eq!(refused("-0.0"), code::CANONICAL_JSON_FLOAT);
    assert_eq!(refused("1.5"), code::CANONICAL_JSON_FLOAT);
    assert_eq!(refused("9223372036854775808"), code::AMOUNT_OVERFLOW);
    assert_eq!(refused("1e400"), code::AMOUNT_OVERFLOW);
    // The control: the largest 64-bit integer is an integer.
    check_entry(&entry_with_note("9223372036854775807")).expect("i64::MAX fits");
}
