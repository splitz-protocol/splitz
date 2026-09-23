//! The log read as a history.
//!
//! A folded bill says what is true now. These assert the three things only the
//! log shows: an entry somebody withdrew, an entry the fold refused, and a
//! payment claimed but not confirmed.

mod support;

use serde_json::{json, Value};
use splitz_core::host::{
    add_expense, base64url_no_pad, confirm_payment, create_bill, join_bill, record_payment,
    set_rate, BillLog, CREATOR_KEY_BYTES,
};
use splitz_host::{
    activity_of, awaiting_confirmation_by, BillEvent, BillEventKind, WalletBillHost,
};
use support::FakeWallet;

/// A 32-byte key in the encoding §9.4 wants, distinct per participant.
fn fake_key(who: &str) -> String {
    let first = who.as_bytes()[0];
    base64url_no_pad(
        &(0..CREATOR_KEY_BYTES)
            .map(|i| first.wrapping_add(i as u8))
            .collect::<Vec<u8>>(),
    )
}

/// Folds `entries` and reads them as a history.
fn history_of(wallet: &FakeWallet, entries: Vec<Value>) -> Vec<BillEvent> {
    let host = WalletBillHost::new(wallet);
    let log = BillLog::with_entries(&host, entries);
    let folded = log.fold().expect("the log folds");
    activity_of(
        &log.entries(),
        &folded.bill,
        &folded.set_aside,
        &folded.withdrawn,
    )
}

#[test]
fn every_entry_becomes_a_line_newest_first() {
    let ana = FakeWallet::new("ana", Some("u1ana"));
    let host = WalletBillHost::new(&ana);
    let create = create_bill(&host, "Dinner", "USD", "equal", &fake_key("ana")).unwrap();
    // §10.2 orders by instant and breaks a tie by entry id, so two entries
    // written at one instant are ordered by a digest rather than by which was
    // written first. The clock moves so the history has a clock order to show.
    ana.tick();
    let join = join_bill(&host, Some("Ana"), Some("u1ana"), None, None).unwrap();
    ana.tick();
    let expense = add_expense(
        &host,
        "x1",
        "ana",
        9000,
        json!({"type": "equal", "among": ["ana"]}),
        Some("dinner"),
    )
    .unwrap();

    let history = history_of(&ana, vec![create, join, expense]);
    assert_eq!(history[0].kind, BillEventKind::ExpenseAdded);
    assert_eq!(history[0].amount_minor_units, Some(9000));
    assert_eq!(history[0].description.as_deref(), Some("dinner"));
    assert_eq!(history.last().unwrap().kind, BillEventKind::Opened);
    assert!(history.iter().all(BillEvent::applied));
}

#[test]
fn a_second_join_carrying_an_address_is_an_address_change() {
    // §13 requires a payer be shown a changed address before settling to it,
    // so it is its own event rather than a second "joined".
    let ana = FakeWallet::new("ana", Some("u1ana"));
    let host = WalletBillHost::new(&ana);
    let create = create_bill(&host, "Dinner", "USD", "equal", &fake_key("ana")).unwrap();
    ana.tick();
    let first = join_bill(&host, Some("Ana"), Some("u1ana"), None, None).unwrap();
    ana.tick();
    let second = join_bill(&host, Some("Ana"), Some("u1ana-new"), None, None).unwrap();

    let history = history_of(&ana, vec![create, first, second]);
    let kinds: Vec<BillEventKind> = history.iter().map(|e| e.kind).collect();
    assert_eq!(
        kinds,
        vec![
            BillEventKind::AddressChanged,
            BillEventKind::Joined,
            BillEventKind::Opened
        ]
    );
}

#[test]
fn a_join_with_no_address_is_not_an_address_change() {
    let ana = FakeWallet::new("ana", Some("u1ana"));
    let host = WalletBillHost::new(&ana);
    let create = create_bill(&host, "Dinner", "USD", "equal", &fake_key("ana")).unwrap();
    ana.tick();
    let first = join_bill(&host, Some("Ana"), Some("u1ana"), None, None).unwrap();
    ana.tick();
    let renamed = join_bill(&host, Some("Ana B"), None, None, None).unwrap();

    let history = history_of(&ana, vec![create, first, renamed]);
    assert!(history
        .iter()
        .all(|e| e.kind != BillEventKind::AddressChanged));
}

#[test]
fn a_priced_bill_says_what_it_was_priced_at() {
    let ana = FakeWallet::new("ana", Some("u1ana"));
    let host = WalletBillHost::new(&ana);
    let create = create_bill(&host, "Dinner", "USD", "equal", &fake_key("ana")).unwrap();
    ana.tick();
    let join = join_bill(&host, Some("Ana"), Some("u1ana"), None, None).unwrap();
    ana.tick();
    let rate = set_rate(&host, "USD", 300_000, Some("a feed")).unwrap();

    let history = history_of(&ana, vec![create, join, rate]);
    let priced = history
        .iter()
        .find(|e| e.kind == BillEventKind::Priced)
        .expect("the rate is a line");
    assert_eq!(priced.amount_minor_units, Some(300_000));
    assert_eq!(priced.description.as_deref(), Some("a feed"));
}

/// The digest a confirmation of the log's payment record carries (§10.5).
fn record_digest(entries: &[Value]) -> String {
    let record = entries
        .iter()
        .find(|e| e["kind"] == "recordPayment")
        .expect("the log holds a payment");
    splitz_core::payment_digest(&record["payment"]).unwrap()
}

/// A bill two people are on, one owing the other.
fn a_bill_with_a_payment(reference: Option<&str>, method: &str) -> (FakeWallet, Vec<Value>) {
    let ana = FakeWallet::new("ana", Some("u1ana"));
    let ben = FakeWallet::new("ben", Some("u1ben"));
    let create = create_bill(
        &WalletBillHost::new(&ana),
        "Dinner",
        "USD",
        "equal",
        &fake_key("ana"),
    )
    .unwrap();
    ana.tick();
    let ana_join = join_bill(
        &WalletBillHost::new(&ana),
        Some("Ana"),
        Some("u1ana"),
        None,
        None,
    )
    .unwrap();
    ben.tick();
    ben.tick();
    let ben_join = join_bill(
        &WalletBillHost::new(&ben),
        Some("Ben"),
        Some("u1ben"),
        None,
        None,
    )
    .unwrap();
    ana.tick();
    let expense = add_expense(
        &WalletBillHost::new(&ana),
        "x1",
        "ana",
        9000,
        json!({"type": "equal", "among": ["ana", "ben"]}),
        Some("dinner"),
    )
    .unwrap();
    ben.tick();
    let payment = record_payment(
        &WalletBillHost::new(&ben),
        "pay-1",
        "ana",
        4500,
        method,
        reference,
        None,
        None,
        None,
    )
    .unwrap();
    (ana, vec![create, ana_join, ben_join, expense, payment])
}

#[test]
fn an_unconfirmed_payment_says_so() {
    // **A recorded payment is a claim.** Presenting an unconfirmed one as
    // settled tells a payer a debt is discharged the payee never agreed was
    // paid.
    let (ana, entries) = a_bill_with_a_payment(Some("tx-1"), "shieldedZec");
    let history = history_of(&ana, entries);
    let recorded = history
        .iter()
        .find(|e| e.kind == BillEventKind::PaymentRecorded)
        .expect("the payment is a line");
    assert!(!recorded.confirmed);
    assert_eq!(recorded.subject.as_deref(), Some("ana"));
    assert_eq!(recorded.amount_minor_units, Some(4500));
}

#[test]
fn the_payees_confirmation_flips_it_and_is_its_own_line() {
    let (ana, mut entries) = a_bill_with_a_payment(Some("tx-1"), "shieldedZec");
    ana.tick();
    let digest = record_digest(&entries);
    let confirmation = confirm_payment(
        &WalletBillHost::new(&ana),
        "pay-1",
        // §10.5: only the recipient settles a debt. `shieldedZec` names a
        // transaction and speaks for nobody in particular.
        "recipientConfirmed",
        None,
        &digest,
    )
    .unwrap();
    entries.push(confirmation);

    let history = history_of(&ana, entries);
    let recorded = history
        .iter()
        .find(|e| e.kind == BillEventKind::PaymentRecorded)
        .unwrap();
    assert!(recorded.confirmed);
    assert!(history
        .iter()
        .any(|e| e.kind == BillEventKind::PaymentConfirmed));
}

#[test]
fn a_swap_carries_its_reference_which_is_not_a_txid() {
    let (ana, entries) = a_bill_with_a_payment(Some("swap-9"), "swap");
    let history = history_of(&ana, entries);
    let recorded = history
        .iter()
        .find(|e| e.kind == BillEventKind::PaymentRecorded)
        .unwrap();
    assert_eq!(recorded.method.as_deref(), Some("swap"));
    assert_eq!(recorded.reference.as_deref(), Some("swap-9"));
}

#[test]
fn only_the_payee_is_offered_the_confirmation() {
    // A payer who could confirm their own payment would settle a debt by
    // asserting twice that they paid it.
    let (ana, entries) = a_bill_with_a_payment(Some("tx-1"), "shieldedZec");
    let host = WalletBillHost::new(&ana);
    let folded = BillLog::with_entries(&host, entries).fold().unwrap();
    assert_eq!(awaiting_confirmation_by(&folded.bill, "ana").len(), 1);
    assert!(awaiting_confirmation_by(&folded.bill, "ben").is_empty());
}

#[test]
fn a_confirmed_payment_leaves_the_waiting_list() {
    let (ana, mut entries) = a_bill_with_a_payment(Some("tx-1"), "shieldedZec");
    ana.tick();
    let digest = record_digest(&entries);
    entries.push(
        confirm_payment(
            &WalletBillHost::new(&ana),
            "pay-1",
            // §10.5: only the recipient settles a debt. `shieldedZec` names a
            // transaction and speaks for nobody in particular.
            "recipientConfirmed",
            None,
            &digest,
        )
        .unwrap(),
    );
    let host = WalletBillHost::new(&ana);
    let folded = BillLog::with_entries(&host, entries).fold().unwrap();
    assert!(awaiting_confirmation_by(&folded.bill, "ana").is_empty());
}

#[test]
fn a_refused_entry_stays_in_the_history_with_its_code() {
    // Shown, never hidden: an entry that vanished silently is
    // indistinguishable from one that was never sent.
    let ana = FakeWallet::new("ana", Some("u1ana"));
    let host = WalletBillHost::new(&ana);
    let create = create_bill(&host, "Dinner", "USD", "equal", &fake_key("ana")).unwrap();
    ana.tick();
    let join = join_bill(&host, Some("Ana"), Some("u1ana"), None, None).unwrap();
    ana.tick();
    // §10.3 refuses a payment whose payer and payee are one person.
    let self_payment =
        record_payment(&host, "pay-1", "ana", 100, "cash", None, None, None, None).unwrap();

    let history = history_of(&ana, vec![create, join, self_payment]);
    let refused = history
        .iter()
        .find(|e| e.kind == BillEventKind::PaymentRecorded)
        .expect("the refused entry is still a line");
    assert_eq!(refused.refused_code.as_deref(), Some("self_payment"));
    assert!(!refused.applied());
}
