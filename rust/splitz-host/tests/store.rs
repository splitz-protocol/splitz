//! What a device holds for a bill, and what the merge does with it (§10.1,
//! §10.2).

mod support;

use serde_json::{json, Value};
use splitz_core::host::{add_expense, create_bill, join_bill};
use splitz_host::{BillStorage, BillStore, InMemoryBillStorage, WalletBillHost};
use support::FakeWallet;

/// A create, a join and an expense, written a minute apart.
fn a_log(wallet: &FakeWallet) -> Vec<Value> {
    let host = WalletBillHost::new(wallet);
    let mut entries = vec![create_bill(&host, "Dinner", "EUR", "equal", &"A".repeat(43)).unwrap()];
    wallet.tick();
    entries.push(join_bill(&host, Some("Ana"), Some("u1ana"), None, None).unwrap());
    wallet.tick();
    entries.push(
        add_expense(
            &host,
            "x1",
            "ana",
            4500,
            json!({"mode": "equal"}),
            Some("Wine"),
        )
        .unwrap(),
    );
    entries
}

#[test]
fn a_bill_not_held_reads_as_no_entries() {
    let storage = InMemoryBillStorage::default();
    let store = BillStore::new(&storage);
    assert!(store.read("b1").unwrap().is_empty());
    assert!(store.bill_ids().unwrap().is_empty());
}

#[test]
fn what_is_merged_is_what_is_read_back_and_the_id_is_listed() {
    let wallet = FakeWallet::ana();
    let storage = InMemoryBillStorage::default();
    let store = BillStore::new(&storage);
    let entries = a_log(&wallet);
    let merged = store.merge("b1", entries.clone()).unwrap();
    assert_eq!(merged.entries.len(), 3);
    assert!(merged.refused.is_empty());
    assert_eq!(store.read("b1").unwrap(), merged.entries);
    assert_eq!(store.bill_ids().unwrap(), vec!["b1".to_owned()]);
}

#[test]
fn merging_the_same_entries_twice_changes_nothing() {
    // §10.2's merge is a set union, so a device that re-receives what it holds
    // holds the same log. A store that appended would double every entry the
    // first time a peer replayed a channel.
    let wallet = FakeWallet::ana();
    let storage = InMemoryBillStorage::default();
    let store = BillStore::new(&storage);
    let entries = a_log(&wallet);
    let once = store.merge("b1", entries.clone()).unwrap().entries;
    let twice = store.merge("b1", entries).unwrap().entries;
    assert_eq!(once, twice);
}

#[test]
fn entries_come_back_in_the_order_10_2_puts_them() {
    let wallet = FakeWallet::ana();
    let storage = InMemoryBillStorage::default();
    let store = BillStore::new(&storage);
    let mut entries = a_log(&wallet);
    entries.reverse();
    let merged = store.merge("b1", entries).unwrap().entries;
    let read = store.read("b1").unwrap();
    assert_eq!(read, merged);
    // The order is the protocol's, not the order they arrived in.
    let ats: Vec<&str> = read.iter().map(|e| e["at"].as_str().unwrap()).collect();
    let mut sorted = ats.clone();
    sorted.sort_unstable();
    assert_eq!(ats, sorted);
}

#[test]
fn stored_text_that_will_not_decode_reads_as_no_entries() {
    // A bill this device cannot read is a state to show. Failing here would
    // take down whatever listed the bills.
    let storage = InMemoryBillStorage::default();
    for stored in ["", "not json", "{\"not\":\"a list\"}", "[1, 2, 3]"] {
        storage.write("splitz_bill_b1", stored).unwrap();
        assert!(
            BillStore::new(&storage).read("b1").unwrap().is_empty(),
            "stored {stored:?}"
        );
    }
}

#[test]
fn what_the_merge_refuses_comes_back_with_the_log() {
    // An entry that vanished silently is indistinguishable from one that was
    // never sent.
    let wallet = FakeWallet::ana();
    let storage = InMemoryBillStorage::default();
    let store = BillStore::new(&storage);
    let mut entries = a_log(&wallet);
    entries.push(json!({"v": 1, "id": "nope", "kind": "notAKind"}));
    let merged = store.merge("b1", entries).unwrap();
    assert_eq!(merged.entries.len(), 3);
    assert_eq!(merged.refused.len(), 1);
}

#[test]
fn forgetting_a_bill_leaves_nothing_behind() {
    let wallet = FakeWallet::ana();
    let storage = InMemoryBillStorage::default();
    let store = BillStore::new(&storage);
    store.merge("b1", a_log(&wallet)).unwrap();
    store.forget("b1").unwrap();
    assert!(store.read("b1").unwrap().is_empty());
    assert!(store.bill_ids().unwrap().is_empty());
}

#[test]
fn a_store_that_cannot_be_half_written_sweeps_nothing() {
    let storage = InMemoryBillStorage::default();
    assert_eq!(
        BillStore::new(&storage).sweep_unfinished_writes().unwrap(),
        0
    );
}

#[test]
fn a_send_reporting_success_with_no_transaction_id_is_pending() {
    // The wallet says money left; with no id nothing can be recorded, and
    // calling it failed would let a retry pay it again.
    use splitz_core::host::{BillHost, SendResult};
    let mut wallet = FakeWallet::ana();
    wallet.sender.outcome.txid = None;
    let host = WalletBillHost::new(&wallet);
    let sent = host.broadcast("zcash:u1ben?amount=0.045");
    assert_eq!(sent.result, SendResult::Pending);
    assert!(sent.detail.unwrap().contains("no transaction id"));
}

#[test]
fn a_stored_log_that_does_not_decode_is_never_written_over() {
    // Not absent: a merge that took it for an empty log would write the
    // relay's copy over it and lose every entry only this device held.
    let wallet = FakeWallet::ana();
    let storage = InMemoryBillStorage::default();
    let store = BillStore::new(&storage);
    storage.write("splitz_bill_b1", "{ damaged").unwrap();

    assert!(store.read("b1").unwrap().is_empty(), "shown as no entries");
    match store.merge("b1", a_log(&wallet)) {
        Err(splitz_host::HostError::Unreadable(name)) => assert_eq!(name, "splitz_bill_b1"),
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert_eq!(
        storage.read("splitz_bill_b1").unwrap().as_deref(),
        Some("{ damaged"),
        "not written over"
    );
}
