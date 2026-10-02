//! §14.3's guard: a send is written down before the wallet is called, and
//! blocks the next one from the same bill until it is resolved.

mod support;

use std::cell::Cell;
use std::collections::BTreeMap;

use splitz_core::host::{
    base64url_no_pad, create_bill, join_bill, payment_id_for_send, BillLog, CREATOR_KEY_BYTES,
};
use splitz_core::ExchangeRate;
use splitz_host::{
    BillStorage, HostError, InMemoryBillStorage, PendingSend, PendingSends, SendEnded, SwapWatch,
    Unrecordable, WalletBillHost,
};
use support::FakeWallet;

const TXID: &str = "aa00000000000000000000000000000000000000000000000000000000000001";

fn fake_key(who: &str) -> String {
    let first = who.as_bytes()[0];
    base64url_no_pad(
        &(0..CREATOR_KEY_BYTES)
            .map(|i| first.wrapping_add(i as u8))
            .collect::<Vec<u8>>(),
    )
}

fn send(bill_id: &str) -> PendingSend {
    PendingSend {
        bill_id: bill_id.to_owned(),
        uri: "zcash:u1ben?amount=0.1".to_owned(),
        carried: BTreeMap::from([("ben".to_owned(), 1000)]),
        at: "2026-10-28T19:30:00.000Z".to_owned(),
        sent: BTreeMap::from([("ben".to_owned(), 10_000_000)]),
        rate: Some(ExchangeRate {
            currency: "USD".to_owned(),
            minor_units_per_zec: 10_000,
            at: "2026-10-28T19:00:00.000Z".to_owned(),
            source: Some("a named feed".to_owned()),
        }),
        swap: None,
        zatoshi: None,
        txid: None,
    }
}

/// Storage whose reads and writes can be made to fail.
#[derive(Default)]
struct Flaky {
    inner: InMemoryBillStorage,
    unreadable: Cell<bool>,
    refuse_writes: Cell<bool>,
}

impl BillStorage for Flaky {
    fn read(&self, key: &str) -> splitz_host::Result<Option<String>> {
        if self.unreadable.get() {
            return Err(HostError::Unreadable(key.to_owned()));
        }
        self.inner.read(key)
    }
    fn write(&self, key: &str, value: &str) -> splitz_host::Result<()> {
        if self.refuse_writes.get() {
            return Err(HostError::Storage("disk full".to_owned()));
        }
        self.inner.write(key, value)
    }
    fn delete(&self, key: &str) -> splitz_host::Result<()> {
        self.inner.delete(key)
    }
    fn keys(&self, prefix: &str) -> splitz_host::Result<Vec<String>> {
        self.inner.keys(prefix)
    }
    fn sweep_unfinished_writes(&self) -> splitz_host::Result<usize> {
        Ok(0)
    }
}

fn in_flight(e: HostError) -> Option<Option<Box<PendingSend>>> {
    match e {
        HostError::SendInFlight { pending, .. } => Some(pending),
        _ => None,
    }
}

#[test]
fn the_next_send_from_the_bill_is_refused() {
    let storage = InMemoryBillStorage::default();
    let sends = PendingSends::new(&storage);
    sends.begin(&send("b1")).unwrap();
    sends
        .end("b1", SendEnded::Unresolved, None, false, None)
        .unwrap();
    let held = sends.of("b1").unwrap().expect("written down");
    assert_eq!(held.uri, "zcash:u1ben?amount=0.1");
    assert_eq!(
        held.rate.as_ref().map(|r| r.minor_units_per_zec),
        Some(10_000)
    );
    let refused = in_flight(sends.begin(&send("b1")).unwrap_err()).expect("in flight");
    assert_eq!(refused.map(|p| p.bill_id), Some("b1".to_owned()));
}

#[test]
fn another_bill_is_not_blocked() {
    let storage = InMemoryBillStorage::default();
    let sends = PendingSends::new(&storage);
    sends.begin(&send("b1")).unwrap();
    sends.begin(&send("b2")).unwrap();
    assert!(sends.of("b2").unwrap().is_some());
}

#[test]
fn a_second_send_before_the_first_ends_is_refused() {
    // Between `begin` and `end` the note is written; a second `begin` from
    // this process is refused whether or not it reads the note.
    let storage = InMemoryBillStorage::default();
    let sends = PendingSends::new(&storage);
    sends.begin(&send("b1")).unwrap();
    storage.delete("pendingsend/b1").unwrap();
    let refused = in_flight(sends.begin(&send("b1")).unwrap_err()).expect("in flight");
    assert!(refused.is_none());
}

#[test]
fn a_note_that_failed_to_write_lets_the_next_send_try_again() {
    let storage = Flaky::default();
    let sends = PendingSends::new(&storage);
    storage.refuse_writes.set(true);
    assert!(matches!(
        sends.begin(&send("b1")),
        Err(HostError::Storage(_))
    ));
    storage.refuse_writes.set(false);
    sends.begin(&send("b1")).unwrap();
    assert!(sends.of("b1").unwrap().is_some());
}

fn started(storage: &InMemoryBillStorage) -> PendingSends<'_> {
    let sends = PendingSends::new(storage);
    sends.begin(&send("b1")).unwrap();
    sends
}

#[test]
fn refused_the_note_goes_and_the_debt_can_be_sent_again() {
    let storage = InMemoryBillStorage::default();
    let sends = started(&storage);
    sends
        .end("b1", SendEnded::Refused, None, false, None)
        .unwrap();
    assert!(sends.of("b1").unwrap().is_none());
    sends.begin(&send("b1")).unwrap();
}

#[test]
fn reached_the_network_and_recorded_the_note_goes() {
    let storage = InMemoryBillStorage::default();
    let sends = started(&storage);
    sends
        .end("b1", SendEnded::ReachedNetwork, Some(TXID), true, None)
        .unwrap();
    assert!(sends.of("b1").unwrap().is_none());
}

#[test]
fn reached_the_network_and_not_recorded_it_stays_with_the_transaction() {
    let storage = InMemoryBillStorage::default();
    let sends = started(&storage);
    sends
        .end("b1", SendEnded::ReachedNetwork, Some(TXID), false, None)
        .unwrap();
    assert_eq!(sends.of("b1").unwrap().unwrap().txid.as_deref(), Some(TXID));
    assert!(in_flight(sends.begin(&send("b1")).unwrap_err()).is_some());
}

#[test]
fn unresolved_it_stays_with_the_transaction_the_wallet_built() {
    let storage = InMemoryBillStorage::default();
    let sends = started(&storage);
    sends
        .end("b1", SendEnded::Unresolved, Some(TXID), false, None)
        .unwrap();
    assert_eq!(sends.of("b1").unwrap().unwrap().txid.as_deref(), Some(TXID));
}

#[test]
fn unresolved_with_no_transaction_named_it_stays_as_written() {
    let storage = InMemoryBillStorage::default();
    let sends = started(&storage);
    sends
        .end("b1", SendEnded::Unresolved, None, false, None)
        .unwrap();
    let held = sends.of("b1").unwrap().expect("kept");
    assert!(held.txid.is_none());
}

#[test]
fn resolving_removes_it() {
    let storage = InMemoryBillStorage::default();
    let sends = started(&storage);
    sends
        .end("b1", SendEnded::Unresolved, None, false, None)
        .unwrap();
    sends.resolve("b1", None).unwrap();
    assert!(sends.of("b1").unwrap().is_none());
}

#[test]
fn a_note_that_is_not_json_still_blocks() {
    let storage = InMemoryBillStorage::default();
    storage.write("pendingsend/b1", "{not json").unwrap();
    let sends = PendingSends::new(&storage);
    assert!(sends.of("b1").unwrap().unwrap().is_damaged());
    assert!(in_flight(sends.begin(&send("b1")).unwrap_err()).is_some());
}

#[test]
fn a_note_this_type_did_not_write_still_blocks() {
    let storage = InMemoryBillStorage::default();
    storage
        .write("pendingsend/b1", r#"{"billId":"b1","uri":""}"#)
        .unwrap();
    assert!(PendingSends::new(&storage)
        .of("b1")
        .unwrap()
        .unwrap()
        .is_damaged());
}

#[test]
fn a_note_naming_another_bill_still_blocks() {
    let storage = InMemoryBillStorage::default();
    storage
        .write("pendingsend/b1", &send("b2").to_json().to_string())
        .unwrap();
    assert!(PendingSends::new(&storage)
        .of("b1")
        .unwrap()
        .unwrap()
        .is_damaged());
}

#[test]
fn a_note_the_storage_cannot_read_still_blocks() {
    let storage = Flaky::default();
    let sends = PendingSends::new(&storage);
    sends.begin(&send("b1")).unwrap();
    sends
        .end("b1", SendEnded::Unresolved, None, false, None)
        .unwrap();
    storage.unreadable.set(true);
    assert!(sends.of("b1").unwrap().unwrap().is_damaged());
}

fn bill(ana: &FakeWallet, ben: &FakeWallet) -> Vec<serde_json::Value> {
    let host = WalletBillHost::new(ana);
    let create = create_bill(&host, "D", "USD", "equal", &fake_key("ana"), None).unwrap();
    let join_ana = join_bill(&host, Some("Ana"), Some("u1ana"), None, None).unwrap();
    let join_ben = join_bill(
        &WalletBillHost::new(ben),
        Some("Ben"),
        Some("u1ben"),
        None,
        None,
    )
    .unwrap();
    ana.tick();
    vec![create, join_ana, join_ben]
}

#[test]
fn records_each_recipient_under_the_transaction() {
    let ana = FakeWallet::new("ana", Some("u1ana"));
    let ben = FakeWallet::new("ben", None);
    let host = WalletBillHost::new(&ana);
    let mut log = BillLog::with_entries(&host, bill(&ana, &ben));
    let storage = InMemoryBillStorage::default();
    let records = PendingSends::new(&storage)
        .records_for(
            &host,
            &mut log,
            &send("b1"),
            &format!("  {} ", TXID.to_uppercase()),
        )
        .unwrap();
    assert_eq!(records.len(), 1);
    let payments = log.fold().unwrap().bill.payments;
    assert_eq!(payments.len(), 1);
    assert_eq!(payments[0].id, payment_id_for_send("ana", TXID, "ben"));
    assert_eq!(payments[0].amount, 1000);
    assert_eq!(payments[0].to, "ben");
}

#[test]
fn does_not_record_a_recipient_twice() {
    let ana = FakeWallet::new("ana", Some("u1ana"));
    let ben = FakeWallet::new("ben", None);
    let host = WalletBillHost::new(&ana);
    let mut log = BillLog::with_entries(&host, bill(&ana, &ben));
    let storage = InMemoryBillStorage::default();
    let sends = PendingSends::new(&storage);
    sends
        .records_for(&host, &mut log, &send("b1"), TXID)
        .unwrap();
    let again = sends
        .records_for(&host, &mut log, &send("b1"), TXID)
        .unwrap();
    assert!(again.is_empty());
    assert_eq!(log.fold().unwrap().bill.payments.len(), 1);
}

#[test]
fn refuses_what_cannot_be_recorded() {
    let ana = FakeWallet::new("ana", Some("u1ana"));
    let ben = FakeWallet::new("ben", None);
    let host = WalletBillHost::new(&ana);
    let mut log = BillLog::with_entries(&host, bill(&ana, &ben));
    let storage = InMemoryBillStorage::default();
    let sends = PendingSends::new(&storage);
    assert_eq!(
        sends.records_for(&host, &mut log, &send("b1"), &TXID[1..]),
        Err(Unrecordable::NotATransactionId)
    );
    assert_eq!(
        sends.records_for(&host, &mut log, &PendingSend::damaged("b1"), TXID),
        Err(Unrecordable::DetailsLost)
    );
    let mut swap = send("b1");
    swap.swap = Some(SwapWatch {
        bill_id: "b1".to_owned(),
        reference: "ref-1".to_owned(),
        to: "ben".to_owned(),
        deposit_address: "t1deposit".to_owned(),
        deposit_memo: None,
        asset_symbol: "USDC".to_owned(),
        asset_chain: "base".to_owned(),
    });
    assert_eq!(
        sends.records_for(&host, &mut log, &swap, TXID),
        Err(Unrecordable::IsASwap)
    );
}

#[test]
fn a_note_reads_back_as_it_was_written() {
    let written = send("b1").sent_as(Some(TXID));
    let back = PendingSend::from_json(&written.to_json()).expect("reads back");
    assert_eq!(back, written);
}

// §14.3: saying a send left nothing in the wallet.

fn own(txid: &str, created: &str) -> splitz_host::OwnTransaction {
    splitz_host::OwnTransaction {
        txid: txid.to_owned(),
        created: created.to_owned(),
    }
}

#[test]
fn a_transaction_built_since_the_note_holds_it_and_is_named() {
    let before = own("aa", "2026-10-28T19:29:59.000Z");
    let after = own("cc", "2026-10-28T19:31:12.000Z");
    assert_eq!(
        splitz_host::unsent_claim_refusal(&send("b1"), false, &[before, after]),
        Some(splitz_host::UnsentClaimRefusal::BuiltSince {
            txid: "cc".to_owned()
        })
    );
}

#[test]
fn one_built_in_the_notes_own_second_holds_it_too() {
    // The wallet stamps whole seconds; the note was written first.
    let same = own("bb", "2026-10-28T19:30:00.000Z");
    assert!(matches!(
        splitz_host::unsent_claim_refusal(&send("b1"), false, &[same]),
        Some(splitz_host::UnsentClaimRefusal::BuiltSince { .. })
    ));
}

#[test]
fn one_built_before_the_note_does_not_hold_it() {
    let before = own("aa", "2026-10-28T19:29:59.000Z");
    assert_eq!(
        splitz_host::unsent_claim_refusal(&send("b1"), false, &[before]),
        None
    );
    assert_eq!(
        splitz_host::unsent_claim_refusal(&send("b1"), false, &[]),
        None
    );
}

#[test]
fn anything_still_sending_holds_it() {
    assert_eq!(
        splitz_host::unsent_claim_refusal(&send("b1"), true, &[]),
        Some(splitz_host::UnsentClaimRefusal::StillSending)
    );
}

#[test]
fn a_note_that_will_not_read_is_held_only_by_what_is_still_sending() {
    let damaged = PendingSend::damaged("b1");
    let after = own("cc", "2026-10-28T19:31:12.000Z");
    assert_eq!(
        splitz_host::unsent_claim_refusal(&damaged, false, &[after]),
        None
    );
    assert_eq!(
        splitz_host::unsent_claim_refusal(&damaged, true, &[]),
        Some(splitz_host::UnsentClaimRefusal::StillSending)
    );
}

// §14.4: withdrawing one's own record of a shielded payment.

fn payment(from: &str, method: &str, reference: Option<&str>) -> splitz_core::PaymentRecord {
    splitz_core::PaymentRecord {
        id: "p1".to_owned(),
        from: from.to_owned(),
        to: "ben".to_owned(),
        amount: 1000,
        currency: "USD".to_owned(),
        method: method.to_owned(),
        at: "2026-10-28T19:30:00.000Z".to_owned(),
        zatoshi: None,
        paid_at_rate: None,
        reference: reference.map(str::to_owned),
        note: None,
    }
}

#[test]
fn withdrawing_ones_own_shielded_record_is_refused_while_mined_or_waiting() {
    use splitz_host::{
        own_payment_withdrawal_refusal as refusal, OwnPaymentWithdrawal, TransactionState,
    };
    let p = payment("me", "shieldedZec", Some("tx1"));
    assert_eq!(
        refusal(&p, "me", Some(TransactionState::Mined)),
        Some(OwnPaymentWithdrawal::Mined)
    );
    assert_eq!(
        refusal(&p, "me", Some(TransactionState::Waiting)),
        Some(OwnPaymentWithdrawal::Waiting)
    );
}

#[test]
fn withdrawing_is_free_once_expired_or_when_the_history_does_not_hold_it() {
    use splitz_host::{own_payment_withdrawal_refusal as refusal, TransactionState};
    let p = payment("me", "shieldedZec", Some("tx1"));
    assert_eq!(refusal(&p, "me", Some(TransactionState::Expired)), None);
    assert_eq!(refusal(&p, "me", None), None);
}

#[test]
fn cash_swaps_another_payers_record_and_no_transaction_are_not_this_rules() {
    use splitz_host::{own_payment_withdrawal_refusal as refusal, TransactionState};
    for p in [
        payment("me", "cash", Some("tx1")),
        payment("me", "swap", Some("tx1")),
        payment("ben", "shieldedZec", Some("tx1")),
        payment("me", "shieldedZec", None),
    ] {
        assert_eq!(refusal(&p, "me", Some(TransactionState::Mined)), None);
    }
}
