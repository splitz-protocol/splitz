//! §14.10: one transaction pays a request's ZEC payees and one swap deposit,
//! and its note records each half after a restart. The same cases as
//! `splitz_host/test/combined_send_test.dart`.

use std::cell::Cell;

use serde_json::json;
use splitz_core::host::{
    add_expense, base64url_no_pad, close_for, create_bill, join_bill, obligation_for, set_rate,
    BillHost, BillLog, PayerObligation, Sent, CREATOR_KEY_BYTES,
};
use splitz_core::{fiat_to_zatoshi, Bill, RateRounding};
use splitz_host::{
    combined_send, swap_deposit, unsent_claim_refusal, HostError, InMemoryBillStorage,
    OwnTransaction, PendingSends, SwapQuote, SwapSendRefusal, TradableAsset, Unrecordable,
    UnsentClaimRefusal,
};

/// `tools/corpus/_spec.py` ADDRESSES[1], so the request renders (§8.6).
const CAI_ZEC: &str = "u1nztelxna9h7w0vtpd2xjhxt4lpu8s9cmdl8n8vcr7actf2ny45nd07cy8cyuhuvw3axcp545y0ktq9cezuzx84jyhex8dk4tdvwhu4dl";
const BEN_BASE: &str = "0xben000000000000000000000000000000000000";
const AT: &str = "2026-10-28T20:00:00.000Z";

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

/// Ana owes Ben 40.00 EUR, paid in USDC on Base, and Cai 20.00 EUR, paid in
/// ZEC. Closed for settling.
fn with_bill<R>(
    run: impl FnOnce(&Host<'_>, &mut BillLog<'_>, &PayerObligation, &str, &Bill) -> R,
) -> R {
    let minute = Cell::new(0);
    let host = |me: &'static str| Host {
        me,
        minute: &minute,
        counter: Cell::new(me.as_bytes()[0]),
    };
    let (ana, ben, cai) = (host("ana"), host("ben"), host("cai"));
    let key = base64url_no_pad(&(0..CREATOR_KEY_BYTES as u8).collect::<Vec<u8>>());
    let entries = vec![
        create_bill(&ana, "Trip", "EUR", "equal", &key, None).unwrap(),
        join_bill(&ana, Some("Ana"), Some(CAI_ZEC), None, None).unwrap(),
        join_bill(
            &ben,
            Some("Ben"),
            None,
            None,
            Some(vec![
                json!({"type": "swap", "address": BEN_BASE, "asset": "USDC", "chain": "base"}),
            ]),
        )
        .unwrap(),
        join_bill(&cai, Some("Cai"), Some(CAI_ZEC), None, None).unwrap(),
        add_expense(
            &ben,
            "x1",
            "ben",
            8000,
            json!({"type": "equal", "among": ["ana", "ben"]}),
            None,
        )
        .unwrap(),
        add_expense(
            &cai,
            "x2",
            "cai",
            4000,
            json!({"type": "equal", "among": ["ana", "cai"]}),
            None,
        )
        .unwrap(),
        set_rate(&ana, "EUR", 51234, None).unwrap(),
    ];
    let mut log = BillLog::new(&ana);
    assert!(log.add(entries).unwrap().is_empty());
    let close = close_for(&ana, &log.fold().unwrap()).unwrap();
    assert!(log.add(vec![close]).unwrap().is_empty());
    let folded = log.fold().unwrap();
    let owed = obligation_for(&ana, &folded).unwrap().unwrap();
    let bill_id = folded.bill.id.clone();
    run(&ana, &mut log, &owed, &bill_id, &folded.bill)
}

/// Ben's 40.00 EUR at the bill's rate, rounded up as a deposit is sized.
fn deposit(bill: &Bill) -> i64 {
    fiat_to_zatoshi(
        4000,
        bill.rate.as_ref().unwrap(),
        Some("EUR"),
        RateRounding::Up,
    )
    .unwrap()
}

fn quote(memo: Option<&str>, bill: &Bill) -> SwapQuote {
    SwapQuote {
        deposit_address: "t1deposit000000000000000000000000".to_owned(),
        recipient: Some(BEN_BASE.to_owned()),
        deposit_memo: memo.map(str::to_owned),
        amount_in_zatoshi: deposit(bill),
        amount_out: "40000000".to_owned(),
        min_amount_out: None,
        asset: TradableAsset {
            asset_id: "nep141:base-usdc".to_owned(),
            symbol: "USDC".to_owned(),
            chain: "base".to_owned(),
            decimals: 6,
        },
        deadline: "2099-01-01T00:00:00.000Z".to_owned(),
        reference: Some("intent-1".to_owned()),
    }
}

#[test]
fn one_request_carries_every_zec_payee_and_the_deposit() {
    with_bill(|_, _, owed, bill_id, bill| {
        assert_eq!(
            owed.carried_to().into_iter().collect::<Vec<_>>(),
            vec![("cai".to_owned(), 2000)]
        );
        let sent = combined_send(bill_id, bill, owed, &quote(None, bill), "ben", 4000, AT).unwrap();
        assert_eq!(sent.uri.matches("address").count(), 2, "{}", sent.uri);
        assert!(sent.uri.contains("t1deposit") && sent.uri.contains(CAI_ZEC));
        assert_eq!(sent.note.carried.get("ben"), Some(&4000));
        assert_eq!(sent.note.carried.get("cai"), Some(&2000));
        assert_eq!(sent.note.sent, owed.carried_zatoshi());
        assert_eq!(sent.note.zatoshi, Some(deposit(bill)));
        assert_eq!(sent.note.swap.as_ref().unwrap().to, "ben");
    });
}

#[test]
fn a_memo_deposit_and_a_payee_the_request_already_pays_are_refused() {
    with_bill(|_, _, owed, bill_id, bill| {
        assert!(combined_send(
            bill_id,
            bill,
            owed,
            &quote(Some("needed"), bill),
            "ben",
            4000,
            AT
        )
        .is_err());
        assert!(matches!(
            combined_send(bill_id, bill, owed, &quote(None, bill), "cai", 2000, AT),
            Err(HostError::Malformed(_))
        ));
    });
}

#[test]
fn a_request_paying_nobody_in_zec_has_nothing_for_a_swap_to_join() {
    with_bill(|_, _, owed, bill_id, bill| {
        let mut nobody = owed.clone();
        nobody.request.payments.clear();
        match combined_send(bill_id, bill, &nobody, &quote(None, bill), "ben", 4000, AT) {
            Err(HostError::Protocol(e)) => {
                assert_eq!(e.code, splitz_core::code::ZIP321_NO_PAYMENTS)
            }
            other => panic!("{other:?}"),
        }
    });
}

#[test]
fn a_swap_leg_a_deposit_alone_would_be_refused_is_refused_here_too() {
    with_bill(|_, _, owed, bill_id, bill| {
        let refused = |q: SwapQuote| match combined_send(bill_id, bill, owed, &q, "ben", 4000, AT) {
            Err(HostError::SwapRefused(why)) => why,
            other => panic!("not refused: {other:?}"),
        };
        let mut expired = quote(None, bill);
        expired.deadline = "2020-01-01T00:00:00.000Z".to_owned();
        assert_eq!(refused(expired), SwapSendRefusal::Expired);
        let mut short = quote(None, bill);
        short.amount_in_zatoshi -= 1;
        assert_eq!(refused(short), SwapSendRefusal::RateChanged);
        let mut elsewhere = quote(None, bill);
        elsewhere.recipient = Some("0xsomebodyelse".to_owned());
        assert_eq!(refused(elsewhere), SwapSendRefusal::RecipientChanged);
        // The honest quote, beside them, goes through.
        assert!(combined_send(bill_id, bill, owed, &quote(None, bill), "ben", 4000, AT).is_ok());
    });
}

#[test]
fn a_restart_records_the_request_half_and_leaves_the_swap_to_its_own_record() {
    with_bill(|ana, log, owed, bill_id, bill| {
        let sent = combined_send(bill_id, bill, owed, &quote(None, bill), "ben", 4000, AT).unwrap();
        let storage = InMemoryBillStorage::default();
        let sends = PendingSends::new(&storage);
        let records = sends
            .records_for(ana, log, &sent.note, &"a".repeat(64))
            .unwrap();
        let to: Vec<&str> = records
            .iter()
            .map(|r| r["payment"]["to"].as_str().unwrap())
            .collect();
        assert_eq!(to, vec!["cai"]);
    });
}

#[test]
fn a_deposit_sent_alone_is_still_recorded_by_its_reference() {
    with_bill(|ana, log, owed, bill_id, bill| {
        let alone = swap_deposit(bill_id, &quote(None, bill), "ben", 4000, &owed.rate, AT).unwrap();
        let storage = InMemoryBillStorage::default();
        let sends = PendingSends::new(&storage);
        assert_eq!(
            sends.records_for(ana, log, &alone.note, &"a".repeat(64)),
            Err(Unrecordable::IsASwap)
        );
    });
}

#[test]
fn the_word_that_nothing_left_is_held_by_a_transaction_sending_both_halves() {
    with_bill(|_, _, owed, bill_id, bill| {
        let sent = combined_send(bill_id, bill, owed, &quote(None, bill), "ben", 4000, AT).unwrap();
        let both: i64 = sent.note.sent.values().sum::<i64>() + sent.note.zatoshi.unwrap();
        let tx = |id: &str, amount: i64| OwnTransaction {
            txid: id.repeat(64),
            created: "2026-10-28T20:00:05.000Z".to_owned(),
            sent: Some(amount),
        };
        assert!(matches!(
            unsent_claim_refusal(&sent.note, false, &[tx("b", both)]),
            Some(UnsentClaimRefusal::BuiltSince { .. })
        ));
        assert_eq!(
            unsent_claim_refusal(&sent.note, false, &[tx("c", both + 1)]),
            None
        );
    });
}
