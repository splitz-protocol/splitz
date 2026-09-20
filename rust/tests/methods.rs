//! The three ways a debt settles: a Zcash output, a swap off this chain, cash.
//!
//! §9.2 makes the method a label rather than a branch — the ledger arithmetic
//! is identical whichever happened — so what these assert is not different
//! money but different *routing*, and the different things a reader is
//! allowed to conclude from each.

use serde_json::{json, Value};
use std::cell::Cell;
use std::collections::BTreeSet;

use splitz::host::{
    add_expense, base64url_no_pad, create_bill, in_lane, join_bill, lane_debts, lane_for,
    obligation_for, record_payment, set_rate, settle, settle_cash, settle_swap, BillHost, BillLog,
    SendResult, Sent, SettleLane, SignEntry, VerifyEntry,
};
use splitz::model::Participant;
use splitz::{net_balances, settle_bill, DEFAULT_EXACT_LIMIT};

// --- a wallet that does nothing ---------------------------------------------

struct FakeHost {
    me: String,
    pay_to: Option<String>,
    counter: Cell<u8>,
}

impl FakeHost {
    fn new(me: &str) -> Self {
        Self {
            me: me.to_owned(),
            pay_to: None,
            counter: Cell::new(0),
        }
    }

    fn paid_at(me: &str, pay_to: &str) -> Self {
        let mut h = Self::new(me);
        h.pay_to = Some(pay_to.to_owned());
        h
    }
}

impl BillHost for FakeHost {
    fn me(&self) -> &str {
        &self.me
    }
    fn pay_to_address(&self) -> Option<&str> {
        self.pay_to.as_deref()
    }
    fn now(&self) -> String {
        "2026-10-28T19:30:00.000Z".to_owned()
    }
    fn random_bytes(&self, byte_count: usize) -> Vec<u8> {
        self.counter.set(self.counter.get().wrapping_add(1));
        (0..byte_count)
            .map(|i| self.counter.get().wrapping_add(i as u8))
            .collect()
    }
    fn broadcast(&self, uri: &str) -> Sent {
        Sent::sent(format!("tx-{}", uri.len()))
    }
    fn signer(&self) -> Option<SignEntry<'_>> {
        None
    }
    fn verifier(&self) -> Option<VerifyEntry<'_>> {
        None
    }
}

fn fake_key(who: &str) -> String {
    let seed = who.as_bytes()[0];
    base64url_no_pad(&(0..32).map(|i| seed.wrapping_add(i)).collect::<Vec<u8>>())
}

/// A bill where each payee declares a different payout preference.
///
/// Ana owes everybody: Ben wants ZEC, Cara wants USDC on Base, Dan wants cash,
/// and Eve has declared nothing at all.
fn three_lane_bill<'a>(ana: &'a FakeHost, extra: Vec<Value>) -> BillLog<'a> {
    let mut entries = vec![
        create_bill(ana, "Dinner", "USD", "equal", &fake_key("ana")).unwrap(),
        join_bill(ana, Some("Ana"), Some("u1ana"), None, None).unwrap(),
        join_bill(
            &FakeHost::new("ben"),
            Some("Ben"),
            None,
            None,
            Some(vec![json!({"type": "zec", "address": "u1ben"})]),
        )
        .unwrap(),
        join_bill(
            &FakeHost::new("cara"),
            Some("Cara"),
            None,
            None,
            Some(vec![json!({
                "type": "swap",
                "asset": "USDC",
                "chain": "base",
                "address": "0xcara"
            })]),
        )
        .unwrap(),
        join_bill(
            &FakeHost::new("dan"),
            Some("Dan"),
            None,
            None,
            Some(vec![json!({"type": "cash"})]),
        )
        .unwrap(),
        join_bill(&FakeHost::new("eve"), Some("Eve"), None, None, None).unwrap(),
    ];
    // Ana owes each of the four 10.00, by having each of them cover a cost
    // split between the two of them.
    for who in ["ben", "cara", "dan", "eve"] {
        let mut among = vec!["ana", who];
        among.sort();
        entries.push(
            add_expense(
                &FakeHost::new(who),
                &format!("x-{who}"),
                who,
                2000,
                json!({"type": "equal", "among": among}),
                None,
            )
            .unwrap(),
        );
    }
    entries.push(set_rate(ana, "USD", 100000, Some("fixed for this test")).unwrap());
    entries.extend(extra);

    let mut log = BillLog::new(ana);
    log.add(entries).unwrap();
    log
}

// --- a payout preference chooses a lane -------------------------------------

#[test]
fn each_of_the_four_payees_lands_in_the_lane_they_asked_for() {
    let ana = FakeHost::paid_at("ana", "u1ana");
    let log = three_lane_bill(&ana, vec![]);
    let folded = log.fold().unwrap();
    let plan = settle_bill(&folded.bill, DEFAULT_EXACT_LIMIT).unwrap();
    let laned = lane_debts(&plan.settlements, &folded.bill).unwrap();

    let lane = |who: &str| laned.iter().find(|d| d.to == who).unwrap().lane;
    assert_eq!(lane("ben"), SettleLane::Zec);
    assert_eq!(lane("cara"), SettleLane::Swap);
    assert_eq!(lane("dan"), SettleLane::Cash);
    // Declared nothing and has no payTo: not a lane, a debt that cannot be
    // settled until she publishes somewhere.
    assert_eq!(lane("eve"), SettleLane::None);
}

#[test]
fn a_swap_debt_carries_the_asset_and_the_chain_never_one_alone() {
    let ana = FakeHost::paid_at("ana", "u1ana");
    let log = three_lane_bill(&ana, vec![]);
    let folded = log.fold().unwrap();
    let plan = settle_bill(&folded.bill, DEFAULT_EXACT_LIMIT).unwrap();
    let laned = lane_debts(&plan.settlements, &folded.bill).unwrap();
    let swap = in_lane(&laned, SettleLane::Swap);
    let swap = swap.first().unwrap();

    assert_eq!(swap.asset(), Some("USDC"));
    assert_eq!(swap.chain(), Some("base"));
    assert_eq!(swap.address(), Some("0xcara"));
}

#[test]
fn the_first_preference_decides_not_the_most_convenient_one() {
    // Cara ranks cash first and a Zcash address second. A reader that fell
    // through to the address because it is the one it can batch would pay her
    // somewhere she ranked lower.
    let cara = Participant {
        id: "cara".to_owned(),
        name: "Cara".to_owned(),
        pay_to: None,
        identity_key: None,
        payouts: vec![
            splitz::model::Payout {
                kind: "cash".to_owned(),
                address: None,
                asset: None,
                chain: None,
            },
            splitz::model::Payout {
                kind: "zec".to_owned(),
                address: Some("u1cara".to_owned()),
                asset: None,
                chain: None,
            },
        ],
    };
    assert_eq!(lane_for(&cara), SettleLane::Cash);
}

// --- the request carries the zec lane and reports the rest (§8.5) -----------

#[test]
fn only_ben_is_in_the_uri_and_the_other_three_are_reported() {
    let ana = FakeHost::paid_at("ana", "u1ana");
    let log = three_lane_bill(&ana, vec![]);
    let folded = log.fold().unwrap();
    let owed = obligation_for(&ana, &folded, &BTreeSet::new())
        .unwrap()
        .unwrap();

    // `settlements` is the whole debt this device owes; the URI carries only
    // what §8.5 could render.
    assert_eq!(owed.settlements.len(), 4);
    assert_eq!(owed.request.payments.len(), 1);
    assert!(owed.uri().unwrap().contains("u1ben"));
    assert!(!owed.uri().unwrap().contains("0xcara"));

    let reason = |who: &str| -> Option<&str> {
        owed.unpayable()
            .iter()
            .find(|u| u.id == who)
            .map(|u| u.reason)
    };
    // A swap and a cash payout are excluded for a reason that has nothing to
    // do with a missing address, and §8.5 requires the difference be reported
    // rather than flattened.
    assert_eq!(reason("cara"), Some("payout_not_zec"));
    assert_eq!(reason("dan"), Some("payout_not_zec"));
    assert_eq!(reason("eve"), Some("no_address"));

    assert_eq!(owed.carried_minor_units(), 1000);
    assert_eq!(owed.withheld_minor_units(), 3000);
    assert!(!owed.is_complete());
}

// --- recording what happened ------------------------------------------------

#[test]
fn a_settle_records_only_what_the_request_carried() {
    // Cara wants USDC and Dan wants cash, so §8.5 leaves them out of the URI.
    // Recording them as paid by that transaction claims it settled a debt it
    // never paid, and leaves them contesting a payment rather than simply
    // still being owed.
    let ana = FakeHost::paid_at("ana", "u1ana");
    let mut log = three_lane_bill(&ana, vec![]);
    let folded = log.fold().unwrap();
    let owed = obligation_for(&ana, &folded, &BTreeSet::new())
        .unwrap()
        .unwrap();

    let settled = settle(&ana, &mut log, &owed).unwrap();
    assert_eq!(settled.result, SendResult::Sent);
    assert_eq!(settled.records.len(), 1);
    let payment = &settled.records[0]["payment"];
    assert_eq!(payment["to"], "ben");
    assert_eq!(payment["method"], "shieldedZec");
    // Nothing off-chain happened, so nothing claims it did.
    assert!(payment.get("reference").is_none());
}

#[test]
fn a_cash_settlement_records_cash_sends_nothing_and_is_folded() {
    let ana = FakeHost::paid_at("ana", "u1ana");
    let mut log = three_lane_bill(&ana, vec![]);
    let record = settle_cash(
        &ana,
        &mut log,
        "cash-dan-1",
        "dan",
        1000,
        Some("handed over at the table"),
    )
    .unwrap();

    let payment = &record["payment"];
    assert_eq!(payment["method"], "cash");
    assert_eq!(payment["note"], "handed over at the table");
    // No transaction exists, so no reference pretends one does.
    assert!(payment.get("reference").is_none());

    // It is on the bill, and §10.5 has not settled it: a record is a claim.
    let folded = log.fold().unwrap();
    assert!(folded.bill.payments.iter().any(|p| p.id == "cash-dan-1"));
    assert!(!folded.bill.confirmed_payments.contains("cash-dan-1"));
}

#[test]
fn a_swap_settlement_records_the_intent_id_not_a_txid() {
    let ana = FakeHost::paid_at("ana", "u1ana");
    let mut log = three_lane_bill(&ana, vec![]);
    let record = settle_swap(
        &ana,
        &mut log,
        "near-intent-7f3a",
        "cara",
        1000,
        Some(1000000),
        None,
        Some("USDC on base"),
    )
    .unwrap();

    let payment = &record["payment"];
    assert_eq!(payment["method"], "swap");
    // §9.2: the reference identifies the swap. A reader that renders it as a
    // Zcash transaction is wrong for every swap.
    assert_eq!(payment["reference"], "near-intent-7f3a");
    assert_eq!(payment["id"], "near-intent-7f3a");
    // Verifiable only in half: the ZEC leg is recorded, the delivery is not.
    assert_eq!(payment["zatoshi"], 1000000);

    let folded = log.fold().unwrap();
    let paid = folded
        .bill
        .payments
        .iter()
        .find(|p| p.id == "near-intent-7f3a")
        .unwrap();
    assert_eq!(paid.method, "swap");
    assert_eq!(paid.reference.as_deref(), Some("near-intent-7f3a"));
    assert_eq!(paid.zatoshi, Some(1000000));
    // The ZEC leg leaving is not the recipient being paid. Only Cara can say
    // that, and she has not.
    assert!(!folded.bill.confirmed_payments.contains("near-intent-7f3a"));
}

#[test]
fn all_three_methods_coexist_on_one_bill_and_net_the_same_way() {
    let ana = FakeHost::paid_at("ana", "u1ana");
    let mut log = three_lane_bill(&ana, vec![]);
    settle_cash(&ana, &mut log, "cash-dan-1", "dan", 1000, None).unwrap();
    settle_swap(
        &ana,
        &mut log,
        "near-intent-7f3a",
        "cara",
        1000,
        None,
        None,
        None,
    )
    .unwrap();

    let folded = log.fold().unwrap();
    assert!(folded.set_aside.is_empty());
    let methods: BTreeSet<&str> = folded
        .bill
        .payments
        .iter()
        .map(|p| p.method.as_str())
        .collect();
    assert_eq!(methods, BTreeSet::from(["cash", "swap"]));

    // §9.2: a label, not a branch. Neither payment has moved a balance,
    // because neither is confirmed — the arithmetic does not care which
    // method it was.
    let debts = net_balances(&folded.bill).unwrap();
    assert_eq!(debts.get("ana"), Some(&-4000));
}

// --- what these paths refuse ------------------------------------------------

#[test]
fn a_method_the_protocol_does_not_define_is_set_aside_at_the_fold() {
    // §10.1 admits the entry — it is well formed — and §10.3 sets it aside
    // when the bill is read. The debt stays owed rather than the whole log
    // becoming unreadable because one peer invented a method.
    let ana = FakeHost::paid_at("ana", "u1ana");
    let mut log = three_lane_bill(&ana, vec![]);
    log.add(vec![record_payment(
        &ana, "p1", "ben", 100, "venmo", None, None, None, None,
    )
    .unwrap()])
        .unwrap();

    let folded = log.fold().unwrap();
    assert!(folded
        .set_aside
        .iter()
        .any(|s| s.code == "bill_unknown_settlement_method"));
    assert!(!folded.bill.payments.iter().any(|p| p.id == "p1"));
}

#[test]
fn a_payment_to_oneself_is_set_aside_whichever_method_it_claims() {
    for method in ["shieldedZec", "swap", "cash"] {
        let ana = FakeHost::paid_at("ana", "u1ana");
        let mut log = three_lane_bill(&ana, vec![]);
        log.add(vec![record_payment(
            &ana,
            &format!("p-{method}"),
            "ana",
            100,
            method,
            None,
            None,
            None,
            None,
        )
        .unwrap()])
            .unwrap();

        let folded = log.fold().unwrap();
        assert!(
            folded.set_aside.iter().any(|s| s.code == "self_payment"),
            "a {method} payment to oneself pads a settlement history"
        );
    }
}

#[test]
fn a_zatoshi_leg_of_zero_is_set_aside() {
    // A swap that sent nothing is not a swap. §9.2 makes zatoshi advisory,
    // which is not the same as unchecked.
    let ana = FakeHost::paid_at("ana", "u1ana");
    let mut log = three_lane_bill(&ana, vec![]);
    log.add(vec![record_payment(
        &ana,
        "p1",
        "ben",
        100,
        "swap",
        Some("p1"),
        Some(0),
        None,
        None,
    )
    .unwrap()])
        .unwrap();

    let folded = log.fold().unwrap();
    assert!(folded.set_aside.iter().any(|s| s.code == "negative_amount"));
}

#[test]
fn a_request_that_can_carry_nothing_sends_nothing() {
    // Everybody on this bill wants cash. There is no URI to broadcast, and
    // nothing may be recorded as sent.
    let ana = FakeHost::paid_at("ana", "u1ana");
    let mut log = BillLog::new(&ana);
    log.add(vec![
        create_bill(&ana, "D", "USD", "equal", &fake_key("ana")).unwrap(),
        join_bill(&ana, Some("Ana"), Some("u1ana"), None, None).unwrap(),
        join_bill(
            &FakeHost::new("dan"),
            Some("Dan"),
            None,
            None,
            Some(vec![json!({"type": "cash"})]),
        )
        .unwrap(),
        add_expense(
            &FakeHost::new("dan"),
            "x-dan",
            "dan",
            2000,
            json!({"type": "equal", "among": ["ana", "dan"]}),
            None,
        )
        .unwrap(),
        set_rate(&ana, "USD", 100000, None).unwrap(),
    ])
    .unwrap();

    let folded = log.fold().unwrap();
    let owed = obligation_for(&ana, &folded, &BTreeSet::new())
        .unwrap()
        .unwrap();
    assert!(owed.uri().is_none());

    let settled = settle(&ana, &mut log, &owed).unwrap();
    assert_eq!(settled.result, SendResult::Failed);
    assert!(settled.records.is_empty());
    assert!(log.fold().unwrap().bill.payments.is_empty());
}
