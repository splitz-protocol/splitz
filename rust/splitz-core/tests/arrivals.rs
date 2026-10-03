//! Payments to this device matched against what its wallet received (§14.7).

use std::cell::Cell;

use serde_json::Value;
use splitz_core::compare_utf8;
use splitz_core::host::{
    arrivals_for, base64url_no_pad, confirm_payment, create_bill, join_bill, record_payment,
    set_rate, Arrival, Arrivals, BillHost, BillLog, FoldedBill, IncomingTransaction, Sent,
    SignEntry, VerifyEntry,
};

const T1: &str = "aa00000000000000000000000000000000000000000000000000000000000001";
const T2: &str = "bb00000000000000000000000000000000000000000000000000000000000002";

/// A clock held still until moved, and a counter for randomness.
struct FakeHost {
    me: String,
    minute: Cell<u32>,
    counter: Cell<u8>,
}

impl FakeHost {
    fn new(me: &str) -> Self {
        Self {
            me: me.to_owned(),
            minute: Cell::new(0),
            counter: Cell::new(0),
        }
    }

    fn tick(&self) {
        self.minute.set(self.minute.get() + 1);
    }
}

impl BillHost for FakeHost {
    fn me(&self) -> &str {
        &self.me
    }
    fn now(&self) -> String {
        format!("2026-10-28T19:{:02}:00.000Z", 30 + self.minute.get())
    }
    fn random_bytes(&self, byte_count: usize) -> Vec<u8> {
        self.counter.set(self.counter.get().wrapping_add(1));
        (0..byte_count)
            .map(|i| self.counter.get().wrapping_add(i as u8))
            .collect()
    }
    fn broadcast(&self, _uri: &str) -> Sent {
        Sent::sent("unused".to_owned())
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

/// Ana and Ben on one bill named `name`; Ben owes Ana.
struct Bill {
    ana: FakeHost,
    ben: FakeHost,
    entries: Vec<Value>,
}

impl Bill {
    fn new(name: &str) -> Self {
        let ana = FakeHost::new("ana");
        let ben = FakeHost::new("ben");
        let create = create_bill(&ana, name, "EUR", "equal", &fake_key("ana"), None).unwrap();
        ana.tick();
        let join_ana = join_bill(&ana, Some("Ana"), Some("u1ana"), None, None).unwrap();
        ana.tick();
        // 1,000,000.00 EUR a ZEC: 1000 zatoshi pays for the 10.00 EUR each
        // record settles.
        let rate = set_rate(&ana, "EUR", 100_000_000, None).unwrap();
        ben.tick();
        ben.tick();
        let join_ben = join_bill(&ben, Some("Ben"), Some("u1ben"), None, None).unwrap();
        ben.tick();
        ben.tick();
        Self {
            ana,
            ben,
            entries: vec![create, join_ana, rate, join_ben],
        }
    }

    fn folded(&self) -> splitz_core::host::FoldedBill {
        let folded = BillLog::with_entries(&self.ana, self.entries.clone())
            .fold()
            .unwrap();
        assert!(folded.set_aside.is_empty(), "{:?}", folded.set_aside);
        folded
    }

    fn paid(&mut self, id: &str, txid: &str, zatoshi: Option<i64>, method: &str) {
        let entry = record_payment(
            &self.ben,
            id,
            "ana",
            1000,
            method,
            Some(txid),
            zatoshi,
            None,
            None,
        )
        .unwrap();
        self.entries.push(entry);
    }

    fn confirm(&mut self, id: &str) {
        let id = &format!("ben:{id}");
        self.ana.tick();
        let record = self.folded().payment_digests[id].clone();
        let entry = confirm_payment(&self.ana, id, "recipientConfirmed", None, &record).unwrap();
        self.entries.push(entry);
    }
}

fn received(txid: &str, zatoshi: i64) -> IncomingTransaction {
    IncomingTransaction {
        txid: txid.to_owned(),
        zatoshi,
        memos: None,
    }
}

fn ids(arrivals: &[Arrival]) -> Vec<String> {
    arrivals
        .iter()
        .map(|a| format!("{}/{}", a.bill_id, a.payment.id))
        .collect()
}

/// `folded` with `ben` bound to `key` (None: unbound), for tests about who a
/// payer is rather than how a key comes to be bound.
fn bound(mut folded: FoldedBill, key: Option<&str>) -> FoldedBill {
    folded.identities.bound.clear();
    if let Some(key) = key {
        folded
            .identities
            .bound
            .insert("ben".to_owned(), key.to_owned());
    }
    folded
}

/// Ben pays T1 on bill X and on bill Y, each bound as `x` and `y`; what Ana's
/// wallet proposes for both bills.
fn two_bills(x: Option<&str>, y: Option<&str>) -> Arrivals {
    let mut bx = Bill::new("X");
    let mut by = Bill::new("Y");
    bx.paid("p1", T1, Some(10_000), "shieldedZec");
    by.paid("p1", T1, Some(10_000), "shieldedZec");
    arrivals_for(
        &[bound(bx.folded(), x), bound(by.folded(), y)],
        "ana",
        &[received(T1, 20_000)],
    )
}

#[test]
fn an_unbound_id_on_another_bill_is_not_the_bound_payer_it_names() {
    let found = two_bills(Some(&fake_key("ben")), None);
    assert!(found.arrived.is_empty());
    assert_eq!(found.disputed.len(), 2);
}

#[test]
fn one_id_bound_to_two_keys_on_two_bills_is_two_payers() {
    let found = two_bills(Some(&fake_key("ben")), Some(&fake_key("mal")));
    assert!(found.arrived.is_empty());
    assert_eq!(found.disputed.len(), 2);
}

#[test]
fn one_id_unbound_on_two_bills_is_two_payers() {
    let found = two_bills(None, None);
    assert!(found.arrived.is_empty());
    assert_eq!(found.disputed.len(), 2);
}

#[test]
fn one_key_on_two_bills_is_one_payer_and_both_arrive() {
    let found = two_bills(Some(&fake_key("ben")), Some(&fake_key("ben")));
    assert!(found.disputed.is_empty());
    assert_eq!(found.arrived.len(), 2);
}

fn payment_ids(arrivals: &[Arrival]) -> Vec<&str> {
    arrivals.iter().map(|a| a.payment.id.as_str()).collect()
}

#[test]
fn a_record_whose_transaction_arrived_with_its_zec_is_proposed() {
    let mut b = Bill::new("Dinner");
    b.paid("p1", T1, Some(20_000), "shieldedZec");
    let folded = b.folded();
    let found = arrivals_for(
        std::slice::from_ref(&folded),
        "ana",
        &[received(T1, 20_000)],
    );
    assert_eq!(payment_ids(&found.arrived), ["ben:p1"]);
    assert_eq!(found.arrived[0].txid, T1);
    assert_eq!(found.arrived[0].record, folded.payment_digests["ben:p1"]);
    assert!(found.short.is_empty() && found.unstated.is_empty());
}

#[test]
fn the_proposed_confirmation_is_one_the_fold_applies() {
    let mut b = Bill::new("Dinner");
    b.paid("p1", T1, Some(20_000), "shieldedZec");
    let a = arrivals_for(&[b.folded()], "ana", &[received(T1, 20_000)])
        .arrived
        .remove(0);
    b.ana.tick();
    let entry =
        confirm_payment(&b.ana, "ben:p1", "walletReceived", Some(&a.txid), &a.record).unwrap();
    b.entries.push(entry);
    assert!(b.folded().bill.confirmed_payments.contains("ben:p1"));
}

#[test]
fn a_transaction_id_is_matched_whatever_its_case_and_padding() {
    let mut b = Bill::new("Dinner");
    b.paid("p1", &T1.to_uppercase(), Some(20_000), "shieldedZec");
    let found = arrivals_for(
        &[b.folded()],
        "ana",
        &[received(&format!(" {T1} "), 20_000)],
    );
    assert_eq!(found.arrived.len(), 1);
}

#[test]
fn a_record_that_arrived_is_one_the_payee_may_not_withdraw() {
    let mut b = Bill::new("Dinner");
    b.paid("p1", T1, Some(20_000), "shieldedZec");
    b.paid("p2", T2, Some(20_001), "shieldedZec");
    let folded = b.folded();
    let bill_id = folded.bill.id.clone();
    let found = arrivals_for(
        std::slice::from_ref(&folded),
        "ana",
        &[received(T1, 20_000), received(T2, 20_000)],
    );
    assert_eq!(
        found.covering(&bill_id, "ben:p1").map(|a| a.txid.as_str()),
        Some(T1)
    );
    // Short of what it states, it is not evidence, and stays the payee's to
    // dispute; another id or another bill is not covered either.
    assert!(found.covering(&bill_id, "ben:p2").is_none());
    assert!(found.covering(&bill_id, "ben:p9").is_none());
    assert!(found.covering("another-bill", "ben:p1").is_none());
}

#[test]
fn a_record_claiming_more_zec_than_arrived_is_short() {
    let mut b = Bill::new("Dinner");
    b.paid("p1", T1, Some(20_001), "shieldedZec");
    let found = arrivals_for(&[b.folded()], "ana", &[received(T1, 20_000)]);
    assert!(found.arrived.is_empty());
    assert_eq!(payment_ids(&found.short), ["ben:p1"]);
}

#[test]
fn a_record_stating_no_zec_cannot_be_checked() {
    let mut b = Bill::new("Dinner");
    b.paid("p1", T1, None, "shieldedZec");
    let found = arrivals_for(&[b.folded()], "ana", &[received(T1, 20_000)]);
    assert!(found.arrived.is_empty());
    assert_eq!(payment_ids(&found.unstated), ["ben:p1"]);
}

#[test]
fn one_transaction_is_evidence_once_across_bills() {
    let mut x = Bill::new("X");
    let mut y = Bill::new("Y");
    x.paid("p1", T1, Some(20_000), "shieldedZec");
    y.paid("p1", T1, Some(20_000), "shieldedZec");
    let ben = fake_key("ben");
    let (fx, fy) = (bound(x.folded(), Some(&ben)), bound(y.folded(), Some(&ben)));
    assert_ne!(fx.bill.id, fy.bill.id);
    let first = if compare_utf8(&fx.bill.id, &fy.bill.id).is_lt() {
        fx.bill.id.clone()
    } else {
        fy.bill.id.clone()
    };
    let found = arrivals_for(&[fy, fx], "ana", &[received(T1, 20_000)]);
    assert_eq!(ids(&found.arrived), vec![format!("{first}/ben:p1")]);
    assert_eq!(found.short.len(), 1);
}

#[test]
fn a_record_already_confirmed_uses_its_share_first() {
    let mut x = Bill::new("X");
    let mut y = Bill::new("Y");
    x.paid("p1", T1, Some(20_000), "shieldedZec");
    x.confirm("p1");
    y.paid("p2", T1, Some(20_000), "shieldedZec");
    let ben = fake_key("ben");
    let fy = bound(y.folded(), Some(&ben));
    let found = arrivals_for(
        &[bound(x.folded(), Some(&ben)), fy.clone()],
        "ana",
        &[received(T1, 30_000)],
    );
    assert!(found.arrived.is_empty(), "20000 of 30000 is already used");
    assert_eq!(ids(&found.short), vec![format!("{}/ben:p2", fy.bill.id)]);
}

#[test]
fn records_stating_the_most_zec_an_integer_holds_cannot_wrap_the_count() {
    // Two confirmed records naming one transaction, each stating 2^63 - 1
    // zatoshi: a subtraction that wrapped would leave the transaction with
    // ZEC to spare, and the next record would arrive.
    let mut b = Bill::new("Dinner");
    b.paid("p1", T1, Some(i64::MAX), "shieldedZec");
    b.confirm("p1");
    b.paid("p2", T1, Some(i64::MAX), "shieldedZec");
    b.confirm("p2");
    b.paid("p3", T1, Some(1000), "shieldedZec");
    let found = arrivals_for(&[b.folded()], "ana", &[received(T1, 20_000)]);
    assert!(found.arrived.is_empty());
    assert_eq!(payment_ids(&found.short), ["ben:p3"]);
}

#[test]
fn two_transactions_each_pay_for_their_own_record() {
    let mut b = Bill::new("Dinner");
    b.paid("p1", T1, Some(20_000), "shieldedZec");
    b.paid("p2", T2, Some(20_000), "shieldedZec");
    let found = arrivals_for(
        &[b.folded()],
        "ana",
        &[received(T1, 20_000), received(T2, 20_000)],
    );
    assert_eq!(payment_ids(&found.arrived), ["ben:p1", "ben:p2"]);
}

#[test]
fn records_to_somebody_else_and_other_methods_are_not_matched() {
    let mut b = Bill::new("Dinner");
    b.paid("p1", T1, Some(20_000), "cash");
    b.paid("p2", T1, Some(20_000), "swap");
    let folded = b.folded();
    assert!(arrivals_for(
        std::slice::from_ref(&folded),
        "ben",
        &[received(T1, 20_000)]
    )
    .arrived
    .is_empty());
    let for_ana = arrivals_for(&[folded], "ana", &[received(T1, 20_000)]);
    assert!(for_ana.arrived.is_empty());
    assert!(for_ana.short.is_empty());
    assert!(for_ana.unstated.is_empty());
}

#[test]
fn a_transaction_nobody_named_proposes_nothing() {
    let mut b = Bill::new("Dinner");
    b.paid("p1", T1, Some(20_000), "shieldedZec");
    let found = arrivals_for(&[b.folded()], "ana", &[received(T2, 20_000)]);
    assert!(found.arrived.is_empty());
    assert!(found.short.is_empty());
    assert!(found.unstated.is_empty());
}

#[test]
fn a_txid_is_compared_with_ascii_space_trimmed_and_ascii_lower_cased_nothing_wider() {
    use splitz_core::host::txid_key;
    assert_eq!(txid_key(" \tABCD\r\n"), "abcd");
    assert_eq!(txid_key("abcd\u{FEFF}"), "abcd\u{FEFF}");
    assert_eq!(txid_key("\u{00A0}abcd"), "\u{00A0}abcd");
    assert_eq!(txid_key("\u{0130}BC"), "\u{0130}bc");
}

#[test]
fn a_txid_in_digest_order_is_reversed_into_the_order_a_send_reports() {
    use splitz_core::host::txid_in_send_order;
    // Reversed outside this crate, in Python, byte pair by byte pair.
    let digest = "00112233445566778899aabbccddeeff0123456789abcdef0f1e2d3c4b5a6978";
    let sent = "78695a4b3c2d1e0fefcdab8967452301ffeeddccbbaa99887766554433221100";
    assert_eq!(txid_in_send_order(digest).as_deref(), Some(sent));
    assert_eq!(txid_in_send_order(sent).as_deref(), Some(digest));
    assert_eq!(
        txid_in_send_order(&format!(" {}\n", digest.to_uppercase())).as_deref(),
        Some(sent)
    );
    assert_eq!(txid_in_send_order(&digest[2..]), None);
    assert_eq!(txid_in_send_order(&format!("{digest}00")), None);
    assert_eq!(txid_in_send_order(&digest.replace('0', "g")), None);
    assert_eq!(txid_in_send_order(""), None);
}

#[test]
fn a_transaction_two_payers_name_is_evidence_for_neither() {
    let mut b = Bill::new("Dinner");
    b.paid("p1", T1, Some(20_000), "shieldedZec");
    let mal = FakeHost::new("mal");
    mal.tick();
    b.entries
        .push(join_bill(&mal, Some("Mal"), Some("u1mal"), None, None).unwrap());
    mal.tick();
    b.entries.push(
        record_payment(
            &mal,
            "0",
            "ana",
            1000,
            "shieldedZec",
            Some(T1),
            Some(20_000),
            None,
            None,
        )
        .unwrap(),
    );
    let found = arrivals_for(&[b.folded()], "ana", &[received(T1, 20_000)]);
    assert!(found.arrived.is_empty());
    let mut disputed = payment_ids(&found.disputed);
    disputed.sort();
    assert_eq!(disputed, ["ben:p1", "mal:0"]);
}

#[test]
fn one_payer_naming_a_transaction_twice_is_still_counted_once() {
    let mut b = Bill::new("Dinner");
    b.paid("p1", T1, Some(20_000), "shieldedZec");
    b.paid("p2", T1, Some(20_000), "shieldedZec");
    let found = arrivals_for(&[b.folded()], "ana", &[received(T1, 20_000)]);
    assert!(found.disputed.is_empty());
    assert_eq!(payment_ids(&found.arrived), ["ben:p1"]);
    assert_eq!(payment_ids(&found.short), ["ben:p2"]);
}

/// One record of 10.00 EUR on a bill priced at `rate`, paid with `zatoshi`.
fn priced(rate: Option<i64>, zatoshi: i64) -> Arrivals {
    let mut b = Bill::new("priced");
    if let Some(rate) = rate {
        b.ana.tick();
        b.entries.push(set_rate(&b.ana, "EUR", rate, None).unwrap());
    } else {
        b.entries.retain(|e| e["kind"] != "setRate");
    }
    b.paid("p1", T1, Some(zatoshi), "shieldedZec");
    arrivals_for(&[b.folded()], "ana", &[received(T1, zatoshi)])
}

#[test]
fn a_record_whose_zec_pays_for_a_fraction_of_its_amount_is_not_proposed() {
    // 1 zatoshi at 1,000,000.00 EUR a ZEC is worth 1 cent, not 10.00 EUR.
    let found = priced(Some(100_000_000), 1);
    assert_eq!(payment_ids(&found.underpriced), vec!["ben:p1"]);
    assert!(found.arrived.is_empty());
}

#[test]
fn ninety_five_percent_of_the_amount_pays_for_it_and_one_zatoshi_less_does_not() {
    // 950 zatoshi x 100,000,000 x 100 = 9.5e12 = 1000 x 95 x 10^8.
    assert_eq!(
        payment_ids(&priced(Some(100_000_000), 950).arrived),
        vec!["ben:p1"]
    );
    assert_eq!(
        payment_ids(&priced(Some(100_000_000), 949).underpriced),
        vec!["ben:p1"]
    );
}

#[test]
fn a_bill_with_no_rate_vouches_for_no_record() {
    let found = priced(None, 20_000);
    assert_eq!(payment_ids(&found.underpriced), vec!["ben:p1"]);
    assert!(found.arrived.is_empty());
}

#[test]
fn a_worth_past_every_integer_still_pays() {
    // i64::MAX zatoshi at i64::MAX a ZEC: the product passes i128 x 100.
    let found = priced(Some(i64::MAX), i64::MAX);
    assert!(
        found.underpriced.is_empty(),
        "{:?}",
        payment_ids(&found.underpriced)
    );
}

/// One record on a bill, paid in a transaction whose memos are `memos`,
/// `<bill>` standing for the bill's id.
fn with_memos(memos: Option<&[&str]>) -> Arrivals {
    let mut b = Bill::new("memo");
    b.paid("p1", T1, Some(20_000), "shieldedZec");
    let folded = b.folded();
    let id = folded.bill.id.clone();
    let tx = IncomingTransaction {
        txid: T1.to_owned(),
        zatoshi: 20_000,
        memos: memos.map(|m| m.iter().map(|s| s.replace("<bill>", &id)).collect()),
    };
    arrivals_for(&[folded], "ana", &[tx])
}

#[test]
fn a_memo_naming_the_bill_is_proposed() {
    assert_eq!(
        payment_ids(&with_memos(Some(&["splitz:<bill>"])).arrived),
        ["ben:p1"]
    );
}

#[test]
fn another_bills_memo_is_not() {
    let found = with_memos(Some(&["splitz:SomeOtherBill00000000"]));
    assert_eq!(payment_ids(&found.unbound), ["ben:p1"]);
    assert!(found.arrived.is_empty());
}

#[test]
fn no_memo_at_all_is_not() {
    assert_eq!(payment_ids(&with_memos(Some(&[])).unbound), ["ben:p1"]);
}

#[test]
fn memos_the_wallet_could_not_read_decide_nothing() {
    assert_eq!(payment_ids(&with_memos(None).arrived), ["ben:p1"]);
}

#[test]
fn the_ninety_five_percent_line_is_exact_at_the_boundary() {
    use splitz_core::host::zatoshi_covers_payment;
    use splitz_core::{ExchangeRate, PaymentRecord};
    let payment = PaymentRecord {
        id: "p".into(),
        from: "ben".into(),
        to: "ana".into(),
        amount: 1000,
        currency: "EUR".into(),
        method: "shieldedZec".into(),
        at: "2026-10-28T19:30:00.000Z".into(),
        zatoshi: None,
        paid_at_rate: None,
        reference: None,
        note: None,
    };
    let rate = |currency: &str| ExchangeRate {
        currency: currency.into(),
        minor_units_per_zec: 100_000,
        at: "2026-10-28T19:30:00.000Z".into(),
        source: None,
    };
    // 10.00 EUR at 1000.00 a ZEC: 95% is 950000 zatoshi exactly.
    assert!(zatoshi_covers_payment(
        950_000,
        &payment,
        Some(&rate("EUR"))
    ));
    assert!(!zatoshi_covers_payment(
        949_999,
        &payment,
        Some(&rate("EUR"))
    ));
    assert!(!zatoshi_covers_payment(
        950_000,
        &payment,
        Some(&rate("USD"))
    ));
    assert!(!zatoshi_covers_payment(950_000, &payment, None));
    // A product past i128 is worth more than any amount.
    let huge = ExchangeRate {
        minor_units_per_zec: i64::MAX,
        ..rate("EUR")
    };
    assert!(zatoshi_covers_payment(i64::MAX, &payment, Some(&huge)));
}

#[test]
fn memos_are_read_for_the_transactions_candidates_name_and_no_others() {
    use splitz_core::host::memo_txids;
    let mut b = Bill::new("Dinner");
    b.paid(
        "p1",
        &format!(" {} ", T1.to_uppercase()),
        Some(20_000),
        "shieldedZec",
    );
    b.paid("p2", T2, Some(20_000), "cash");
    let folded = b.folded();
    assert_eq!(
        memo_txids(std::slice::from_ref(&folded), "ana")
            .into_iter()
            .collect::<Vec<_>>(),
        vec![T1.to_owned()]
    );
    assert!(memo_txids(std::slice::from_ref(&folded), "ben").is_empty());
}
