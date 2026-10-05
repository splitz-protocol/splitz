//! The three ways a debt settles: a Zcash output, a swap off this chain, cash.
//!
//! §9.2 makes the method a label rather than a branch — the ledger arithmetic
//! is identical whichever happened — so what these assert is not different
//! money but different *routing*, and the different things a reader is
//! allowed to conclude from each.

use serde_json::{json, Value};
use std::cell::Cell;
use std::collections::{BTreeMap, BTreeSet};

use splitz_core::host::{
    add_expense, amend_entry, base64url_no_pad, create_bill, join_bill, lane_for, obligation_for,
    obligation_via, payment_id_for_send, record_payment, set_rate, settle, BillHost, BillLog,
    SendResult, Sent, SettleLane, SignEntry, VerifyEntry,
};
use splitz_core::model::Participant;
use splitz_core::net_balances;

// --- a wallet that does nothing ---------------------------------------------

struct FakeHost {
    me: String,
    counter: Cell<u8>,
}

impl FakeHost {
    fn new(me: &str) -> Self {
        Self {
            me: me.to_owned(),
            counter: Cell::new(0),
        }
    }
}

impl BillHost for FakeHost {
    fn me(&self) -> &str {
        &self.me
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
    three_lane_bill_with(ana, extra, vec![])
}

/// `three_lane_bill`, with `cara_also` declared after Cara's first payout.
fn three_lane_bill_with<'a>(
    ana: &'a FakeHost,
    extra: Vec<Value>,
    cara_also: Vec<Value>,
) -> BillLog<'a> {
    let mut cara = vec![json!({
        "type": "swap",
        "asset": "USDC",
        "chain": "base",
        "address": "0xcara"
    })];
    cara.extend(cara_also);
    let mut entries = vec![
        create_bill(ana, "Dinner", "USD", "equal", &fake_key("ana"), None).unwrap(),
        join_bill(ana, Some("Ana"), Some("u1ana"), None, None).unwrap(),
        join_bill(
            &FakeHost::new("ben"),
            Some("Ben"),
            None,
            None,
            Some(vec![json!({"type": "zec", "address": "u1ben"})]),
        )
        .unwrap(),
        join_bill(&FakeHost::new("cara"), Some("Cara"), None, None, Some(cara)).unwrap(),
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

// --- a payer may settle by a lower preference (§14.8) ----------------------

/// Cara ranks USDC on Base first and a Zcash address second.
fn cara_also_zec(ana: &FakeHost) -> BillLog<'_> {
    three_lane_bill_with(
        ana,
        vec![],
        vec![json!({"type": "zec", "address": "u1cara"})],
    )
}

#[test]
fn choosing_her_second_payout_carries_it_in_the_request() {
    let ana = FakeHost::new("ana");
    let log = cara_also_zec(&ana);
    let folded = log.fold().unwrap();
    let first = obligation_for(&ana, &folded).unwrap().unwrap();
    let via = BTreeMap::from([("cara".to_owned(), 1)]);
    let chosen = obligation_via(&ana, &folded, &via).unwrap().unwrap();

    // Who owes what does not move; only where Cara's share is sent does.
    assert_eq!(chosen.settlements, first.settlements);
    let labels = |o: &splitz_core::host::PayerObligation| -> Vec<String> {
        o.request
            .payments
            .iter()
            .map(|p| p.label.clone().unwrap_or_default())
            .collect()
    };
    assert_eq!(labels(&first), ["Ben"]);
    assert_eq!(labels(&chosen), ["Ben", "Cara"]);
    assert_eq!(chosen.request.payments[1].address, "u1cara");
    let unpayable: Vec<&str> = chosen
        .request
        .unpayable
        .iter()
        .map(|u| u.id.as_str())
        .collect();
    assert_eq!(unpayable, ["dan", "eve"]);
    assert_eq!(chosen.request.carried_minor_units, 2000);
}

#[test]
fn the_send_records_her_payment_like_any_other_and_her_order_stands() {
    let ana = FakeHost::new("ana");
    let mut log = cara_also_zec(&ana);
    let folded = log.fold().unwrap();
    let via = BTreeMap::from([("cara".to_owned(), 1)]);
    let owed = obligation_via(&ana, &folded, &via).unwrap().unwrap();
    let settled = settle(&ana, &mut log, &owed).unwrap();

    assert_eq!(settled.result, SendResult::Sent);
    let txid = settled.txid.clone().unwrap();
    let cara = settled
        .records
        .iter()
        .map(|r| &r["payment"])
        .find(|p| p["to"] == "cara")
        .unwrap();
    assert_eq!(cara["method"], "shieldedZec");
    assert_eq!(cara["id"], json!(payment_id_for_send("ana", &txid, "cara")));
    assert_eq!(cara["reference"], json!(txid));
    // No entry rewrote her preferences: every device still reads USDC first.
    let after = log.fold().unwrap();
    let kinds: Vec<&str> = after
        .bill
        .participant("cara")
        .unwrap()
        .payouts
        .iter()
        .map(|p| p.kind.as_str())
        .collect();
    assert_eq!(kinds, ["swap", "zec"]);
}

#[test]
fn a_payout_she_never_declared_is_refused() {
    let ana = FakeHost::new("ana");
    let log = cara_also_zec(&ana);
    let folded = log.fold().unwrap();
    let via = BTreeMap::from([("cara".to_owned(), 2)]);
    let refused = obligation_via(&ana, &folded, &via).unwrap_err();
    assert_eq!(refused.code, splitz_core::code::PAYOUT_NOT_DECLARED);
}

// --- a payout preference chooses a lane -------------------------------------

#[test]
fn each_of_the_four_payees_lands_in_the_lane_they_asked_for() {
    let ana = FakeHost::new("ana");
    let log = three_lane_bill(&ana, vec![]);
    let folded = log.fold().unwrap();
    let lane = |who: &str| lane_for(folded.bill.participant(who).unwrap());
    assert_eq!(lane("ben"), SettleLane::Zec);
    assert_eq!(lane("cara"), SettleLane::Swap);
    assert_eq!(lane("dan"), SettleLane::Cash);
    // Declared nothing and has no payTo: not a lane, a debt that cannot be
    // settled until she publishes somewhere.
    assert_eq!(lane("eve"), SettleLane::None);
}

#[test]
fn a_swap_debt_carries_the_asset_and_the_chain_never_one_alone() {
    let ana = FakeHost::new("ana");
    let log = three_lane_bill(&ana, vec![]);
    let folded = log.fold().unwrap();
    let cara = folded.bill.participant("cara").unwrap();
    assert_eq!(lane_for(cara), SettleLane::Swap);
    let swap = cara.payouts.first().unwrap();

    assert_eq!(swap.asset.as_deref(), Some("USDC"));
    assert_eq!(swap.chain.as_deref(), Some("base"));
    assert_eq!(swap.address.as_deref(), Some("0xcara"));
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
            splitz_core::model::Payout {
                kind: "cash".to_owned(),
                address: None,
                asset: None,
                chain: None,
            },
            splitz_core::model::Payout {
                kind: "zec".to_owned(),
                address: Some("u1cara".to_owned()),
                asset: None,
                chain: None,
            },
        ],
    };
    assert_eq!(lane_for(&cara), SettleLane::Cash);
}

#[test]
fn a_swap_payout_missing_its_address_asset_or_chain_is_nobody_to_pay() {
    let swap = |asset: Option<&str>, chain: Option<&str>, address: Option<&str>| {
        lane_for(&splitz_core::model::Participant {
            id: "cara".to_owned(),
            name: "Cara".to_owned(),
            pay_to: None,
            identity_key: None,
            payouts: vec![splitz_core::model::Payout {
                kind: "swap".to_owned(),
                address: address.map(str::to_owned),
                asset: asset.map(str::to_owned),
                chain: chain.map(str::to_owned),
            }],
        })
    };
    let (usdc, near) = (Some("USDC"), Some("near"));
    assert_eq!(swap(usdc, near, Some("")), SettleLane::None);
    assert_eq!(swap(usdc, near, Some("  ")), SettleLane::None);
    assert_eq!(swap(usdc, near, None), SettleLane::None);
    assert_eq!(swap(usdc, None, Some("cai.near")), SettleLane::None);
    assert_eq!(swap(None, near, Some("cai.near")), SettleLane::None);
    assert_eq!(swap(usdc, near, Some("cai.near")), SettleLane::Swap);
}

// --- the request carries the zec lane and reports the rest (§8.5) -----------

#[test]
fn only_ben_is_in_the_uri_and_the_other_three_are_reported() {
    let ana = FakeHost::new("ana");
    let log = three_lane_bill(&ana, vec![]);
    let folded = log.fold().unwrap();
    let owed = obligation_for(&ana, &folded).unwrap().unwrap();

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
    let ana = FakeHost::new("ana");
    let mut log = three_lane_bill(&ana, vec![]);
    let folded = log.fold().unwrap();
    let owed = obligation_for(&ana, &folded).unwrap().unwrap();

    let settled = settle(&ana, &mut log, &owed).unwrap();
    assert_eq!(settled.result, SendResult::Sent);
    assert_eq!(settled.records.len(), 1);
    let payment = &settled.records[0]["payment"];
    assert_eq!(payment["to"], "ben");
    assert_eq!(payment["method"], "shieldedZec");
    let txid = settled.txid.clone().unwrap();
    assert_eq!(
        payment["id"],
        json!(payment_id_for_send("ana", &txid, "ben"))
    );
    // §10.5: the record carries its own id and the transaction is the
    // reference, which is what an `onChain` confirmation is checked against.
    assert_eq!(payment["reference"], json!(txid));
}

#[test]
fn a_cash_settlement_records_cash_sends_nothing_and_is_folded() {
    let ana = FakeHost::new("ana");
    let mut log = three_lane_bill(&ana, vec![]);
    let record = record_payment(
        &ana,
        "cash-dan-1",
        "dan",
        1000,
        "cash",
        None,
        None,
        None,
        Some("handed over at the table"),
    )
    .unwrap();
    log.add(vec![record.clone()]).unwrap();

    let payment = &record["payment"];
    assert_eq!(payment["method"], "cash");
    assert_eq!(payment["note"], "handed over at the table");
    // No transaction exists, so no reference pretends one does.
    assert!(payment.get("reference").is_none());

    // It is on the bill, and §10.5 has not settled it: a record is a claim.
    let folded = log.fold().unwrap();
    assert!(folded
        .bill
        .payments
        .iter()
        .any(|p| p.id == "ana:cash-dan-1"));
    assert!(!folded.bill.confirmed_payments.contains("ana:cash-dan-1"));
}

#[test]
fn a_swap_settlement_records_the_intent_id_not_a_txid() {
    let ana = FakeHost::new("ana");
    let mut log = three_lane_bill(&ana, vec![]);
    // The swap's own identifier is the payment id as well as the reference:
    // it is what a reader checks the record against.
    let record = record_payment(
        &ana,
        "near-intent-7f3a",
        "cara",
        1000,
        "swap",
        Some("near-intent-7f3a"),
        Some(1000000),
        None,
        Some("USDC on base"),
    )
    .unwrap();
    log.add(vec![record.clone()]).unwrap();

    let payment = &record["payment"];
    assert_eq!(payment["method"], "swap");
    // §9.2: the reference identifies the swap. A reader that renders it as a
    // Zcash transaction is wrong for every swap.
    assert_eq!(payment["reference"], "near-intent-7f3a");
    assert_eq!(payment["id"], "ana:near-intent-7f3a");
    // Verifiable only in half: the ZEC leg is recorded, the delivery is not.
    assert_eq!(payment["zatoshi"], 1000000);

    let folded = log.fold().unwrap();
    let paid = folded
        .bill
        .payments
        .iter()
        .find(|p| p.id == "ana:near-intent-7f3a")
        .unwrap();
    assert_eq!(paid.method, "swap");
    assert_eq!(paid.reference.as_deref(), Some("near-intent-7f3a"));
    assert_eq!(paid.zatoshi, Some(1000000));
    // The ZEC leg leaving is not the recipient being paid. Only Cara can say
    // that, and she has not.
    assert!(!folded
        .bill
        .confirmed_payments
        .contains("ana:near-intent-7f3a"));
}

#[test]
fn all_three_methods_coexist_on_one_bill_and_net_the_same_way() {
    let ana = FakeHost::new("ana");
    let mut log = three_lane_bill(&ana, vec![]);
    let cash = record_payment(
        &ana,
        "cash-dan-1",
        "dan",
        1000,
        "cash",
        None,
        None,
        None,
        None,
    )
    .unwrap();
    let swap = record_payment(
        &ana,
        "near-intent-7f3a",
        "cara",
        1000,
        "swap",
        Some("near-intent-7f3a"),
        None,
        None,
        None,
    )
    .unwrap();
    log.add(vec![cash, swap]).unwrap();

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
    let ana = FakeHost::new("ana");
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
    assert!(!folded.bill.payments.iter().any(|p| p.id == "ana:p1"));
}

#[test]
fn a_payout_nobody_could_be_paid_by_is_not_written() {
    let ana = FakeHost::new("ana");
    let join = |payouts: Vec<Value>| join_bill(&ana, Some("Ana"), None, None, Some(payouts));
    for bad in [
        json!({"type": "zec"}),
        json!({"type": "zec", "address": "  "}),
        json!({"type": "swap", "asset": "USDC", "address": "0xa"}),
        json!({"type": "swap", "asset": "", "chain": "base", "address": "0xa"}),
        json!({"type": "swap", "asset": "USDC", "chain": "base", "address": 7}),
    ] {
        let refused = join(vec![json!({"type": "cash"}), bad.clone()]).unwrap_err();
        assert_eq!(refused.code, "payout_incomplete", "{bad}");
    }
    let honest = vec![
        json!({"type": "zec", "address": "u1ana"}),
        json!({"type": "swap", "asset": "USDC", "chain": "base", "address": "0xa"}),
        json!({"type": "cash"}),
    ];
    assert_eq!(
        join(honest.clone()).unwrap()["participant"]["payouts"],
        Value::Array(honest)
    );
    // A type §9.1 does not define is left for every reader to refuse.
    assert!(join(vec![json!({"type": "venmo"})]).is_ok());
}

#[test]
fn a_payment_of_nothing_or_a_swap_naming_none_is_not_written() {
    let ana = FakeHost::new("ana");
    let write = |amount: i64, method: &str, reference: Option<&str>| {
        record_payment(
            &ana, "p", "ben", amount, method, reference, None, None, None,
        )
    };
    for amount in [0, -1] {
        assert_eq!(
            write(amount, "cash", None).unwrap_err().code,
            "payment_not_positive"
        );
    }
    for reference in [None, Some(""), Some("  ")] {
        assert_eq!(
            write(100, "swap", reference).unwrap_err().code,
            "swap_missing_reference",
            "{reference:?}"
        );
    }
    // One unit, a swap that names its intent, and cash naming nothing are all
    // honest records.
    assert!(write(1, "cash", None).is_ok());
    assert!(write(100, "swap", Some("intent-1")).is_ok());
}

#[test]
fn a_payment_to_oneself_is_set_aside_whichever_method_it_claims() {
    for method in ["shieldedZec", "swap", "cash"] {
        let ana = FakeHost::new("ana");
        let mut log = three_lane_bill(&ana, vec![]);
        log.add(vec![record_payment(
            &ana,
            &format!("p-{method}"),
            "ana",
            100,
            method,
            Some(&format!("r-{method}")),
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
    let ana = FakeHost::new("ana");
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
    let ana = FakeHost::new("ana");
    let mut log = BillLog::new(&ana);
    log.add(vec![
        create_bill(&ana, "D", "USD", "equal", &fake_key("ana"), None).unwrap(),
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
    let owed = obligation_for(&ana, &folded).unwrap().unwrap();
    assert!(owed.uri().is_none());

    let settled = settle(&ana, &mut log, &owed).unwrap();
    assert_eq!(settled.result, SendResult::Failed);
    assert!(settled.records.is_empty());
    assert!(log.fold().unwrap().bill.payments.is_empty());
}

#[test]
fn an_amendment_may_not_write_what_the_record_may_not() {
    let ana = FakeHost::new("ana");
    let amend = |member: &str, payload: Value| amend_entry(&ana, "t", member, payload);
    assert_eq!(
        amend(
            "payment",
            json!({"id": "ana:p1", "amount": 0, "method": "cash"})
        )
        .unwrap_err()
        .code,
        "payment_not_positive"
    );
    assert_eq!(
        amend(
            "payment",
            json!({"id": "ana:p1", "amount": 5, "method": "swap"})
        )
        .unwrap_err()
        .code,
        "swap_missing_reference"
    );
    assert_eq!(
        amend(
            "participant",
            json!({"id": "ana", "payouts": [{"type": "zec", "address": ""}]})
        )
        .unwrap_err()
        .code,
        "payout_incomplete"
    );
    assert!(amend(
        "payment",
        json!({"id": "ana:p1", "amount": 5, "method": "cash"})
    )
    .is_ok());
    // A byte order mark is not white space, in any implementation.
    assert!(record_payment(
        &ana,
        "b",
        "ben",
        100,
        "swap",
        Some("\u{feff}"),
        None,
        None,
        None
    )
    .is_ok());
}
