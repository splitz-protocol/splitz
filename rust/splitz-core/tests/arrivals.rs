//! Payments to this device matched against what its wallet received (§14.7).

use std::cell::Cell;

use serde_json::Value;
use splitz_core::compare_utf8;
use splitz_core::host::{
    arrivals_for, base64url_no_pad, confirm_payment, create_bill, join_bill, record_payment,
    Arrival, BillHost, BillLog, IncomingTransaction, Sent, SignEntry, VerifyEntry,
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
        let create = create_bill(&ana, name, "EUR", "equal", &fake_key("ana")).unwrap();
        ana.tick();
        let join_ana = join_bill(&ana, Some("Ana"), Some("u1ana"), None, None).unwrap();
        ben.tick();
        ben.tick();
        let join_ben = join_bill(&ben, Some("Ben"), Some("u1ben"), None, None).unwrap();
        ben.tick();
        ben.tick();
        Self {
            ana,
            ben,
            entries: vec![create, join_ana, join_ben],
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
    }
}

fn ids(arrivals: &[Arrival]) -> Vec<String> {
    arrivals
        .iter()
        .map(|a| format!("{}/{}", a.bill_id, a.payment.id))
        .collect()
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
    assert_eq!(payment_ids(&found.arrived), ["p1"]);
    assert_eq!(found.arrived[0].txid, T1);
    assert_eq!(found.arrived[0].record, folded.payment_digests["p1"]);
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
    let entry = confirm_payment(&b.ana, "p1", "walletReceived", Some(&a.txid), &a.record).unwrap();
    b.entries.push(entry);
    assert!(b.folded().bill.confirmed_payments.contains("p1"));
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
fn a_record_claiming_more_zec_than_arrived_is_short() {
    let mut b = Bill::new("Dinner");
    b.paid("p1", T1, Some(20_001), "shieldedZec");
    let found = arrivals_for(&[b.folded()], "ana", &[received(T1, 20_000)]);
    assert!(found.arrived.is_empty());
    assert_eq!(payment_ids(&found.short), ["p1"]);
}

#[test]
fn a_record_stating_no_zec_cannot_be_checked() {
    let mut b = Bill::new("Dinner");
    b.paid("p1", T1, None, "shieldedZec");
    let found = arrivals_for(&[b.folded()], "ana", &[received(T1, 20_000)]);
    assert!(found.arrived.is_empty());
    assert_eq!(payment_ids(&found.unstated), ["p1"]);
}

#[test]
fn one_transaction_is_evidence_once_across_bills() {
    let mut x = Bill::new("X");
    let mut y = Bill::new("Y");
    x.paid("p1", T1, Some(20_000), "shieldedZec");
    y.paid("p1", T1, Some(20_000), "shieldedZec");
    let (fx, fy) = (x.folded(), y.folded());
    assert_ne!(fx.bill.id, fy.bill.id);
    let first = if compare_utf8(&fx.bill.id, &fy.bill.id).is_lt() {
        fx.bill.id.clone()
    } else {
        fy.bill.id.clone()
    };
    let found = arrivals_for(&[fy, fx], "ana", &[received(T1, 20_000)]);
    assert_eq!(ids(&found.arrived), vec![format!("{first}/p1")]);
    assert_eq!(found.short.len(), 1);
}

#[test]
fn a_record_already_confirmed_uses_its_share_first() {
    let mut x = Bill::new("X");
    let mut y = Bill::new("Y");
    x.paid("p1", T1, Some(20_000), "shieldedZec");
    x.confirm("p1");
    y.paid("p2", T1, Some(20_000), "shieldedZec");
    let fy = y.folded();
    let found = arrivals_for(&[x.folded(), fy.clone()], "ana", &[received(T1, 30_000)]);
    assert!(found.arrived.is_empty(), "20000 of 30000 is already used");
    assert_eq!(ids(&found.short), vec![format!("{}/p2", fy.bill.id)]);
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
    assert_eq!(payment_ids(&found.short), ["p3"]);
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
    assert_eq!(payment_ids(&found.arrived), ["p1", "p2"]);
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
