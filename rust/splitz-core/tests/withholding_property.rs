//! §14.4, as properties over generated bills.
//!
//! Paying some of one's debts exactly never holds back the ones not yet paid;
//! and whatever expenses arrive while a payment is unconfirmed, what a request
//! carries plus what the payer has pending never exceeds what they owe. The
//! first is the rule's failure mode, an honest payer refused; the second is
//! what the rule exists for, a debt asked for twice.

use serde_json::{json, Value};
use std::cell::Cell;
use std::collections::BTreeSet;

use splitz_core::host::{
    add_expense, confirm_payment, create_bill, join_bill, obligation_for, record_payment, set_rate,
    BillHost, BillLog, PayerObligation, Sent,
};
use splitz_core::net_balances;

/// A clock that moves one second per entry and randomness that is a counter,
/// so a seed always builds the same bill.
struct Host {
    me: String,
    second: Cell<u32>,
    counter: Cell<u8>,
}

impl Host {
    fn new(me: &str) -> Self {
        Self {
            me: me.to_owned(),
            second: Cell::new(0),
            counter: Cell::new(0),
        }
    }
}

impl BillHost for Host {
    fn me(&self) -> &str {
        &self.me
    }

    fn now(&self) -> String {
        let s = self.second.get();
        self.second.set(s + 1);
        format!("2026-10-28T19:{:02}:{:02}.000Z", s / 60, s % 60)
    }

    fn random_bytes(&self, byte_count: usize) -> Vec<u8> {
        self.counter.set(self.counter.get().wrapping_add(1));
        let base = self.counter.get().wrapping_add(self.me.as_bytes()[0]);
        (0..byte_count)
            .map(|i| base.wrapping_add(i as u8))
            .collect()
    }

    fn broadcast(&self, _uri: &str) -> Sent {
        panic!("this test sends nothing")
    }
}

/// xorshift64: deterministic and dependency-free.
struct Rng(u64);

impl Rng {
    fn below(&mut self, n: u64) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0 % n
    }

    fn coin(&mut self) -> bool {
        self.below(2) == 1
    }
}

const NAMES: [&str; 6] = ["ana", "ben", "cai", "dee", "eve", "fay"];

/// A priced bill among three to six people.
struct Trip {
    rng: Rng,
    people: Vec<&'static str>,
    hosts: Vec<Host>,
    entries: Vec<Value>,
    expenses: usize,
}

impl Trip {
    fn new(seed: u64) -> Self {
        let mut rng = Rng(seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1);
        let people: Vec<_> = NAMES[..3 + rng.below(4) as usize].to_vec();
        let hosts: Vec<_> = people.iter().map(|p| Host::new(p)).collect();
        let mut entries =
            vec![create_bill(&hosts[0], "Trip", "USD", "equal", &"A".repeat(43), None).unwrap()];
        for (p, h) in people.iter().zip(&hosts) {
            entries.push(join_bill(h, Some(p), Some(&format!("u1{p}")), None, None).unwrap());
        }
        entries.push(set_rate(&hosts[0], "USD", 100_000, None).unwrap());
        Self {
            rng,
            people,
            hosts,
            entries,
            expenses: 0,
        }
    }

    fn host(&self, who: &str) -> &Host {
        &self.hosts[self.people.iter().position(|p| *p == who).unwrap()]
    }

    fn expense(&mut self) {
        let payer = self.people[self.rng.below(self.people.len() as u64) as usize];
        let mut among = Vec::new();
        for p in self.people.clone() {
            if p == payer || self.rng.coin() {
                among.push(p);
            }
        }
        let amount = 100 + self.rng.below(5000) as i64;
        let id = format!("x{}", self.expenses);
        self.expenses += 1;
        let e = add_expense(
            self.host(payer),
            &id,
            payer,
            amount,
            json!({ "type": "equal", "among": among }),
            None,
        )
        .unwrap();
        self.entries.push(e);
    }

    fn obligation(&self, who: &str) -> Option<PayerObligation> {
        let h = self.host(who);
        let mut log = BillLog::new(h);
        log.add(self.entries.clone()).unwrap();
        obligation_for(h, &log.fold().unwrap()).unwrap()
    }

    fn pay(&mut self, who: &str, id: &str, to: &str, amount: i64) {
        let e = record_payment(
            self.host(who),
            id,
            to,
            amount,
            "shieldedZec",
            None,
            None,
            None,
            None,
        )
        .unwrap();
        self.entries.push(e);
    }
}

#[test]
fn paying_some_debts_exactly_leaves_every_other_one_payable() {
    let (mut payers, mut checked) = (0, 0);
    for seed in 1..=500u64 {
        let mut trip = Trip::new(seed);
        for _ in 0..2 + trip.rng.below(5) {
            trip.expense();
        }
        for debtor in trip.people.clone() {
            let owed = match trip.obligation(debtor) {
                Some(o) if o.settlements.len() >= 2 => o.settlements,
                _ => continue,
            };
            payers += 1;
            // A random nonempty proper subset, each paid exactly what it asks.
            let mut paid = BTreeSet::new();
            while paid.is_empty() || paid.len() == owed.len() {
                paid.clear();
                for s in &owed {
                    if trip.rng.coin() {
                        paid.insert(s.to.clone());
                    }
                }
            }
            let before = trip.entries.clone();
            for s in owed.iter().filter(|s| paid.contains(&s.to)) {
                trip.pay(debtor, &format!("p-{}", s.to), &s.to, s.amount);
            }
            let after = trip.obligation(debtor).unwrap();
            let asked: BTreeSet<_> = after.settlements.iter().map(|s| &s.to).collect();
            let waiting: BTreeSet<_> = after.awaiting.iter().map(|a| &a.to).collect();
            for s in &owed {
                checked += 1;
                if paid.contains(&s.to) {
                    assert!(
                        waiting.contains(&s.to),
                        "seed {seed}: {debtor} paid {}",
                        s.to
                    );
                } else {
                    assert!(
                        asked.contains(&s.to),
                        "seed {seed}: {debtor} paid {paid:?} exactly; {} must still be payable",
                        s.to
                    );
                }
            }
            trip.entries = before;
        }
    }
    // A property that never reached a bill proves nothing.
    assert!(payers > 200, "{payers} payers");
    println!("{payers} payers on generated bills, {checked} settlements checked");
}

#[test]
fn a_request_plus_what_is_pending_never_exceeds_what_is_owed() {
    let (mut payers, mut held) = (0, 0);
    for seed in 1..=1500u64 {
        let mut trip = Trip::new(seed);
        for _ in 0..2 + trip.rng.below(4) {
            trip.expense();
        }
        let debtor = trip.people[trip.rng.below(trip.people.len() as u64) as usize];
        let first = trip.obligation(debtor).unwrap();
        if first.settlements.is_empty() {
            continue;
        }
        payers += 1;
        // Some of what is asked, each paid exactly, and left unconfirmed.
        for (i, s) in first.settlements.iter().enumerate() {
            if i == 0 || trip.rng.coin() {
                trip.pay(debtor, &format!("p-{}", s.to), &s.to, s.amount);
            }
        }
        // Expenses arrive while those payments wait; netting may move the
        // debt they paid onto somebody else.
        for _ in 0..1 + trip.rng.below(3) {
            trip.expense();
        }

        let h = trip.host(debtor);
        let mut log = BillLog::new(h);
        log.add(trip.entries.clone()).unwrap();
        let bill = log.fold().unwrap().bill;
        let pending: i64 = bill
            .payments
            .iter()
            .filter(|p| p.from == debtor && !bill.confirmed_payments.contains(&p.id))
            .map(|p| p.amount)
            .sum();
        let balance = net_balances(&bill).unwrap()[debtor];
        let owes = (-balance).max(0);
        let asked = trip.obligation(debtor).unwrap();
        let carried: i64 = asked.settlements.iter().map(|s| s.amount).sum();
        if !asked.awaiting.is_empty() {
            held += 1;
        }
        // Later expenses can leave what is pending above what is owed; the
        // request then adds nothing to it.
        let room = (owes - pending).max(0);
        assert!(
            carried <= room,
            "seed {seed}: {debtor} owes {owes}, has {pending} pending, and is asked for {carried} more"
        );

        // Paying every request and confirming everything leaves nobody
        // overpaid by a request.
        for s in &asked.settlements {
            trip.pay(debtor, &format!("q-{}", s.to), &s.to, s.amount);
        }
        let mut log = BillLog::new(trip.host(debtor));
        log.add(trip.entries.clone()).unwrap();
        let folded = log.fold().unwrap();
        for p in &folded.bill.payments {
            let e = confirm_payment(
                trip.host(&p.to),
                &p.id,
                "recipientConfirmed",
                None,
                &folded.payment_digests[&p.id],
            )
            .unwrap();
            trip.entries.push(e);
        }
        let mut log = BillLog::new(trip.host(debtor));
        log.add(trip.entries.clone()).unwrap();
        let end = log.fold().unwrap().bill;
        assert_eq!(end.confirmed_payments.len(), end.payments.len());
        let over = (balance + pending).max(0);
        assert!(
            net_balances(&end).unwrap()[debtor] <= over,
            "seed {seed}: {debtor} ends overpaid by what was asked"
        );
    }
    assert!(payers > 700, "{payers} payers");
    assert!(held > 100, "{held} held");
    println!("{payers} payers checked, {held} with a debt held back");
}
