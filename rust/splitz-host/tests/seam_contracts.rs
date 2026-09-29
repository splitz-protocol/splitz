//! The §15 seam checks, against the implementations this crate ships and
//! against implementations that each break one rule.

use std::cell::RefCell;
use std::collections::BTreeMap;

use splitz_host::{
    check_bill_storage, check_secret_store, check_splits_relay, check_zec_prices, BillStorage,
    FixedZecPrices, HostError, InMemoryBillStorage, InMemorySecretStore, InMemorySplitsRelay,
    NoZecPrices, Result, SeamFinding, SecretStore, SplitsRelay, ZecPrices,
};

fn rules(found: Vec<SeamFinding>) -> Vec<&'static str> {
    found.into_iter().map(|f| f.rule).collect()
}

#[test]
fn the_implementations_this_crate_ships_keep_section_15() {
    assert_eq!(check_secret_store(&InMemorySecretStore::default(), "a"), []);
    assert_eq!(check_bill_storage(&InMemoryBillStorage::default(), "a"), []);
    assert_eq!(check_splits_relay(&InMemorySplitsRelay::default(), "a"), []);
    let fixed = FixedZecPrices::new([("USD".to_owned(), 138_819)]);
    assert_eq!(check_zec_prices(&fixed, "USD"), []);
    assert_eq!(check_zec_prices(&NoZecPrices, "USD"), []);
}

/// A secret store that refuses a key it never held.
#[derive(Default)]
struct RefusesWhenAbsent(InMemorySecretStore);

impl SecretStore for RefusesWhenAbsent {
    fn read(&self, key: &str) -> Result<Option<String>> {
        self.0
            .read(key)?
            .map(Some)
            .ok_or_else(|| HostError::Storage("no such key".to_owned()))
    }
    fn write(&self, key: &str, value: &str) -> Result<()> {
        self.0.write(key, value)
    }
    fn delete(&self, key: &str) -> Result<()> {
        self.0.delete(key)
    }
}

/// A store whose `keys` ignores the prefix it is given.
#[derive(Default)]
struct ListsEverything(InMemoryBillStorage);

impl BillStorage for ListsEverything {
    fn read(&self, key: &str) -> Result<Option<String>> {
        self.0.read(key)
    }
    fn write(&self, key: &str, value: &str) -> Result<()> {
        self.0.write(key, value)
    }
    fn delete(&self, key: &str) -> Result<()> {
        self.0.delete(key)
    }
    fn keys(&self, _prefix: &str) -> Result<Vec<String>> {
        self.0.keys("")
    }
    fn sweep_unfinished_writes(&self) -> Result<usize> {
        Ok(0)
    }
}

/// A relay that keeps every push, a repeat included.
#[derive(Default)]
struct Duplicates(RefCell<BTreeMap<String, Vec<String>>>);

impl SplitsRelay for Duplicates {
    fn push(&self, channel: &str, blobs: &[String]) -> Result<()> {
        self.0
            .borrow_mut()
            .entry(channel.to_owned())
            .or_default()
            .extend(blobs.iter().cloned());
        Ok(())
    }
    fn fetch(&self, channel: &str) -> Result<Vec<String>> {
        Ok(self.0.borrow().get(channel).cloned().unwrap_or_default())
    }
}

/// A price source that invents a figure for anything.
struct Invents;

impl ZecPrices for Invents {
    fn minor_units_per_zec(&self, _currency: &str) -> Result<Option<i64>> {
        Ok(Some(0))
    }
}

#[test]
fn each_rule_names_the_implementation_that_breaks_it() {
    assert!(
        rules(check_secret_store(&RefusesWhenAbsent::default(), "a"))
            .contains(&"a key never written reads as empty")
    );
    assert_eq!(
        rules(check_bill_storage(&ListsEverything::default(), "a")),
        ["keys answers every key under the prefix, and only those"]
    );
    assert_eq!(
        rules(check_splits_relay(&Duplicates::default(), "a")),
        ["pushing a blob again changes nothing"]
    );
    assert_eq!(
        rules(check_zec_prices(&Invents, "USD")),
        [
            "a code nobody prices answers empty, not an error",
            "an answer is a positive whole number of minor units an IEEE-754 double holds exactly",
        ]
    );
}
