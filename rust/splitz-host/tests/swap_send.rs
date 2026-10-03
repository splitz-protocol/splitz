//! §15.7's two swap rules: a deposit checked against the bill as stored
//! before it is sent, and a failed swap's record withdrawn.

use std::cell::Cell;
use std::collections::BTreeMap;

use serde_json::json;
use splitz_core::host::{
    add_expense, authored_id, base64url_no_pad, confirm_payment, create_bill, join_bill,
    obligation_via, record_payment, set_rate, BillHost, BillLog, FoldedBill, PayerObligation, Sent,
    CREATOR_KEY_BYTES,
};
use splitz_core::{fiat_to_zatoshi, ExchangeRate, Payout, RateRounding};
use splitz_host::{
    declared_payout_index, failed_swap_withdrawals, swap_send_refusal, SwapQuote, SwapSendRefusal,
    TradableAsset,
};

const BEN_BASE: &str = "0xben000000000000000000000000000000000000";
const BEN_ARB: &str = "0xbenarb0000000000000000000000000000000000";
const REFERENCE: &str = "intent-1";
const NOW: &str = "2026-10-28T20:00:00.000Z";

/// A host speaking as `me` on a clock every host of the test shares.
struct Host<'c> {
    me: &'static str,
    minute: &'c Cell<u32>,
    counter: Cell<u8>,
}

impl BillHost for Host<'_> {
    fn me(&self) -> &str {
        self.me
    }
    fn now(&self) -> String {
        self.minute.set(self.minute.get() + 1);
        let total = 19 * 60 + 30 + self.minute.get();
        format!("2026-10-28T{:02}:{:02}:00Z", total / 60, total % 60)
    }
    fn random_bytes(&self, byte_count: usize) -> Vec<u8> {
        self.counter.set(self.counter.get().wrapping_add(1));
        (0..byte_count)
            .map(|i| self.counter.get().wrapping_add(i as u8))
            .collect()
    }
    fn broadcast(&self, _: &str) -> Sent {
        unreachable!("nothing is sent")
    }
}

struct Built {
    folded: FoldedBill,
    obligation: Option<PayerObligation>,
    entry: Option<String>,
}

#[derive(Clone, Copy)]
struct Options {
    paid: bool,
    paid_by: &'static str,
    method: &'static str,
    confirmed: bool,
    priced: bool,
}

const PLAIN: Options = Options {
    paid: false,
    paid_by: "ana",
    method: "swap",
    confirmed: false,
    priced: true,
};

/// Ana owes Ben 40.00 EUR, and Ben is paid in USDC: on Base first, on
/// Arbitrum second. `paid` adds a swap record under `REFERENCE` by
/// `paid_by`, with `method`, and `confirmed` Ben's confirmation of it.
fn bill(o: Options, via: &BTreeMap<String, i64>) -> Built {
    let minute = Cell::new(0);
    let host = |me: &'static str| Host {
        me,
        minute: &minute,
        counter: Cell::new(me.as_bytes()[0]),
    };
    let (ana, ben) = (host("ana"), host("ben"));
    let key = base64url_no_pad(&(0..CREATOR_KEY_BYTES as u8).collect::<Vec<u8>>());
    let mut entries = vec![
        create_bill(&ana, "Trip", "EUR", "equal", &key, None).unwrap(),
        join_bill(&ana, Some("Ana"), Some("u1ana0000000000000"), None, None).unwrap(),
        join_bill(
            &ben,
            Some("Ben"),
            None,
            None,
            Some(vec![
                json!({"type": "swap", "address": BEN_BASE, "asset": "USDC", "chain": "base"}),
                json!({"type": "swap", "address": BEN_ARB, "asset": "USDC", "chain": "arb"}),
            ]),
        )
        .unwrap(),
        add_expense(
            &ben,
            "x1",
            "ben",
            8000,
            json!({"type": "equal", "among": ["ana", "ben"]}),
            None,
        )
        .unwrap(),
    ];
    if o.priced {
        entries.push(set_rate(&ana, "EUR", 51234, None).unwrap());
    }
    let mut entry = None;
    if o.paid {
        let (payer, to) = if o.paid_by == "ana" {
            (&ana, "ben")
        } else {
            (&ben, "ana")
        };
        let record = record_payment(
            payer,
            "p1",
            to,
            4000,
            o.method,
            Some(REFERENCE),
            None,
            None,
            None,
        )
        .unwrap();
        entry = Some(record["id"].as_str().unwrap().to_owned());
        entries.push(record);
    }
    let mut log = BillLog::new(&ana);
    assert!(log.add(entries).unwrap().is_empty());
    if o.confirmed {
        let payment_id = authored_id(o.paid_by, "p1");
        let digest = log.fold().unwrap().payment_digests[&payment_id].clone();
        let confirm =
            confirm_payment(&ben, &payment_id, "recipientConfirmed", None, &digest).unwrap();
        assert!(log.add(vec![confirm]).unwrap().is_empty());
    }
    let folded = log.fold().unwrap();
    assert!(folded.set_aside.is_empty(), "{:?}", folded.set_aside);
    let obligation = obligation_via(&ana, &folded, via).unwrap();
    Built {
        folded,
        obligation,
        entry,
    }
}

fn rate() -> ExchangeRate {
    ExchangeRate {
        currency: "EUR".to_owned(),
        minor_units_per_zec: 51234,
        at: "2026-10-28T19:40:00.000Z".to_owned(),
        source: None,
    }
}

fn zatoshi() -> i64 {
    fiat_to_zatoshi(4000, &rate(), Some("EUR"), RateRounding::Up).unwrap()
}

fn asset(symbol: &str, chain: &str) -> TradableAsset {
    TradableAsset {
        asset_id: format!("nep141:{chain}-{symbol}"),
        symbol: symbol.to_owned(),
        chain: chain.to_owned(),
        decimals: 6,
    }
}

fn quote() -> SwapQuote {
    SwapQuote {
        deposit_address: "t1deposit000000000000000000000000".to_owned(),
        recipient: Some(BEN_BASE.to_owned()),
        deposit_memo: None,
        amount_in_zatoshi: zatoshi(),
        amount_out: "39990000".to_owned(),
        min_amount_out: None,
        asset: asset("USDC", "base"),
        deadline: "2026-10-28T20:30:00.000Z".to_owned(),
        reference: Some(REFERENCE.to_owned()),
    }
}

fn swap_payout(address: &str, chain: &str) -> Payout {
    Payout {
        kind: "swap".to_owned(),
        address: Some(address.to_owned()),
        asset: Some("USDC".to_owned()),
        chain: Some(chain.to_owned()),
    }
}

struct Ask<'a> {
    at: &'a str,
    amount: i64,
    chosen: Option<Payout>,
    options: Options,
    with_obligation: bool,
}

const ASK: Ask<'static> = Ask {
    at: NOW,
    amount: 4000,
    chosen: None,
    options: PLAIN,
    with_obligation: true,
};

fn refused(q: &SwapQuote, ask: Ask<'_>) -> Option<SwapSendRefusal> {
    let first = bill(ask.options, &BTreeMap::new());
    let payouts = &first.folded.bill.participant("ben").unwrap().payouts;
    let index = ask
        .chosen
        .as_ref()
        .and_then(|c| declared_payout_index(payouts, c));
    let read = match index {
        None => first,
        Some(i) => bill(ask.options, &BTreeMap::from([("ben".to_owned(), i as i64)])),
    };
    let obligation = if ask.with_obligation {
        read.obligation.as_ref()
    } else {
        None
    };
    swap_send_refusal(
        q,
        ask.at,
        &read.folded.bill,
        obligation,
        "ben",
        ask.amount,
        ask.chosen.as_ref(),
    )
    .unwrap()
}

#[test]
fn the_figures_are_the_ones_this_bill_produces() {
    let b = bill(PLAIN, &BTreeMap::new());
    assert_eq!(zatoshi(), 7807316);
    let unpayable: Vec<_> = b
        .obligation
        .unwrap()
        .request
        .unpayable
        .iter()
        .map(|u| (u.id.clone(), u.reason, u.minor_units))
        .collect();
    assert_eq!(unpayable, [("ben".to_owned(), "payout_not_zec", 4000)]);
}

#[test]
fn a_quote_that_still_answers_the_bill_may_be_sent() {
    assert_eq!(refused(&quote(), ASK), None);
}

#[test]
fn an_expired_quote_is_refused_at_its_deadline_and_after() {
    let at = |at| refused(&quote(), Ask { at, ..ASK });
    assert_eq!(
        at("2026-10-28T20:30:00.000Z"),
        Some(SwapSendRefusal::Expired)
    );
    assert_eq!(
        at("2026-10-28T21:00:00.000Z"),
        Some(SwapSendRefusal::Expired)
    );
    assert_eq!(at("2026-10-28T20:29:59.999Z"), None);
}

#[test]
fn a_deposit_that_needs_a_memo_is_refused_and_an_empty_one_is_none() {
    let memo = |m: &str| {
        let mut q = quote();
        q.deposit_memo = Some(m.to_owned());
        refused(&q, ASK)
    };
    assert_eq!(memo("12345"), Some(SwapSendRefusal::NeedsMemo));
    assert_eq!(memo(""), None);
}

#[test]
fn expiry_is_reported_before_the_memo() {
    let mut q = quote();
    q.deposit_memo = Some("1".to_owned());
    assert_eq!(
        refused(
            &q,
            Ask {
                at: "2026-10-28T20:30:00.000Z",
                ..ASK
            }
        ),
        Some(SwapSendRefusal::Expired)
    );
}

#[test]
fn a_payout_the_payee_no_longer_declares_is_refused() {
    let gone = swap_payout("0xgone", "base");
    assert_eq!(
        refused(
            &quote(),
            Ask {
                chosen: Some(gone),
                ..ASK
            }
        ),
        Some(SwapSendRefusal::PayoutGone)
    );
    // Matched on all four fields: the declared address on another chain is
    // not declared.
    assert_eq!(
        refused(
            &quote(),
            Ask {
                chosen: Some(swap_payout(BEN_BASE, "arb")),
                ..ASK
            }
        ),
        Some(SwapSendRefusal::PayoutGone)
    );
}

#[test]
fn the_payout_chosen_is_the_one_the_quote_is_held_to() {
    let mut q = quote();
    q.recipient = Some(BEN_ARB.to_owned());
    q.asset = asset("USDC", "arb");
    let second = || Ask {
        chosen: Some(swap_payout(BEN_ARB, "arb")),
        ..ASK
    };
    assert_eq!(refused(&q, second()), None);
    // The first payout's quote no longer answers once the second is chosen.
    assert_eq!(
        refused(&quote(), second()),
        Some(SwapSendRefusal::RecipientChanged)
    );
}

#[test]
fn a_debt_already_paid_and_waiting_is_held_naming_who_confirms() {
    let b = bill(
        Options {
            paid: true,
            ..PLAIN
        },
        &BTreeMap::new(),
    );
    let r = swap_send_refusal(
        &quote(),
        NOW,
        &b.folded.bill,
        b.obligation.as_ref(),
        "ben",
        4000,
        None,
    )
    .unwrap();
    assert_eq!(
        r,
        Some(SwapSendRefusal::Held {
            paid_to: vec!["ben".to_owned()]
        })
    );
}

#[test]
fn a_debt_no_longer_owed_in_exactly_the_quoted_amount_is_refused() {
    let amount = |amount| refused(&quote(), Ask { amount, ..ASK });
    assert_eq!(amount(3999), Some(SwapSendRefusal::NotOwed));
    assert_eq!(amount(4001), Some(SwapSendRefusal::NotOwed));
    assert_eq!(
        refused(
            &quote(),
            Ask {
                with_obligation: false,
                ..ASK
            }
        ),
        Some(SwapSendRefusal::NotOwed)
    );
}

#[test]
fn an_unpriced_bill_owes_nothing_a_quote_can_pay() {
    assert_eq!(
        refused(
            &quote(),
            Ask {
                options: Options {
                    priced: false,
                    ..PLAIN
                },
                ..ASK
            }
        ),
        Some(SwapSendRefusal::NotOwed)
    );
}

#[test]
fn a_quote_for_another_address_is_refused() {
    let mut q = quote();
    q.recipient = Some("0xbenold".to_owned());
    assert_eq!(refused(&q, ASK), Some(SwapSendRefusal::RecipientChanged));
    q.recipient = None;
    assert_eq!(refused(&q, ASK), Some(SwapSendRefusal::RecipientChanged));
}

#[test]
fn a_quote_for_another_asset_or_chain_is_refused_and_case_is_not() {
    let with = |a: TradableAsset| {
        let mut q = quote();
        q.asset = a;
        refused(&q, ASK)
    };
    assert_eq!(
        with(asset("USDC", "eth")),
        Some(SwapSendRefusal::AssetChanged)
    );
    assert_eq!(
        with(asset("USDT", "base")),
        Some(SwapSendRefusal::AssetChanged)
    );
    assert_eq!(with(asset("usdc", "BASE")), None);
}

#[test]
fn a_quote_priced_at_another_rate_is_refused() {
    let priced = |zatoshi| {
        let mut q = quote();
        q.amount_in_zatoshi = zatoshi;
        refused(&q, ASK)
    };
    assert_eq!(priced(zatoshi() + 1), Some(SwapSendRefusal::RateChanged));
    assert_eq!(priced(zatoshi() - 1), Some(SwapSendRefusal::RateChanged));
}

#[test]
fn the_payers_unconfirmed_swap_record_is_withdrawn() {
    let b = bill(
        Options {
            paid: true,
            ..PLAIN
        },
        &BTreeMap::new(),
    );
    assert_eq!(
        failed_swap_withdrawals(&b.folded, "ana", REFERENCE),
        vec![b.entry.unwrap()]
    );
}

#[test]
fn a_record_under_another_reference_is_left() {
    let b = bill(
        Options {
            paid: true,
            ..PLAIN
        },
        &BTreeMap::new(),
    );
    assert!(failed_swap_withdrawals(&b.folded, "ana", "intent-2").is_empty());
}

#[test]
fn a_confirmed_record_is_left() {
    let b = bill(
        Options {
            paid: true,
            confirmed: true,
            ..PLAIN
        },
        &BTreeMap::new(),
    );
    assert!(!b.folded.bill.confirmed_payments.is_empty());
    assert!(failed_swap_withdrawals(&b.folded, "ana", REFERENCE).is_empty());
}

#[test]
fn a_record_somebody_else_wrote_is_theirs_to_withdraw() {
    let b = bill(
        Options {
            paid: true,
            paid_by: "ben",
            ..PLAIN
        },
        &BTreeMap::new(),
    );
    assert!(failed_swap_withdrawals(&b.folded, "ana", REFERENCE).is_empty());
    assert_eq!(
        failed_swap_withdrawals(&b.folded, "ben", REFERENCE),
        vec![b.entry.unwrap()]
    );
}

#[test]
fn a_record_that_is_not_a_swap_is_left() {
    let b = bill(
        Options {
            paid: true,
            method: "shieldedZec",
            ..PLAIN
        },
        &BTreeMap::new(),
    );
    assert!(failed_swap_withdrawals(&b.folded, "ana", REFERENCE).is_empty());
}

#[test]
fn a_payout_is_found_by_all_four_fields() {
    let a = Payout {
        kind: "zec".to_owned(),
        address: Some("u1a".to_owned()),
        asset: None,
        chain: None,
    };
    let b = swap_payout("0xb", "base");
    let list = [a.clone(), b.clone()];
    assert_eq!(declared_payout_index(&list, &b), Some(1));
    assert_eq!(declared_payout_index(&list, &a), Some(0));
    let no_chain = Payout {
        chain: None,
        ..b.clone()
    };
    assert_eq!(declared_payout_index(&list, &no_chain), None);
    assert_eq!(declared_payout_index(&[], &a), None);
}

#[test]
fn base_units_read_as_whole_tokens() {
    use splitz_host::format_base_units;
    assert_eq!(format_base_units("39990000", 6).as_deref(), Some("39.99"));
    assert_eq!(format_base_units("1000000", 6).as_deref(), Some("1"));
    assert_eq!(format_base_units("5", 6).as_deref(), Some("0.000005"));
    assert_eq!(format_base_units("0", 6).as_deref(), Some("0"));
    assert_eq!(format_base_units("007", 0).as_deref(), Some("7"));
    for bad in ["", "1.5", "-1", "1e6", " 1"] {
        assert_eq!(format_base_units(bad, 6), None, "{bad}");
    }
    assert_eq!(format_base_units("1", -1), None);
    // A provider's decimals are a uint8; past that the rendering would be
    // sized by whatever it answered.
    let tiny = format_base_units("1", splitz_host::MAX_TOKEN_DECIMALS).unwrap();
    assert_eq!(tiny.len(), 2 + 255);
    assert!(tiny.starts_with("0.000") && tiny.ends_with('1'));
    assert_eq!(format_base_units("1", 256), None);
    assert_eq!(format_base_units("1", i32::MAX), None);
}

#[test]
fn a_swap_record_names_its_asset_and_chain_and_the_floor_when_quoted() {
    use splitz_host::swap_record_note;
    assert_eq!(swap_record_note("USDC", "base", None), "USDC on base");
    assert_eq!(
        swap_record_note("USDC", "base", Some("39.5")),
        "at least 39.5 USDC on base"
    );
}

#[test]
fn a_deposit_is_one_request_and_a_note_carrying_the_swap() {
    let rate = splitz_core::ExchangeRate {
        currency: "EUR".to_owned(),
        minor_units_per_zec: 51_234,
        at: "2026-10-28T19:30:00.000Z".to_owned(),
        source: None,
    };
    let mut q = quote();
    q.amount_in_zatoshi = 7_807_316;
    let deposit =
        splitz_host::swap_deposit("bill-1", &q, "ben", 4000, &rate, "2026-10-28T19:31:00.000Z")
            .unwrap();
    assert_eq!(
        deposit.uri,
        "zcash:t1deposit000000000000000000000000?amount=0.07807316&label=swap%20to%20USDC"
    );
    let note = deposit.note;
    assert_eq!(note.uri, deposit.uri);
    assert_eq!(note.carried.get("ben"), Some(&4000));
    assert_eq!(note.zatoshi, Some(7_807_316));
    assert_eq!(note.rate, Some(rate.clone()));
    let watch = note.swap.expect("the note carries the swap");
    assert_eq!(watch.reference, REFERENCE);
    assert_eq!(
        (watch.asset_symbol.as_str(), watch.asset_chain.as_str()),
        ("USDC", "base")
    );

    // A deposit that needs a memo, and a debt of nothing, are refused.
    let mut memo = q.clone();
    memo.deposit_memo = Some("123".to_owned());
    assert!(splitz_host::swap_deposit("bill-1", &memo, "ben", 4000, &rate, "t").is_err());
    assert!(splitz_host::swap_deposit("bill-1", &q, "ben", 0, &rate, "t").is_err());
    // An empty memo is no memo.
    let mut empty = q.clone();
    empty.deposit_memo = Some(String::new());
    assert!(splitz_host::swap_deposit("bill-1", &empty, "ben", 4000, &rate, "t").is_ok());
}
