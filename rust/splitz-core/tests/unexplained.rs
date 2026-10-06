//! §6.3: what a settlement's covers do not explain, and the refusal when its
//! figures leave the signed 64-bit range.

use splitz_core::{code, DirectDebt, Settlement};

fn settlement(amount: i64, covers: &[i64]) -> Settlement {
    Settlement {
        from: "ben".into(),
        to: "ana".into(),
        amount,
        covers: covers
            .iter()
            .map(|a| DirectDebt {
                from: "ben".into(),
                to: "cai".into(),
                amount: *a,
            })
            .collect(),
    }
}

#[test]
fn an_honest_settlement_is_explained_by_its_covers() {
    assert_eq!(settlement(1500, &[]).unexplained().unwrap(), 1500);
    assert_eq!(settlement(1500, &[1000, 500]).unexplained().unwrap(), 0);
}

#[test]
fn covers_summing_past_the_range_are_refused_not_wrapped() {
    let e = settlement(1500, &[i64::MAX, 1]).unexplained().unwrap_err();
    assert_eq!(e.code, code::AMOUNT_OVERFLOW);
}

#[test]
fn an_amount_less_its_covers_past_the_range_is_refused() {
    let e = settlement(i64::MIN, &[1]).unexplained().unwrap_err();
    assert_eq!(e.code, code::AMOUNT_OVERFLOW);
}
