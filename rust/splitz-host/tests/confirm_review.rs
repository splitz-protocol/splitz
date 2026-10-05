//! What a payee is warned about before confirming a payment (§14.7).

use std::collections::{BTreeMap, BTreeSet};

use splitz_core::host::FoldedBill;
use splitz_core::{Bill, ExchangeRate, Identities, PaymentRecord};
use splitz_host::{concerns_before_confirming, PaymentConcern};

fn eur(per: i64) -> ExchangeRate {
    ExchangeRate {
        currency: "EUR".into(),
        minor_units_per_zec: per,
        at: "2026-10-28T19:30:00.000Z".into(),
        source: None,
    }
}

fn bill(rate_by: &str) -> FoldedBill {
    FoldedBill {
        bill: Bill {
            id: "b".into(),
            name: "Dinner".into(),
            currency: "EUR".into(),
            split_mode: "equal".into(),
            participants: vec![],
            expenses: vec![],
            payments: vec![],
            confirmed_payments: BTreeSet::new(),
            rate: Some(eur(100_000)),
        },
        creator_id: "ana".into(),
        set_aside: vec![],
        withdrawn: vec![],
        replaced_addresses: vec![],
        identities: Identities::default(),
        payment_authors: BTreeMap::new(),
        payment_digests: BTreeMap::new(),
        expense_entries: BTreeMap::new(),
        expense_authors: BTreeMap::new(),
        payment_entries: BTreeMap::new(),
        rate_entry: None,
        rate_author: Some(rate_by.into()),
        in_force: vec![],
        amendment_of: BTreeMap::new(),
    }
}

fn paid(at: Option<ExchangeRate>) -> PaymentRecord {
    PaymentRecord {
        id: "ben:p1".into(),
        from: "ben".into(),
        to: "ana".into(),
        amount: 1000,
        currency: "EUR".into(),
        method: "shieldedZec".into(),
        at: "2026-10-28T19:40:00.000Z".into(),
        zatoshi: Some(1_000_000),
        paid_at_rate: at,
        reference: None,
        note: None,
    }
}

#[test]
fn an_honest_payment_raises_nothing() {
    let b = bill("ana");
    assert!(concerns_before_confirming(&paid(Some(eur(100_000))), &b, None).is_empty());
    assert!(concerns_before_confirming(&paid(Some(eur(100_000))), &b, Some(104_000)).is_empty());
}

#[test]
fn each_concern_alone_and_all_at_once_in_order() {
    use PaymentConcern::*;
    assert_eq!(
        concerns_before_confirming(&paid(None), &bill("ben"), None),
        vec![RateSetByPayer]
    );
    assert_eq!(
        concerns_before_confirming(&paid(Some(eur(90_000))), &bill("ana"), None),
        vec![PricedAtAnotherRate]
    );
    for live in [105_264, 95_238] {
        assert_eq!(
            concerns_before_confirming(&paid(None), &bill("ana"), Some(live)),
            vec![RateFarFromLive],
            "{live}"
        );
    }
    assert_eq!(
        concerns_before_confirming(&paid(Some(eur(130_000))), &bill("ben"), Some(100_000)),
        vec![RateSetByPayer, PricedAtAnotherRate, RateFarFromLive]
    );
}

#[test]
fn the_creator_prices_a_bill_only_they_have_not_priced() {
    use splitz_host::creator_rate_missing;
    assert!(creator_rate_missing(&bill("ben"), "ana"));
    let mut unpriced = bill("ana");
    unpriced.rate_author = None;
    assert!(creator_rate_missing(&unpriced, "ana"));
    assert!(!creator_rate_missing(&bill("ana"), "ana"));
    assert!(!creator_rate_missing(&bill("ben"), "ben"));
}
