//! The wallet seam, exercised without a wallet.
//!
//! The corpus pins the protocol's values and the differential lane pins its
//! answers; neither reaches `splitz_core::host`, because nothing it produces is a
//! corpus case — an entry's id depends on a clock and a nonce the host
//! supplies. What is asserted here is what a wallet meets: that every entry
//! this layer writes passes the protocol's own ingress check, that a send
//! which did not land records nothing, and that a contested payee is not
//! settled to silently.

use serde_json::{json, Value};
use std::cell::Cell;
use std::collections::BTreeSet;

use splitz_core::host::{
    accept_scan, add_expense, authored_id, base64url_no_pad, confirm_payment, create_bill,
    delta_for, invite_for, join_bill, obligation_for, read_scan, record_payment, record_send,
    set_rate, settle, shareable_bill, sign_entry, void_entry, BillHost, BillLog, Scanned,
    SendResult, Sent, SignEntry, VerifyEntry,
};
use splitz_core::{
    check_entry, net_balances, participant_id, sha256_hex, signing_message, Delta, Invite,
};

// --- a wallet that does nothing ---------------------------------------------

/// The clock is held still and the randomness is a counter: §9.3 instants
/// order a log and §9.4 derives a bill id from a nonce, so a log that moves
/// between runs cannot be asserted against a fixed expectation.
type OwnedSigner = Box<dyn Fn(&[u8]) -> String>;
type OwnedVerifier = Box<dyn Fn(&Value, &str) -> bool>;

struct FakeHost {
    me: String,
    pay_to: Option<String>,
    minute: Cell<u32>,
    counter: Cell<u8>,
    sign: Option<OwnedSigner>,
    verify: Option<OwnedVerifier>,
}

impl FakeHost {
    fn new(me: &str) -> Self {
        Self {
            me: me.to_owned(),
            pay_to: None,
            minute: Cell::new(0),
            counter: Cell::new(0),
            sign: None,
            verify: None,
        }
    }

    fn paid_at(me: &str, pay_to: &str) -> Self {
        let mut h = Self::new(me);
        h.pay_to = Some(pay_to.to_owned());
        h
    }

    /// Moves the clock on, so two entries written in one test are two entries
    /// and §10.2 has an order to put them in.
    fn tick(&self) {
        self.minute.set(self.minute.get() + 1);
    }
}

impl FakeHost {
    /// The address this fake is paid at, for a test that writes it into a join.
    fn pay_to_address(&self) -> Option<&str> {
        self.pay_to.as_deref()
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

    fn broadcast(&self, uri: &str) -> Sent {
        Sent::sent(format!("tx-{}", uri.len()))
    }

    fn signer(&self) -> Option<SignEntry<'_>> {
        self.sign.as_deref()
    }

    fn verifier(&self) -> Option<VerifyEntry<'_>> {
        self.verify.as_deref()
    }
}

/// A 32-byte key in the encoding §9.4 wants, distinct per participant.
fn fake_key(who: &str) -> String {
    let seed = who.as_bytes()[0];
    base64url_no_pad(&(0..32).map(|i| seed.wrapping_add(i)).collect::<Vec<u8>>())
}

/// A signer that is not cryptography: it names the key and digests the bytes
/// it was handed. §10.7 is about which key carries a validly signed
/// self-claim, not about which curve signs it.
///
/// The digest is what makes it a test of §10.6 as well: a signature over any
/// other bytes than `signing_message`'s fails to verify, so a signer that
/// takes its message from the wrong place is caught here rather than agreeing
/// with itself.
fn signing_host(me: &str, key: &str, pay_to: Option<&str>) -> FakeHost {
    signing_host_on(me, key, pay_to, TEST_BILL)
}

/// [`signing_host`], verifying on `bill`.
fn signing_host_on(me: &str, key: &str, pay_to: Option<&str>, bill: &str) -> FakeHost {
    let owned = key.to_owned();
    let mut h = match pay_to {
        Some(a) => FakeHost::paid_at(me, a),
        None => FakeHost::new(me),
    };
    h.sign = Some(Box::new(move |message: &[u8]| {
        format!("sig-by-{owned}:{}", sha256_hex(message))
    }));
    let bill = bill.to_owned();
    h.verify = Some(Box::new(move |entry: &Value, key: &str| {
        let Ok(message) = signing_message(entry, &bill) else {
            return false;
        };
        entry.get("sig").and_then(Value::as_str)
            == Some(&format!("sig-by-{key}:{}", sha256_hex(message.as_bytes())))
    }));
    h
}

/// The bill every entry in this file is signed and verified on (§10.6).
const TEST_BILL: &str = "host-test-bill";

fn equal_split(among: &[&str]) -> Value {
    json!({ "type": "equal", "among": among })
}

// --- entries ----------------------------------------------------------------

#[test]
fn a_create_entry_opens_a_bill_and_its_id_is_the_bill() {
    let ana = FakeHost::new("ana");
    let create = create_bill(&ana, "Dinner", "EUR", "equal", &fake_key("ana"), None).unwrap();

    // §9.4: the bill's id IS the digest of the entry that opens it, so a
    // wallet cannot choose one and two wallets cannot disagree about it.
    assert_eq!(
        create.get("id").and_then(Value::as_str),
        Some(splitz_core::derive_bill_id(&create).unwrap().as_str())
    );
    check_entry(&create).expect("a create this layer wrote passes ingress");
}

#[test]
fn two_bills_opened_at_one_instant_by_one_person_are_two_bills() {
    let ana = FakeHost::new("ana");
    let first = create_bill(&ana, "Dinner", "EUR", "equal", &fake_key("ana"), None).unwrap();
    let second = create_bill(&ana, "Dinner", "EUR", "equal", &fake_key("ana"), None).unwrap();

    // The clock has not moved and neither has the author. §9.4's nonce is the
    // only thing that separates them.
    assert_eq!(first.get("at"), second.get("at"));
    assert_ne!(first.get("nonce"), second.get("nonce"));
    assert_ne!(first.get("id"), second.get("id"));
}

#[test]
fn every_entry_kind_this_layer_writes_passes_ingress() {
    let ana = FakeHost::paid_at("ana", "u1ana");
    let written: Vec<Value> = vec![
        create_bill(&ana, "Dinner", "EUR", "equal", &fake_key("ana"), None).unwrap(),
        join_bill(
            &ana,
            Some("Ana"),
            Some("u1ana"),
            Some(&fake_key("ana")),
            None,
        )
        .unwrap(),
        add_expense(&ana, "x1", "ana", 9000, equal_split(&["ana"]), Some("food")).unwrap(),
        record_payment(
            &ana,
            "tx1",
            "ben",
            4500,
            "shieldedZec",
            None,
            None,
            None,
            None,
        )
        .unwrap(),
        confirm_payment(&ana, "tx1", "shieldedZec", Some("memo"), "r").unwrap(),
        set_rate(&ana, "EUR", 51234, Some("test")).unwrap(),
        void_entry(&ana, "e1").unwrap(),
    ];
    assert_eq!(written.len(), 7, "one per entry kind this layer writes");
    for entry in &written {
        let kind = entry.get("kind").and_then(Value::as_str).unwrap();
        check_entry(entry).unwrap_or_else(|e| panic!("{kind} was refused at ingress: {e}"));
    }
}

#[test]
fn a_wallet_that_does_not_sign_gets_its_entry_back_unsigned() {
    let ana = FakeHost::new("ana");
    assert!(ana.signer().is_none());
    let entry = join_bill(&ana, Some("Ana"), None, None, None).unwrap();
    let signed = sign_entry(&ana, &entry, TEST_BILL).unwrap();

    // Not an error, and not an empty signature either: §10.7 binds no key and
    // the fold reports no binding rather than claiming one.
    assert_eq!(signed, entry);
    assert!(signed.get("sig").is_none());
}

#[test]
fn signing_does_not_move_the_id_and_covers_the_id() {
    let ana = signing_host("ana", &fake_key("ana"), Some("u1ana"));
    let entry = join_bill(&ana, Some("Ana"), Some("u1ana"), None, None).unwrap();
    let signed = sign_entry(&ana, &entry, TEST_BILL).unwrap();

    // §9.5's digest covers every member but `id`, `sig` and `v`.
    assert_eq!(signed.get("id"), entry.get("id"));
    assert!(signed.get("sig").is_some());
    check_entry(&signed).expect("a signed entry still passes ingress");

    // §10.6's message covers `id`, so it is a message about this entry.
    assert!(signing_message(&entry, TEST_BILL)
        .unwrap()
        .contains(entry.get("id").and_then(Value::as_str).unwrap()));
}

// --- the host seam ----------------------------------------------------------

#[test]
fn a_wallet_with_no_address_to_be_paid_at_is_still_a_host() {
    let bare = FakeHost::new("ana");
    assert_eq!(bare.pay_to_address(), None);
    assert!(bare.signer().is_none());
    assert!(bare.verifier().is_none());
    // It can still open a bill and write to it.
    create_bill(&bare, "Dinner", "EUR", "equal", &fake_key("ana"), None).unwrap();
}

#[test]
fn the_clock_and_the_randomness_come_from_the_host() {
    let ana = FakeHost::new("ana");
    let before = join_bill(&ana, Some("Ana"), None, None, None).unwrap();
    ana.tick();
    let after = join_bill(&ana, Some("Ana"), None, None, None).unwrap();

    assert_ne!(before.get("at"), after.get("at"));
    assert_eq!(
        before.get("at").and_then(Value::as_str),
        Some("2026-10-28T19:30:00.000Z"),
        "§9.3 is what decides the spelling, not the host's string"
    );
}

// --- a bill, end to end -----------------------------------------------------

/// A bill two people share: ana pays 90.00, split evenly, so ben owes 45.00.
fn dinner(ana: &FakeHost, ben: &FakeHost) -> Vec<Value> {
    let create = create_bill(ana, "Dinner", "EUR", "equal", &fake_key("ana"), None).unwrap();
    ana.tick();
    let join_ana = join_bill(ana, Some("Ana"), ana.pay_to_address(), None, None).unwrap();
    ben.tick();
    ben.tick();
    let join_ben = join_bill(ben, Some("Ben"), ben.pay_to_address(), None, None).unwrap();
    ana.tick();
    let expense = add_expense(ana, "x1", "ana", 9000, equal_split(&["ana", "ben"]), None).unwrap();
    ana.tick();
    let rate = set_rate(ana, "EUR", 51234, None).unwrap();
    vec![create, join_ana, join_ben, expense, rate]
}

#[test]
fn the_record_of_a_send_is_signed_so_a_verifying_fold_keeps_it() {
    // Everyone signs, so everyone is bound, and §10.3 then applies an entry
    // authored by any of them only from a copy that verifies.
    // §10.7: a participant who publishes a key is named by the id it derives.
    let ben_id = participant_id(&fake_key("ben")).unwrap();
    let bill = |ben_signs: bool| {
        let ana = signing_host("ana", &fake_key("ana"), Some("u1ana"));
        let signer = signing_host(&ben_id, &fake_key("ben"), Some("u1ben"));
        let create = create_bill(&ana, "Dinner", "EUR", "equal", &fake_key("ana"), None).unwrap();
        let bill_id = create["id"].as_str().unwrap().to_owned();
        // Ben's device verifies on this bill, as a real one folding it would.
        let mut ben = signing_host_on(&ben_id, &fake_key("ben"), Some("u1ben"), &bill_id);
        if !ben_signs {
            ben.sign = None;
        }
        let mut entries = vec![sign_entry(&ana, &create, &bill_id).unwrap()];
        ana.tick();
        let join_ana = join_bill(&ana, Some("Ana"), Some("u1ana"), None, None).unwrap();
        entries.push(sign_entry(&ana, &join_ana, &bill_id).unwrap());
        signer.tick();
        signer.tick();
        let join_ben = join_bill(
            &signer,
            Some("Ben"),
            Some("u1ben"),
            Some(&fake_key("ben")),
            None,
        )
        .unwrap();
        entries.push(sign_entry(&signer, &join_ben, &bill_id).unwrap());
        ana.tick();
        let expense = add_expense(
            &ana,
            "x1",
            "ana",
            9000,
            equal_split(&["ana", &ben_id]),
            None,
        )
        .unwrap();
        entries.push(sign_entry(&ana, &expense, &bill_id).unwrap());
        ana.tick();
        let rate = set_rate(&ana, "EUR", 51234, None).unwrap();
        entries.push(sign_entry(&ana, &rate, &bill_id).unwrap());
        for _ in 0..5 {
            ben.tick();
        }
        (ben, bill_id, entries)
    };

    let (ben, bill_id, entries) = bill(true);
    let mut log = BillLog::with_entries(&ben, entries).for_bill(bill_id);
    let folded = log.fold().unwrap();
    assert!(folded.identities.bound.contains_key("ana"));
    assert!(folded.identities.bound.contains_key(&ben_id));
    let owed = obligation_for(&ben, &folded).unwrap().unwrap();
    let settled = settle(&ben, &mut log, &owed).unwrap();
    assert!(settled.records[0]["sig"].as_str().is_some());
    let after = log.fold().unwrap();
    assert!(after.set_aside.is_empty(), "{:?}", after.set_aside);
    assert_eq!(after.bill.payments.len(), 1);

    // The rule the signature satisfies: the same record unsigned is not the
    // bound payer speaking, and a verifying fold sets it aside.
    let (ben, bill_id, entries) = bill(false);
    let mut log = BillLog::with_entries(&ben, entries).for_bill(bill_id);
    let owed = obligation_for(&ben, &log.fold().unwrap()).unwrap().unwrap();
    let unsigned = settle(&ben, &mut log, &owed).unwrap();
    assert!(unsigned.records[0].get("sig").is_none());
    let refused = log.fold().unwrap();
    assert!(refused.bill.payments.is_empty());
    assert_eq!(refused.set_aside.len(), 1);
    assert_eq!(refused.set_aside[0].code, "unauthorized_entry");
}

#[test]
fn a_whole_bill_from_nothing_to_a_payment_request() {
    let ana = FakeHost::paid_at("ana", "u1ana");
    let ben = FakeHost::paid_at("ben", "u1ben");
    let mut log = BillLog::new(&ana);
    let refused = log.add(dinner(&ana, &ben)).unwrap();
    assert!(
        refused.is_empty(),
        "every entry this layer writes is valid: {refused:?}"
    );

    let folded = log.fold().unwrap();
    assert_eq!(
        folded
            .bill
            .participants
            .iter()
            .map(|p| p.id.as_str())
            .collect::<Vec<_>>(),
        vec!["ana", "ben"]
    );
    assert!(folded.set_aside.is_empty());

    // §4: 90.00 split evenly is 45.00 each; ana paid, so ben owes ana 45.00.
    let owed = net_balances(&folded.bill).unwrap();
    assert_eq!(owed.get("ana"), Some(&4500));
    assert_eq!(owed.get("ben"), Some(&-4500));

    // ana is owed, so ana has nothing to pay.
    let ana_owes = obligation_for(&ana, &folded).unwrap().unwrap();
    assert!(ana_owes.settlements.is_empty());

    // ben owes, and ana can be paid, so one request carries the whole debt.
    let ben_owes = obligation_for(&ben, &folded).unwrap().unwrap();
    assert_eq!(ben_owes.settlements.len(), 1);
    assert_eq!(ben_owes.settlements[0].to, "ana");
    assert_eq!(ben_owes.settlements[0].amount, 4500);
    assert!(ben_owes.request.unpayable.is_empty());
    assert!(ben_owes.is_complete());
    assert!(ben_owes.uri().unwrap().starts_with("zcash:u1ana"));
}

#[test]
fn a_recipient_with_no_address_is_reported_never_dropped() {
    let ana = FakeHost::new("ana");
    let ben = FakeHost::paid_at("ben", "u1ben");
    let mut log = BillLog::new(&ana);
    log.add(dinner(&ana, &ben)).unwrap();
    let folded = log.fold().unwrap();

    let ben_owes = obligation_for(&ben, &folded).unwrap().unwrap();

    // The debt exists and cannot be carried. Both facts survive.
    assert_eq!(ben_owes.settlements.len(), 1);
    assert_eq!(ben_owes.settlements[0].to, "ana");
    assert_eq!(ben_owes.request.unpayable.len(), 1);
    assert_eq!(ben_owes.request.unpayable[0].id, "ana");
    assert_eq!(ben_owes.request.unpayable[0].reason, "no_address");
    assert_eq!(ben_owes.withheld_minor_units(), 4500);
    assert!(
        !ben_owes.is_complete(),
        "a request that covers less than the plan must say so"
    );
}

#[test]
fn an_unpriced_bill_is_an_ordinary_bill_not_a_refusal() {
    let ana = FakeHost::paid_at("ana", "u1ana");
    let create = create_bill(&ana, "Dinner", "EUR", "equal", &fake_key("ana"), None).unwrap();
    ana.tick();
    let join = join_bill(&ana, Some("Ana"), Some("u1ana"), None, None).unwrap();
    let mut log = BillLog::new(&ana);
    log.add(vec![create, join]).unwrap();
    let folded = log.fold().unwrap();

    assert!(folded.bill.rate.is_none());
    assert!(
        obligation_for(&ana, &folded).unwrap().is_none(),
        "there is no §12 code for unpriced, so there is none here"
    );
}

// --- sending ----------------------------------------------------------------

#[test]
fn a_sent_request_records_what_was_owed_when_it_was_made() {
    let ana = FakeHost::paid_at("ana", "u1ana");
    let ben = FakeHost::paid_at("ben", "u1ben");
    let mut log = BillLog::new(&ben);
    log.add(dinner(&ana, &ben)).unwrap();
    let folded = log.fold().unwrap();
    let owed = obligation_for(&ben, &folded).unwrap().unwrap();

    let settled = settle(&ben, &mut log, &owed).unwrap();
    assert_eq!(settled.result, SendResult::Sent);
    assert_eq!(settled.records.len(), 1);
    let payment = &settled.records[0]["payment"];
    assert_eq!(payment["to"], json!("ana"));
    assert_eq!(payment["amount"], json!(4500));
    let txid = settled.txid.clone().unwrap();
    assert_eq!(payment["id"], json!(format!("ben:{txid}:ana")));
    assert_eq!(payment["reference"], json!(txid));

    // §10.5: a record is a claim. The balance has not moved.
    let after = log.fold().unwrap();
    assert_eq!(net_balances(&after.bill).unwrap().get("ben"), Some(&-4500));
}

/// A wallet that builds and signs but does not broadcast.
struct PendingHost(FakeHost);

impl BillHost for PendingHost {
    fn me(&self) -> &str {
        self.0.me()
    }
    fn now(&self) -> String {
        self.0.now()
    }
    fn random_bytes(&self, n: usize) -> Vec<u8> {
        self.0.random_bytes(n)
    }
    fn broadcast(&self, _uri: &str) -> Sent {
        Sent::pending(Some("created, not broadcast".to_owned()), None)
    }
}

#[test]
fn a_send_that_was_built_but_not_broadcast_records_nothing() {
    let ana = FakeHost::paid_at("ana", "u1ana");
    let ben = FakeHost::paid_at("ben", "u1ben");
    let entries = dinner(&ana, &ben);
    let pending = PendingHost(FakeHost::paid_at("ben", "u1ben"));
    let mut log = BillLog::new(&pending);
    log.add(entries).unwrap();
    let folded = log.fold().unwrap();
    let owed = obligation_for(&pending, &folded).unwrap().unwrap();

    let settled = settle(&pending, &mut log, &owed).unwrap();
    assert_eq!(settled.result, SendResult::Pending);
    assert!(settled.records.is_empty());
    assert_eq!(settled.txid, None);
    assert_eq!(settled.detail.as_deref(), Some("created, not broadcast"));

    // The debt stays exactly as it was: neither settled nor retried.
    let after = log.fold().unwrap();
    assert!(after.bill.payments.is_empty());
}

#[test]
fn a_confirmation_is_what_clears_the_debt() {
    let ana = FakeHost::paid_at("ana", "u1ana");
    let ben = FakeHost::paid_at("ben", "u1ben");
    let mut log = BillLog::new(&ben);
    log.add(dinner(&ana, &ben)).unwrap();
    let folded = log.fold().unwrap();
    let owed = obligation_for(&ben, &folded).unwrap().unwrap();
    let settled = settle(&ben, &mut log, &owed).unwrap();
    let txid = settled.txid.unwrap();
    let payment_id = format!("ben:{txid}:ana");

    // Recorded, not confirmed: ben still owes.
    let before = log.fold().unwrap();
    assert_eq!(net_balances(&before.bill).unwrap().get("ben"), Some(&-4500));

    // §10.5: the payee confirms, and with a method §10.5 defines. The
    // payment's own `shieldedZec` is not one of them — a confirmation names
    // how the money was seen to arrive, not how it was sent — and an entry
    // saying otherwise is set aside rather than settling anything.
    ana.tick();
    let digest = log.fold().unwrap().payment_digests[&payment_id].clone();
    let wrong = confirm_payment(&ana, &payment_id, "shieldedZec", None, &digest).unwrap();
    let mut aside = BillLog::with_entries(&ben, log.entries());
    aside.add(vec![wrong]).unwrap();
    assert_eq!(
        aside
            .fold()
            .unwrap()
            .set_aside
            .iter()
            .map(|s| s.code)
            .collect::<Vec<_>>(),
        vec!["bill_unknown_confirmation_method"]
    );

    ana.tick();
    let confirmation =
        confirm_payment(&ana, &payment_id, "recipientConfirmed", None, &digest).unwrap();
    log.add(vec![confirmation]).unwrap();

    let after = log.fold().unwrap();
    assert!(after.bill.confirmed_payments.contains(&payment_id));
    assert_eq!(net_balances(&after.bill).unwrap().get("ben"), Some(&0));
}

#[test]
fn a_debt_already_paid_and_not_yet_confirmed_is_not_requested_again() {
    let ana = FakeHost::paid_at("ana", "u1ana");
    let ben = FakeHost::paid_at("ben", "u1ben");
    let mut log = BillLog::new(&ben);
    log.add(dinner(&ana, &ben)).unwrap();
    let folded = log.fold().unwrap();
    let owed = obligation_for(&ben, &folded).unwrap().unwrap();
    settle(&ben, &mut log, &owed).unwrap();

    let again = obligation_for(&ben, &log.fold().unwrap()).unwrap().unwrap();
    assert!(
        again.settlements.is_empty(),
        "asking again would send the same money twice"
    );
    assert_eq!(again.awaiting.len(), 1);
    assert_eq!(again.awaiting[0].to, "ana");
    assert_eq!(again.awaiting[0].paid, 4500);
}

// --- identity ---------------------------------------------------------------

/// ana's signed create, her signed join, ben's signed self-claim under the id
/// his key derives, ben's expense splitting 90.00 with ana, and ana's rate.
fn a_dinner_ben_paid(ana: &FakeHost, ben: &FakeHost, ana_key: &str, ben_key: &str) -> Vec<Value> {
    let ben_id = participant_id(ben_key).unwrap();
    let mut entries = vec![sign_entry(
        ana,
        &create_bill(ana, "Dinner", "EUR", "equal", ana_key, None).unwrap(),
        TEST_BILL,
    )
    .unwrap()];
    ana.tick();
    entries.push(
        sign_entry(
            ana,
            &join_bill(ana, Some("Ana"), Some("u1ana"), Some(ana_key), None).unwrap(),
            TEST_BILL,
        )
        .unwrap(),
    );
    ben.tick();
    ben.tick();
    entries.push(
        sign_entry(
            ben,
            &join_bill(ben, Some("Ben"), Some("u1ben"), Some(ben_key), None).unwrap(),
            TEST_BILL,
        )
        .unwrap(),
    );
    ben.tick();
    entries.push(
        sign_entry(
            ben,
            &add_expense(
                ben,
                "x1",
                &ben_id,
                9000,
                equal_split(&["ana", &ben_id]),
                None,
            )
            .unwrap(),
            TEST_BILL,
        )
        .unwrap(),
    );
    ana.tick();
    entries.push(sign_entry(ana, &set_rate(ana, "EUR", 51234, None).unwrap(), TEST_BILL).unwrap());
    entries
}

#[test]
fn a_rival_key_cannot_claim_a_bound_participant() {
    // §10.7. A participant who publishes a key is named by the id that key
    // derives, so no second key can claim them.
    let ana_key = fake_key("ana");
    let ben_key = fake_key("ben");
    let impostor_key = fake_key("zzz");
    let ben_id = participant_id(&ben_key).unwrap();
    let ana = signing_host("ana", &ana_key, Some("u1ana"));
    let ben = signing_host(&ben_id, &ben_key, Some("u1ben"));
    let impostor = signing_host(&ben_id, &impostor_key, Some("u1impostor"));

    let mut log = BillLog::new(&ana);
    log.add(a_dinner_ben_paid(&ana, &ben, &ana_key, &ben_key))
        .unwrap();
    assert_eq!(
        log.fold().unwrap().identities.bound.get(&ben_id),
        Some(&ben_key)
    );

    for _ in 0..5 {
        impostor.tick();
    }
    let rival = sign_entry(
        &impostor,
        &join_bill(
            &impostor,
            Some("Ben"),
            Some("u1impostor"),
            Some(&impostor_key),
            None,
        )
        .unwrap(),
        TEST_BILL,
    )
    .unwrap();
    let rival_id = rival["id"].as_str().unwrap().to_owned();
    log.add(vec![rival]).unwrap();

    let after = log.fold().unwrap();
    assert_eq!(
        after.identities.bound.get(&ben_id),
        Some(&ben_key),
        "the rival claim leaves ben's binding as it was"
    );
    // Written as ben and signed with another key, it is refused before its
    // record is read: an entry by a bound participant verifies against their
    // key (§10.3).
    assert!(after
        .set_aside
        .iter()
        .any(|a| a.id == rival_id && a.code == splitz_core::code::UNAUTHORIZED_ENTRY));
    assert_eq!(
        after.bill.participant(&ben_id).unwrap().pay_to.as_deref(),
        Some("u1ben")
    );
}

#[test]
fn a_rival_claim_does_not_change_who_a_payer_pays() {
    let ana_key = fake_key("ana");
    let ben_key = fake_key("ben");
    let impostor_key = fake_key("zzz");
    let ben_id = participant_id(&ben_key).unwrap();
    let ana = signing_host("ana", &ana_key, Some("u1ana"));
    let ben = signing_host(&ben_id, &ben_key, Some("u1ben"));
    let impostor = signing_host(&ben_id, &impostor_key, Some("u1impostor"));

    let mut log = BillLog::new(&ana);
    log.add(a_dinner_ben_paid(&ana, &ben, &ana_key, &ben_key))
        .unwrap();
    let before = obligation_for(&ana, &log.fold().unwrap()).unwrap().unwrap();
    assert!(before.uri().unwrap().starts_with("zcash:u1ben"));

    for _ in 0..5 {
        impostor.tick();
    }
    let rival = sign_entry(
        &impostor,
        &join_bill(
            &impostor,
            Some("Ben"),
            Some("u1impostor"),
            Some(&impostor_key),
            None,
        )
        .unwrap(),
        TEST_BILL,
    )
    .unwrap();
    log.add(vec![rival]).unwrap();

    let after = obligation_for(&ana, &log.fold().unwrap()).unwrap().unwrap();
    assert_eq!(
        after.uri(),
        before.uri(),
        "the request pays the address ben published, not the rival"
    );
    assert_eq!(after.settlements.len(), 1);
    assert_eq!(after.settlements[0].to, ben_id);
    assert_eq!(after.settlements[0].amount, 4500);
}

// --- sharing ----------------------------------------------------------------

#[test]
fn an_invite_round_trips_through_the_protocol_parser() {
    let ana = FakeHost::paid_at("ana", "u1ana");
    let key = fake_key("ana");
    let create = create_bill(&ana, "Dinner", "EUR", "equal", &key, None).unwrap();
    ana.tick();
    let join = join_bill(&ana, Some("Ana"), Some("u1ana"), None, None).unwrap();
    let mut log = BillLog::new(&ana);
    log.add(vec![create, join]).unwrap();
    let bill = log.fold().unwrap().bill;

    let uri = invite_for(&bill, &key, Some("Dinner"), None).unwrap();
    match read_scan(&uri) {
        Scanned::Invite(Invite {
            bill_id, key: k, ..
        }) => {
            assert_eq!(bill_id, bill.id);
            assert_eq!(k, key);
        }
        other => panic!("an invite read as {other:?}"),
    }
}

#[test]
fn a_whole_bill_travels_in_one_square_and_opens_on_the_other_side() {
    let ana = FakeHost::paid_at("ana", "u1ana");
    let key = fake_key("ana");
    let create = create_bill(&ana, "Dinner", "EUR", "equal", &key, None).unwrap();
    ana.tick();
    let join = join_bill(&ana, Some("Ana"), None, None, None).unwrap();
    let mut log = BillLog::new(&ana);
    log.add(vec![create, join]).unwrap();
    let bill = log.fold().unwrap().bill;

    let square = shareable_bill(&log, &key, &bill).expect("two entries fit one square");
    let ben = FakeHost::new("ben");
    let mut theirs = BillLog::new(&ben);
    match read_scan(&square) {
        Scanned::Bill(scan) => {
            assert!(scan.invite.is_some(), "§11.2 carries the invite");
            let refused = accept_scan(&mut theirs, scan).unwrap();
            assert!(refused.is_empty());
        }
        other => panic!("a payload read as {other:?}"),
    }
    assert!(theirs.opens_a_bill());
    assert_eq!(theirs.fold().unwrap().bill.id, bill.id);
    let theirs_bill = theirs.fold().unwrap().bill;
    assert!(
        theirs_bill.participant(ben.me()).is_none(),
        "holding a bill is not being on it"
    );
}

#[test]
fn a_delta_carries_only_what_the_peer_has_not_seen_and_no_key() {
    let ana = FakeHost::paid_at("ana", "u1ana");
    let create = create_bill(&ana, "Dinner", "EUR", "equal", &fake_key("ana"), None).unwrap();
    ana.tick();
    let join = join_bill(&ana, Some("Ana"), None, None, None).unwrap();
    let mut log = BillLog::new(&ana);
    log.add(vec![create.clone(), join]).unwrap();

    let mut theirs = BTreeSet::new();
    theirs.insert(create["id"].as_str().unwrap().to_owned());

    match delta_for(&log, &theirs) {
        Delta::Square { uri, entry_count } => {
            assert_eq!(entry_count, 1, "only the entry the peer lacks");
            assert!(uri.starts_with("splitzd1:"), "a delta carries no invite");
        }
        other => panic!("a one-entry delta came back as {other:?}"),
    }

    // Nothing missing and too much missing are different answers.
    let all: BTreeSet<String> = log
        .entries()
        .iter()
        .map(|e| e["id"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(delta_for(&log, &all), Delta::NothingMissing);
}

#[test]
fn something_that_is_neither_is_refused_with_a_code() {
    match read_scan("not a splitz anything") {
        Scanned::Refused(code) => assert!(!code.is_empty()),
        other => panic!("nonsense read as {other:?}"),
    }
    // A payload that claims to be one is answered as one, not as a bad invite.
    match read_scan("splitz1:!!!!") {
        Scanned::Refused(code) => assert_eq!(code, "payload_damaged"),
        other => panic!("a damaged payload read as {other:?}"),
    }
}

#[test]
fn an_invite_member_that_is_not_a_string_is_refused_not_panicked() {
    // §11.2 carries the invite verbatim and validates nothing inside it, so
    // every member is whatever a peer wrote. A camera is pointed at this.
    for bad in [json!(5), json!(true), json!([]), json!({}), Value::Null] {
        let body = json!({ "v": 1, "log": [], "invite": { "v": 1, "b": bad, "k": "Kk" } });
        let text = splitz_core::encode_payload(splitz_core::BILL_PREFIX, &body).unwrap();
        match read_scan(&text) {
            Scanned::Bill(scan) => assert!(scan.invite.is_none()),
            other => panic!("a payload read as {other:?}"),
        }
    }
}

/// Three on a bill, and one of them owes the other two: ben pays 90.00 and cat
/// pays 90.00, each split evenly across all three, so ana owes 30.00 to ben and
/// 30.00 to cat — two settlements carried by one transaction.
fn two_debts(ana: &FakeHost, ben: &FakeHost, cat: &FakeHost) -> Vec<Value> {
    let create = create_bill(ana, "Dinner", "EUR", "equal", &fake_key("ana"), None).unwrap();
    ana.tick();
    let join_ana = join_bill(ana, Some("Ana"), ana.pay_to_address(), None, None).unwrap();
    ben.tick();
    ben.tick();
    let join_ben = join_bill(ben, Some("Ben"), ben.pay_to_address(), None, None).unwrap();
    cat.tick();
    cat.tick();
    cat.tick();
    let join_cat = join_bill(cat, Some("Cat"), cat.pay_to_address(), None, None).unwrap();
    ben.tick();
    let e1 = add_expense(
        ben,
        "x1",
        "ben",
        9000,
        equal_split(&["ana", "ben", "cat"]),
        None,
    )
    .unwrap();
    cat.tick();
    let e2 = add_expense(
        cat,
        "x2",
        "cat",
        9000,
        equal_split(&["ana", "ben", "cat"]),
        None,
    )
    .unwrap();
    ana.tick();
    let rate = set_rate(ana, "EUR", 51234, None).unwrap();
    vec![create, join_ana, join_ben, join_cat, e1, e2, rate]
}

/// `FakeHost`, with a ZIP 321 reader that refuses one address.
struct Refusing<'a> {
    inner: &'a FakeHost,
    unread: &'a str,
}

impl BillHost for Refusing<'_> {
    fn me(&self) -> &str {
        self.inner.me()
    }

    fn now(&self) -> String {
        self.inner.now()
    }

    fn random_bytes(&self, byte_count: usize) -> Vec<u8> {
        self.inner.random_bytes(byte_count)
    }

    fn broadcast(&self, uri: &str) -> Sent {
        self.inner.broadcast(uri)
    }

    fn reads_address(&self, address: &str) -> bool {
        address != self.unread
    }
}

#[test]
fn an_address_the_payer_cannot_read_is_unpayable_and_the_rest_is_paid() {
    // Cat's `payTo` passes §8.3's alphabet but this wallet's reader refuses
    // it; a request naming it would be refused whole, so ben's share goes out
    // alone.
    let ana = FakeHost::paid_at("ana", "u1ana");
    let ben = FakeHost::paid_at("ben", "u1ben");
    let cat = FakeHost::paid_at("cat", "u1cat");
    let mut log = BillLog::new(&ana);
    assert!(log.add(two_debts(&ana, &ben, &cat)).unwrap().is_empty());
    let folded = log.fold().unwrap();

    let reading = Refusing {
        inner: &ana,
        unread: "u1cat",
    };
    let owed = obligation_for(&reading, &folded).unwrap().unwrap();
    let unpayable: Vec<String> = owed
        .request
        .unpayable
        .iter()
        .map(|u| format!("{}:{}", u.id, u.reason))
        .collect();
    assert_eq!(unpayable, vec!["cat:bad_address"]);
    assert_eq!(owed.request.recipients, vec!["ben"]);
    assert_eq!(owed.request.carried_minor_units, 3000);
    assert!(!owed.uri().unwrap().contains("u1cat"));

    let everything = obligation_for(&ana, &folded).unwrap().unwrap();
    assert!(everything.request.unpayable.is_empty());
    assert_eq!(everything.request.carried_minor_units, 6000);
}

#[test]
fn one_transaction_paying_two_people_is_two_records_each_confirmable() {
    // §10.5 gives every record its own id. Under one shared id the fold sets
    // the second aside as `duplicate_payment`, so a payment that was made
    // leaves no record on the bill: its payee is still shown as owed, cannot
    // confirm — the surviving record names somebody else — and the payer has
    // already sent the money.
    let ana = FakeHost::paid_at("ana", "u1ana");
    let ben = FakeHost::paid_at("ben", "u1ben");
    let cat = FakeHost::paid_at("cat", "u1cat");
    let mut log = BillLog::new(&ana);
    let refused = log.add(two_debts(&ana, &ben, &cat)).unwrap();
    assert!(refused.is_empty(), "{refused:?}");

    let folded = log.fold().unwrap();
    let owed = obligation_for(&ana, &folded).unwrap().unwrap();
    assert_eq!(
        owed.settlements
            .iter()
            .map(|s| s.to.as_str())
            .collect::<Vec<_>>(),
        vec!["ben", "cat"]
    );

    let settled = settle(&ana, &mut log, &owed).unwrap();
    let txid = settled.txid.clone().unwrap();
    assert_eq!(settled.records.len(), 2);
    assert_eq!(
        settled
            .records
            .iter()
            .map(|r| r["payment"]["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        vec![format!("ana:{txid}:ben"), format!("ana:{txid}:cat")]
    );
    for record in &settled.records {
        // The transaction ties both records to the chain, and is what
        // `onChain` reads.
        assert_eq!(record["payment"]["reference"], json!(txid));
    }

    let after = log.fold().unwrap();
    assert!(
        after.set_aside.is_empty(),
        "neither record is a duplicate of the other: {:?}",
        after.set_aside
    );
    // §10.2 orders the log, and these two records share an instant, so the
    // tiebreak is by entry id — which depends on this test's own clock and
    // nonce. What is asserted is that both survive, not the order they land in.
    let mut paid: Vec<&str> = after.bill.payments.iter().map(|p| p.to.as_str()).collect();
    paid.sort_unstable();
    assert_eq!(paid, vec!["ben", "cat"]);

    // ben confirms his own record and clears his own half. cat's stands.
    for _ in 0..8 {
        ben.tick();
    }
    let payment_id = format!("ana:{txid}:ben");
    let digest = log.fold().unwrap().payment_digests[&payment_id].clone();
    let confirmation =
        confirm_payment(&ben, &payment_id, "recipientConfirmed", None, &digest).unwrap();
    log.add(vec![confirmation]).unwrap();

    let end = log.fold().unwrap();
    assert!(end.set_aside.is_empty(), "{:?}", end.set_aside);
    assert!(end.bill.confirmed_payments.contains(&payment_id));
    let balances = net_balances(&end.bill).unwrap();
    assert_eq!(balances.get("ben"), Some(&0));
    assert_eq!(balances.get("cat"), Some(&3000));
    assert_eq!(balances.get("ana"), Some(&-3000));
}

#[test]
fn items_in_a_scanned_log_that_are_not_entries_are_refused_not_dropped() {
    // The same text the Dart seam reads in `sharing_test.dart`: three
    // non-objects and no entry. Each is refused as §10.1 refuses it.
    let text = "splitz1:eyJsb2ciOlsxLCJ4IixudWxsXSwidiI6MX0";
    let Scanned::Bill(scan) = read_scan(text) else {
        panic!("a payload");
    };
    let ana = FakeHost::new("ana");
    let mut log = BillLog::new(&ana);
    let refused = accept_scan(&mut log, scan).unwrap();
    let rows: Vec<String> = refused
        .iter()
        .map(|r| format!("{}:{}", r.id, r.code))
        .collect();
    assert_eq!(
        rows,
        vec![":bill_type_error", ":bill_type_error", ":bill_type_error"]
    );
}

#[test]
fn a_pending_send_found_on_chain_later_records_what_a_sent_one_would() {
    // Two logs from one bill: in one the send succeeds at once; in the other
    // it is left pending and recorded afterwards from the txid. The payments
    // are the same, so a payee confirming either confirms the same one.
    let ana = FakeHost::paid_at("ana", "u1ana");
    let ben = FakeHost::paid_at("ben", "u1ben");
    let cat = FakeHost::paid_at("cat", "u1cat");
    let entries = two_debts(&ana, &ben, &cat);

    let mut now = BillLog::new(&ana);
    now.add(entries.clone()).unwrap();
    let owed = obligation_for(&ana, &now.fold().unwrap()).unwrap().unwrap();
    let sent = settle(&ana, &mut now, &owed).unwrap();
    assert_eq!(sent.result, SendResult::Sent);
    let txid = sent.txid.clone().unwrap();

    let pending = PendingHost(FakeHost::paid_at("ana", "u1ana"));
    let mut later = BillLog::new(&pending);
    later.add(entries).unwrap();
    let owed = obligation_for(&pending, &later.fold().unwrap())
        .unwrap()
        .unwrap();
    assert!(settle(&pending, &mut later, &owed)
        .unwrap()
        .records
        .is_empty());
    let carried = owed.carried_to();
    assert_eq!(
        carried
            .iter()
            .map(|(k, v)| (k.as_str(), *v))
            .collect::<Vec<_>>(),
        vec![("ben", 3000), ("cat", 3000)]
    );

    let records = record_send(
        &pending,
        &mut later,
        &carried,
        &txid,
        &owed.carried_zatoshi(),
        Some(&owed.rate),
    )
    .unwrap();
    // `at` is when the record was written, which is later for a recovery.
    let payments = |rs: &[Value]| {
        rs.iter()
            .map(|r| {
                let mut p = r["payment"].clone();
                p.as_object_mut().unwrap().remove("at");
                p
            })
            .collect::<Vec<_>>()
    };
    assert_eq!(payments(&records), payments(&sent.records));
    assert!(later.fold().unwrap().set_aside.is_empty());
}

#[test]
fn an_id_is_written_under_its_author_once_whatever_the_author_holds() {
    assert_eq!(authored_id("ana", "hotel"), "ana:hotel");
    assert_eq!(authored_id("ana", "ana:hotel"), "ana:hotel");
    // §10.3 step 5 gives an author holding `:` no minted ids; the builder
    // still writes the same id for one, in both implementations.
    assert_eq!(authored_id("ben:t1", "ben:t1:ana"), "ben:t1:ana");
    assert_eq!(authored_id("ben:t1", "ana"), "ben:t1:ana");
}
