//! §14.2's review-screen check, as a wallet on the binding runs it.

use std::collections::HashMap;

use splitz_ffi::{
    add_expense_entry, check_payer_review, create_bill_entry, identity_key_from_seed,
    join_bill_entry, merge_entries, obligation_of, obligation_via, participant_id_for_key,
    rate_figure, render_amount, set_rate_entry, HostFacts, PayerObligation, Payout, ReviewFinding,
    ReviewRule, SplitzError,
};

struct Device {
    seed: String,
    key: String,
    me: String,
    minute: std::cell::Cell<u32>,
}

impl Device {
    fn new(byte: u8) -> Self {
        let seed = splitz_host::base64url_encode(&[byte; 32]);
        let key = identity_key_from_seed(seed.clone()).unwrap();
        let me = participant_id_for_key(key.clone()).unwrap();
        Self {
            seed,
            key,
            me,
            minute: std::cell::Cell::new(0),
        }
    }

    fn facts(&self) -> HostFacts {
        self.minute.set(self.minute.get() + 1);
        let total = 19 * 60 + 30 + self.minute.get();
        HostFacts {
            me: self.me.clone(),
            now: format!("2026-10-28T{:02}:{:02}:00.000Z", total / 60, total % 60),
            nonce: vec![self.seed.as_bytes()[0]; 16],
        }
    }
}

/// Ben owes Ana 45.00 of a 90.00 dinner and Cal, who published no address,
/// 15.00 of a 30.00 taxi, at 3000.00 EUR a ZEC that Ana set.
fn owing() -> (Device, String, Vec<String>, PayerObligation) {
    let (ben, bill_id, entries) = bill(vec![]);
    let owed = obligation_of(ben.facts(), bill_id.clone(), entries.clone())
        .unwrap()
        .expect("priced");
    (ben, bill_id, entries, owed)
}

/// The same bill, with Cal declaring `cal_payouts`. Returns Ben's device.
fn bill(cal_payouts: Vec<Payout>) -> (Device, String, Vec<String>) {
    let ana = Device::new(1);
    let ben = Device::new(90);
    let cal = Device::new(40);
    let create = create_bill_entry(
        ana.facts(),
        "Dinner".into(),
        "EUR".into(),
        "equal".into(),
        ana.key.clone(),
        ana.seed.clone(),
    )
    .unwrap();
    let bill_id = serde_json::from_str::<serde_json::Value>(&create).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let join = |d: &Device, name: &str, pay_to: Option<&str>, payouts: Vec<Payout>| {
        join_bill_entry(
            d.facts(),
            bill_id.clone(),
            Some(name.into()),
            pay_to.map(str::to_owned),
            Some(d.key.clone()),
            payouts,
            d.seed.clone(),
        )
        .unwrap()
    };
    let expense = |d: &Device, id: &str, amount: i64, among: [&str; 2]| {
        add_expense_entry(
            d.facts(),
            bill_id.clone(),
            id.into(),
            d.me.clone(),
            amount,
            format!(
                r#"{{"type":"equal","among":["{}","{}"]}}"#,
                among[0], among[1]
            ),
            None,
            d.seed.clone(),
        )
        .unwrap()
    };
    let entries = vec![
        create.clone(),
        join(&ana, "Ana", Some("u1ana"), vec![]),
        join(&ben, "Ben", Some("u1ben"), vec![]),
        join(&cal, "Cal", None, cal_payouts),
        expense(&ana, "x1", 9000, [&ana.me, &ben.me]),
        expense(&cal, "x2", 3000, [&ben.me, &cal.me]),
        set_rate_entry(
            ana.facts(),
            bill_id.clone(),
            "EUR".into(),
            300_000,
            None,
            ana.seed.clone(),
        )
        .unwrap(),
    ];
    let entries = merge_entries(vec![], entries).unwrap().entries;
    (ben, bill_id, entries)
}

/// What a screen that shows every §14.2 fact for Ben's send says.
fn full_screen() -> Vec<String> {
    vec![
        "Pay Ana 0.015 ZEC".into(),
        "to u1ana".into(),
        "at 3000.00 EUR per ZEC, set by Ana".into(),
        "Not in this payment: Cal — has not published an address".into(),
    ]
}

fn words() -> HashMap<String, String> {
    HashMap::from([(
        "no_address".to_owned(),
        "has not published an address".to_owned(),
    )])
}

#[test]
fn the_figures_are_the_ones_the_screen_shows() {
    let (_, _, _, owed) = owing();
    assert_eq!(rate_figure(owed.rate.clone()), "3000.00");
    assert_eq!(owed.request.payments.len(), 1);
    assert_eq!(
        render_amount(owed.request.payments[0].zatoshi).unwrap(),
        "0.015"
    );
    assert_eq!(owed.request.unpayable.len(), 1);
    assert_eq!(owed.request.unpayable[0].reason, "no_address");
    match render_amount(0) {
        Err(SplitzError::Protocol { code, .. }) => assert_eq!(code, "zip321_amount_not_positive"),
        other => panic!("expected a protocol refusal, got {other:?}"),
    }
}

#[test]
fn a_screen_that_shows_every_fact_passes_and_one_missing_is_named() {
    let (ben, bill_id, entries, owed) = owing();
    let review = |text: Vec<String>, words: HashMap<String, String>| {
        check_payer_review(
            ben.facts(),
            bill_id.clone(),
            entries.clone(),
            owed.clone(),
            text,
            words,
            HashMap::new(),
            String::new(),
        )
        .unwrap()
    };
    assert_eq!(review(full_screen(), words()), vec![]);

    let mut no_address = full_screen();
    no_address[1] = "to your contact".into();
    assert_eq!(
        review(no_address, words()),
        vec![ReviewFinding {
            rule: ReviewRule::Output,
            fact: "the address Ana is paid at".into(),
            expected: "u1ana".into(),
        }]
    );

    assert_eq!(
        review(full_screen(), HashMap::new()),
        vec![ReviewFinding {
            rule: ReviewRule::Unpayable,
            fact: "why Cal cannot be paid (no_address)".into(),
            expected: "no_address".into(),
        }]
    );

    let mut no_rate = full_screen();
    no_rate[2] = "at 3000.001 EUR per ZEC, set by Ana".into();
    assert_eq!(
        review(no_rate, words())
            .iter()
            .map(|f| (f.rule, f.expected.as_str()))
            .collect::<Vec<_>>(),
        vec![(ReviewRule::Rate, "3000.00")]
    );
}

#[test]
fn an_obligation_the_binding_did_not_write_is_refused() {
    let (ben, bill_id, entries, owed) = owing();
    let review = |entries: Vec<String>, owed: PayerObligation| {
        check_payer_review(
            ben.facts(),
            bill_id.clone(),
            entries,
            owed,
            full_screen(),
            words(),
            HashMap::new(),
            String::new(),
        )
    };
    let host = |r: Result<Vec<ReviewFinding>, SplitzError>| match r {
        Err(SplitzError::Host { detail, .. }) => detail,
        other => panic!("expected a Host error, got {other:?}"),
    };

    assert!(host(review(vec!["{not json".into()], owed.clone())).contains("not JSON"));

    let mut other_uri = owed.clone();
    other_uri.request.uri = Some(format!("{}&x=1", owed.request.uri.clone().unwrap()));
    match review(entries.clone(), other_uri) {
        Err(SplitzError::Protocol { code, .. }) => assert_eq!(code, "zip321_not_canonical"),
        other => panic!("expected zip321_not_canonical, got {other:?}"),
    }

    let mut other_amount = owed.clone();
    other_amount.request.payments[0].zatoshi += 1;
    assert!(host(review(entries.clone(), other_amount)).contains("not the ones its request"));

    let mut dropped = owed.clone();
    dropped.request.payments.clear();
    assert!(host(review(entries.clone(), dropped)).contains("not the ones its request"));

    let mut other_reason = owed.clone();
    other_reason.request.unpayable[0].reason = "asleep".into();
    assert!(host(review(entries, other_reason)).contains("not a reason §8.5 gives"));
}

/// Cal asks for cash first and declares an address second; Ben, far away,
/// pays Cal's share to the address.
#[test]
fn a_recipient_paid_by_a_lower_preference_is_checked() {
    let payout = |kind: &str, address: Option<&str>| Payout {
        kind: kind.into(),
        address: address.map(str::to_owned),
        asset: None,
        chain: None,
    };
    let (ben, bill_id, entries) = bill(vec![payout("cash", None), payout("zec", Some("u1cal"))]);
    let cal = participant_id_for_key(
        identity_key_from_seed(splitz_host::base64url_encode(&[40; 32])).unwrap(),
    )
    .unwrap();
    let via = HashMap::from([(cal.clone(), 1)]);

    let first = obligation_of(ben.facts(), bill_id.clone(), entries.clone())
        .unwrap()
        .unwrap();
    assert_eq!(first.request.unpayable[0].reason, "payout_not_zec");
    let owed = obligation_via(ben.facts(), bill_id.clone(), entries.clone(), via.clone())
        .unwrap()
        .unwrap();
    assert!(owed.request.unpayable.is_empty());
    assert_eq!(
        render_amount(
            owed.request
                .payments
                .iter()
                .find(|p| p.to == cal)
                .unwrap()
                .zatoshi
        )
        .unwrap(),
        "0.005"
    );

    let screen: Vec<String> = [
        "Pay Ana 0.015 ZEC",
        "to u1ana",
        "Pay Cal 0.005 ZEC — by their second choice",
        "to u1cal",
        "at 3000.00 EUR per ZEC, set by Ana",
    ]
    .map(str::to_owned)
    .to_vec();
    let review = |text: Vec<String>, lower: &str| {
        check_payer_review(
            ben.facts(),
            bill_id.clone(),
            entries.clone(),
            owed.clone(),
            text,
            HashMap::new(),
            via.clone(),
            lower.to_owned(),
        )
        .unwrap()
    };
    assert_eq!(review(screen.clone(), "by their second choice"), vec![]);
    assert_eq!(
        review(screen.clone(), "paid another way"),
        vec![ReviewFinding {
            rule: ReviewRule::LowerPreference,
            fact: "that Cal is paid by a lower preference".into(),
            expected: "paid another way".into(),
        }]
    );

    match obligation_via(ben.facts(), bill_id, entries, HashMap::from([(cal, 2)])) {
        Err(SplitzError::Protocol { code, .. }) => assert_eq!(code, "payout_not_declared"),
        other => panic!("expected payout_not_declared, got {other:?}"),
    }
}
