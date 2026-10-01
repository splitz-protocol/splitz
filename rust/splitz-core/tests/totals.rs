//! Where this device stands with each person, across bills (host totals).

use std::cell::Cell;

use serde_json::{json, Value};
use splitz_core::host::{
    add_expense, base64url_no_pad, confirm_payment, create_bill, join_bill, record_payment,
    totals_across, BillHost, BillLog, FoldedBill, Sent, SignEntry, VerifyEntry,
};

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

/// Ana pays `amount` on a bill in `currency` named `name`, split evenly with
/// Ben, so Ben owes Ana half.
struct Bill {
    ana: FakeHost,
    ben: FakeHost,
    entries: Vec<Value>,
}

impl Bill {
    fn new(name: &str, currency: &str) -> Self {
        let ana = FakeHost::new("ana");
        let ben = FakeHost::new("ben");
        let create = create_bill(&ana, name, currency, "equal", &fake_key("ana"), None).unwrap();
        ana.tick();
        let join_ana = join_bill(&ana, Some("Ana"), Some("u1ana"), None, None).unwrap();
        ben.tick();
        ben.tick();
        let join_ben = join_bill(&ben, Some("Ben"), Some("u1ben"), None, None).unwrap();
        ana.tick();
        let expense = add_expense(
            &ana,
            "x1",
            "ana",
            9000,
            json!({"type": "equal", "among": ["ana", "ben"]}),
            None,
        )
        .unwrap();
        ben.tick();
        ben.tick();
        Self {
            ana,
            ben,
            entries: vec![create, join_ana, join_ben, expense],
        }
    }

    fn folded(&self) -> FoldedBill {
        let folded = BillLog::with_entries(&self.ana, self.entries.clone())
            .fold()
            .unwrap();
        assert!(folded.set_aside.is_empty(), "{:?}", folded.set_aside);
        folded
    }

    fn record(&mut self, id: &str, amount: i64) {
        let entry =
            record_payment(&self.ben, id, "ana", amount, "cash", None, None, None, None).unwrap();
        self.entries.push(entry);
    }
}

#[test]
fn one_person_across_two_bills_is_one_standing() {
    let bills = [
        Bill::new("X", "EUR").folded(),
        Bill::new("Y", "EUR").folded(),
    ];
    let for_ana = totals_across(&bills, "ana");
    assert!(for_ana.uncounted.is_empty());
    assert_eq!(for_ana.standings.len(), 1);
    let with_ben = &for_ana.standings[0];
    assert_eq!(
        (with_ben.with_id.as_str(), with_ben.currency.as_str()),
        ("ben", "EUR")
    );
    assert_eq!(
        (with_ben.owed_to_me, with_ben.owed_by_me, with_ben.net()),
        (9000, 0, 9000)
    );
    assert_eq!(with_ben.bill_ids.len(), 2);
    let with_ana = &totals_across(&bills, "ben").standings[0];
    assert_eq!((with_ana.owed_by_me, with_ana.net()), (9000, -9000));
}

#[test]
fn two_currencies_are_two_standings_never_one_sum() {
    let bills = [
        Bill::new("X", "EUR").folded(),
        Bill::new("Y", "USD").folded(),
    ];
    let got: Vec<String> = totals_across(&bills, "ana")
        .standings
        .iter()
        .map(|s| format!("{} {}", s.currency, s.owed_to_me))
        .collect();
    assert_eq!(got, ["EUR 4500", "USD 4500"]);
}

#[test]
fn a_payment_recorded_and_not_confirmed_is_on_its_way_still_owed() {
    let mut x = Bill::new("X", "EUR");
    x.record("p1", 4500);
    let bills = [x.folded()];
    let for_ben = &totals_across(&bills, "ben").standings[0];
    assert_eq!(for_ben.owed_by_me, 4500, "a record moves nothing (§10.5)");
    assert_eq!(for_ben.sent_awaiting, 4500);
    assert_eq!(
        totals_across(&bills, "ana").standings[0].received_awaiting,
        4500
    );
}

#[test]
fn a_confirmed_payment_is_neither_owed_nor_awaiting() {
    let mut x = Bill::new("X", "EUR");
    x.record("p1", 4500);
    x.ana.tick();
    let record = x.folded().payment_digests["ben:p1"].clone();
    let confirm = confirm_payment(&x.ana, "ben:p1", "recipientConfirmed", None, &record).unwrap();
    x.entries.push(confirm);
    assert!(totals_across(&[x.folded()], "ana").standings.is_empty());
}

#[test]
fn a_bill_that_would_carry_a_sum_past_64_bits_is_left_out_whole() {
    // Payments carry no cap (§2.2), so two unconfirmed records of 2^63 - 1 on
    // two bills cannot both be counted.
    let mut x = Bill::new("X", "EUR");
    let mut y = Bill::new("Y", "EUR");
    x.record("p1", i64::MAX);
    y.record("p1", i64::MAX);
    let totals = totals_across(&[x.folded(), y.folded()], "ana");
    assert_eq!(
        totals.uncounted.values().cloned().collect::<Vec<_>>(),
        ["amount_overflow"]
    );
    let kept = &totals.standings[0];
    assert_eq!(kept.received_awaiting, i64::MAX);
    assert_eq!(kept.owed_to_me, 4500, "the left-out bill adds nothing");
    assert_eq!(kept.bill_ids.len(), 1);
}

#[test]
fn somebody_on_no_bill_with_this_device_is_not_a_standing() {
    assert!(totals_across(&[Bill::new("X", "EUR").folded()], "cat")
        .standings
        .is_empty());
}

/// `f` with `bound` as the ids section 10.7 bound, as a verifying fold reports
/// it.
fn bound_to(mut f: FoldedBill, bound: &[&str]) -> FoldedBill {
    f.identities.bound = bound
        .iter()
        .map(|id| ((*id).to_owned(), format!("key-{id}")))
        .collect();
    f
}

#[test]
fn an_id_bound_on_one_bill_and_not_on_another_is_two_standings() {
    let x = bound_to(Bill::new("X", "EUR").folded(), &["ana", "ben"]);
    let y = Bill::new("Y", "EUR").folded();
    let apart = totals_across(&[x.clone(), y.clone()], "ana").standings;
    let rows: Vec<(&str, i64, usize)> = apart
        .iter()
        .map(|s| (s.with_id.as_str(), s.owed_to_me, s.bill_ids.len()))
        .collect();
    assert_eq!(rows, [("ben", 4500, 1), ("ben", 4500, 1)]);
    let one = totals_across(&[x, bound_to(y, &["ana", "ben"])], "ana").standings;
    assert_eq!(one.len(), 1);
    assert_eq!(one[0].owed_to_me, 9000);
}

#[test]
fn an_id_bound_to_two_keys_on_two_bills_is_two_standings() {
    // Y's creator binds `ben` to a key of their own: the same string, and
    // somebody else.
    let x = bound_to(Bill::new("X", "EUR").folded(), &["ana", "ben"]);
    let mut y = bound_to(Bill::new("Y", "EUR").folded(), &["ana", "ben"]);
    y.identities
        .bound
        .insert("ben".to_owned(), "key-mal".to_owned());
    let rows: Vec<(String, i64, usize)> = totals_across(&[x, y], "ana")
        .standings
        .iter()
        .map(|s| (s.with_id.clone(), s.owed_to_me, s.bill_ids.len()))
        .collect();
    assert_eq!(
        rows,
        [("ben".to_owned(), 4500, 1), ("ben".to_owned(), 4500, 1)]
    );
}

#[test]
fn a_record_the_payee_wrote_in_the_payers_name_is_not_on_its_way() {
    let mut x = Bill::new("X", "EUR");
    x.ana.tick();
    // Ana, the payee, records Ben paying her: section 10.4 lets either party
    // write a payment, but section 14.4 counts as sent only what its payer
    // recorded.
    let mut in_bens_name =
        record_payment(&x.ana, "p1", "ana", 4500, "cash", None, None, None, None).unwrap();
    in_bens_name["payment"]["from"] = json!("ben");
    in_bens_name["id"] = json!(splitz_core::derive_entry_id(&in_bens_name).unwrap());
    x.entries.push(in_bens_name);
    let for_ben = totals_across(&[x.folded()], "ben").standings;
    assert_eq!(for_ben[0].sent_awaiting, 0);
    assert_eq!(for_ben[0].owed_by_me, 4500);
    x.record("p2", 4500);
    assert_eq!(
        totals_across(&[x.folded()], "ben").standings[0].sent_awaiting,
        4500
    );
}
