//! Swaps a device sent and has not seen finish.

use serde_json::json;
use splitz_host::{BillStorage, InMemoryBillStorage, SwapWatch, SwapWatchList};

fn a_watch(bill: &str, reference: &str) -> SwapWatch {
    SwapWatch {
        bill_id: bill.to_owned(),
        reference: reference.to_owned(),
        to: "ben".to_owned(),
        deposit_address: "u1provider".to_owned(),
        deposit_memo: Some("memo-1".to_owned()),
        asset_symbol: "USDC".to_owned(),
        asset_chain: "base".to_owned(),
    }
}

#[test]
fn a_watch_round_trips_through_storage() {
    let storage = InMemoryBillStorage::default();
    let list = SwapWatchList::new(&storage);
    let watch = a_watch("b1", "near-intent-7f3a");
    list.add(&watch).unwrap();
    assert_eq!(list.held(None).unwrap(), vec![watch]);
}

#[test]
fn a_watch_is_kept_for_the_bill_it_settles() {
    let storage = InMemoryBillStorage::default();
    let list = SwapWatchList::new(&storage);
    list.add(&a_watch("b1", "r1")).unwrap();
    list.add(&a_watch("b2", "r2")).unwrap();
    assert_eq!(list.held(Some("b1")).unwrap().len(), 1);
    assert_eq!(list.held(Some("b1")).unwrap()[0].reference, "r1");
    assert_eq!(list.held(None).unwrap().len(), 2);
    assert!(list.held(Some("b9")).unwrap().is_empty());
}

#[test]
fn forgetting_one_leaves_the_others() {
    // A list that only grows is one nobody reads.
    let storage = InMemoryBillStorage::default();
    let list = SwapWatchList::new(&storage);
    list.add(&a_watch("b1", "r1")).unwrap();
    list.add(&a_watch("b1", "r2")).unwrap();
    list.forget("r1").unwrap();
    let held = list.held(None).unwrap();
    assert_eq!(held.len(), 1);
    assert_eq!(held[0].reference, "r2");
}

#[test]
fn a_reference_with_characters_a_key_cannot_hold_still_round_trips() {
    let storage = InMemoryBillStorage::default();
    let list = SwapWatchList::new(&storage);
    for reference in ["a/b", "a b", "café", "a%2Fb"] {
        list.add(&a_watch("b1", reference)).unwrap();
    }
    let mut held: Vec<String> = list
        .held(None)
        .unwrap()
        .into_iter()
        .map(|w| w.reference)
        .collect();
    held.sort();
    assert_eq!(held, vec!["a b", "a%2Fb", "a/b", "café"]);
    list.forget("a/b").unwrap();
    assert_eq!(list.held(None).unwrap().len(), 3);
}

#[test]
fn a_damaged_entry_is_skipped_rather_than_failing_the_list() {
    // It costs a follow-up, not a bill.
    let storage = InMemoryBillStorage::default();
    storage.write("swapwatch/broken", "not json").unwrap();
    storage
        .write("swapwatch/partial", &json!({"billId": "b1"}).to_string())
        .unwrap();
    let list = SwapWatchList::new(&storage);
    list.add(&a_watch("b1", "r1")).unwrap();
    assert_eq!(list.held(None).unwrap().len(), 1);
}

#[test]
fn a_watch_is_namespaced_away_from_the_bills() {
    // A sweep of one must never reach the other.
    let storage = InMemoryBillStorage::default();
    SwapWatchList::new(&storage)
        .add(&a_watch("b1", "r1"))
        .unwrap();
    assert!(storage.keys("splitz_bill_").unwrap().is_empty());
    assert_eq!(storage.keys("swapwatch/").unwrap().len(), 1);
}

#[test]
fn the_quote_a_status_query_needs_carries_only_what_it_reads() {
    // The bill already holds what was owed and what was sent; a second copy
    // here would be a second thing to keep right.
    let watch = a_watch("b1", "near-intent-7f3a");
    let quote = watch.as_quote();
    assert_eq!(quote.deposit_address, "u1provider");
    assert_eq!(quote.deposit_memo.as_deref(), Some("memo-1"));
    assert_eq!(quote.payment_reference(), "near-intent-7f3a");
    assert_eq!(quote.amount_in_zatoshi, 0);
    // A watch is not a quote: every reader sees its deadline as past.
    assert!(quote.has_expired("2026-01-01T00:00:00.000Z"));
}
