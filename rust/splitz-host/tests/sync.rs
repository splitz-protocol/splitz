//! Moving a bill between devices through a relay that holds only ciphertext.

mod support;

use std::cell::Cell;

use serde_json::{json, Value};
use splitz_core::host::{add_expense, create_bill, join_bill};
use splitz_host::{
    channel_for_bill, BillStore, HostError, InMemoryBillStorage, InMemorySecretStore,
    InMemorySplitsRelay, Randomness, Sealing, Signer, SplitsKeys, SplitsRelay, SplitsSync,
    WalletBillHost,
};
use support::{seed_for, FakeWallet};

struct Counter(Cell<u8>);

impl Randomness for Counter {
    fn bytes(&self, count: usize) -> Vec<u8> {
        (0..count)
            .map(|_| {
                let next = self.0.get().wrapping_add(1);
                self.0.set(next);
                next
            })
            .collect()
    }
}

/// One device: its own storage and keychain, sharing a relay with the others.
struct Device {
    wallet: FakeWallet,
    storage: InMemoryBillStorage,
    secrets: InMemorySecretStore,
    random: Counter,
}

impl Device {
    fn new(id: &str, pay_to: &str, seed: u8) -> Self {
        Self {
            wallet: FakeWallet::new(id, Some(pay_to)),
            storage: InMemoryBillStorage::default(),
            secrets: InMemorySecretStore::default(),
            random: Counter(Cell::new(seed)),
        }
    }

    fn store(&self) -> BillStore<'_> {
        BillStore::new(&self.storage)
    }

    fn keys(&self) -> SplitsKeys<'_> {
        SplitsKeys::new(&self.secrets, &self.random)
    }

    fn host(&self) -> WalletBillHost<'_> {
        WalletBillHost::new(&self.wallet)
    }
}

fn ids(entries: &[Value]) -> Vec<String> {
    entries
        .iter()
        .map(|e| e["id"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn two_devices_that_sync_the_same_bill_hold_the_same_entries() {
    let relay = InMemorySplitsRelay::default();
    let ana = Device::new("ana", "u1ana", 1);
    let ben = Device::new("ben", "u1ben", 90);

    let (ana_store, ana_keys) = (ana.store(), ana.keys());
    let key = ana_keys.ensure_bill_key("placeholder").unwrap();
    let create = create_bill(&ana.host(), "Dinner", "EUR", "equal", &"A".repeat(43)).unwrap();
    let bill_id = create["id"].as_str().unwrap().to_owned();
    ana_keys.store_bill_key(&bill_id, &key).unwrap();
    ana_store.merge(&bill_id, vec![create.clone()]).unwrap();

    // Ben joins with the key the invite carried, and writes an expense.
    let (ben_store, ben_keys) = (ben.store(), ben.keys());
    ben_keys.store_bill_key(&bill_id, &key).unwrap();
    ben.wallet.tick();
    let join = join_bill(&ben.host(), Some("Ben"), Some("u1ben"), None, None).unwrap();
    ben.wallet.tick();
    let expense = add_expense(
        &ben.host(),
        "x1",
        "ben",
        4500,
        json!({"mode": "equal"}),
        Some("Wine"),
    )
    .unwrap();
    ben_store.merge(&bill_id, vec![join, expense]).unwrap();

    let ana_sync = SplitsSync::new(&ana_store, &ana_keys, &relay);
    let ben_sync = SplitsSync::new(&ben_store, &ben_keys, &relay);
    ana_sync.sync(&bill_id, None, None).unwrap();
    ben_sync.sync(&bill_id, None, None).unwrap();
    let back = ana_sync.sync(&bill_id, None, None).unwrap();

    assert_eq!(back.unopenable, 0);
    assert_eq!(ids(&ana_store.read(&bill_id).unwrap()).len(), 3);
    assert_eq!(
        ids(&ana_store.read(&bill_id).unwrap()),
        ids(&ben_store.read(&bill_id).unwrap())
    );
}

#[test]
fn syncing_twice_changes_nothing() {
    let relay = InMemorySplitsRelay::default();
    let ana = Device::new("ana", "u1ana", 1);
    let (store, keys) = (ana.store(), ana.keys());
    let key = keys.ensure_bill_key("placeholder").unwrap();
    let create = create_bill(&ana.host(), "Dinner", "EUR", "equal", &"A".repeat(43)).unwrap();
    let bill_id = create["id"].as_str().unwrap().to_owned();
    keys.store_bill_key(&bill_id, &key).unwrap();
    store.merge(&bill_id, vec![create]).unwrap();

    let sync = SplitsSync::new(&store, &keys, &relay);
    let once = sync.sync(&bill_id, None, None).unwrap();
    let twice = sync.sync(&bill_id, None, None).unwrap();
    assert_eq!(ids(&once.entries), ids(&twice.entries));
    assert_eq!(relay.fetch(&channel_for_bill(&bill_id)).unwrap().len(), 1);
}

#[test]
fn a_blob_from_another_bill_is_skipped_not_fatal() {
    // One bad blob must not strand a bill. It is counted, because a channel
    // where every blob is unopenable is a wrong key and looks exactly like a
    // quiet relay unless somebody counts.
    let relay = InMemorySplitsRelay::default();
    let ana = Device::new("ana", "u1ana", 1);
    let (store, keys) = (ana.store(), ana.keys());
    let key = keys.ensure_bill_key("placeholder").unwrap();
    let create = create_bill(&ana.host(), "Dinner", "EUR", "equal", &"A".repeat(43)).unwrap();
    let bill_id = create["id"].as_str().unwrap().to_owned();
    keys.store_bill_key(&bill_id, &key).unwrap();
    store.merge(&bill_id, vec![create]).unwrap();

    let stranger = keys.ensure_bill_key("another-bill").unwrap();
    assert_ne!(stranger, key);
    let foreign = Sealing
        .seal(&json!({"v": 1, "id": "e9"}), &stranger)
        .unwrap();
    relay.push(&channel_for_bill(&bill_id), &[foreign]).unwrap();

    let result = SplitsSync::new(&store, &keys, &relay)
        .sync(&bill_id, None, None)
        .unwrap();
    assert_eq!(result.unopenable, 1);
    assert_eq!(result.entries.len(), 1);
}

#[test]
fn the_channel_is_the_bill_ids_hash_never_the_id() {
    let channel = channel_for_bill("a-bill-id");
    assert_ne!(channel, "a-bill-id");
    assert_eq!(channel.len(), 64);
    assert_eq!(channel, channel_for_bill("a-bill-id"));
}

#[test]
fn a_bill_with_no_key_cannot_be_synced_and_says_so() {
    let relay = InMemorySplitsRelay::default();
    let ana = Device::new("ana", "u1ana", 1);
    let (store, keys) = (ana.store(), ana.keys());
    match SplitsSync::new(&store, &keys, &relay).pull("never-seen") {
        Err(HostError::Sync(why)) => assert!(why.contains("No key")),
        other => panic!("expected a sync refusal, got {other:?}"),
    }
}

#[test]
fn pushing_signs_this_devices_own_unsigned_entries() {
    let relay = InMemorySplitsRelay::default();
    let ana = Device::new("ana", "u1ana", 1);
    let (store, keys) = (ana.store(), ana.keys());
    let key = keys.ensure_bill_key("placeholder").unwrap();
    let seed = seed_for("ana");
    let public_key = Signer.public_key_from_seed(&seed).unwrap();

    let create = create_bill(&ana.host(), "Dinner", "EUR", "equal", &public_key).unwrap();
    let bill_id = create["id"].as_str().unwrap().to_owned();
    keys.store_bill_key(&bill_id, &key).unwrap();
    ana.wallet.tick();
    let join = join_bill(
        &ana.host(),
        Some("Ana"),
        Some("u1ana"),
        Some(&public_key),
        None,
    )
    .unwrap();
    store.merge(&bill_id, vec![create, join]).unwrap();

    SplitsSync::new(&store, &keys, &relay)
        .push(&bill_id, Some(&seed), Some("ana"))
        .unwrap();

    let blobs = relay.fetch(&channel_for_bill(&bill_id)).unwrap();
    assert_eq!(blobs.len(), 2);
    for blob in &blobs {
        let entry = Sealing.open(blob, &key).unwrap();
        assert!(entry.get("sig").is_some(), "unsigned: {entry}");
        assert!(Signer.verify_entry(&entry, &public_key, &bill_id));
    }
    // The stored log is untouched: signing happens on the way out.
    for entry in store.read(&bill_id).unwrap() {
        assert!(entry.get("sig").is_none());
    }
}

#[test]
fn a_peers_entry_is_merged_without_judging_who_wrote_it() {
    // An entry admitted or refused by what this device happened to hold when
    // it arrived would make the stored log depend on network order, and two
    // devices that pulled in a different order would hold different bills.
    // §10.7 decides authorship over the whole log at fold time.
    let relay = InMemorySplitsRelay::default();
    let ana = Device::new("ana", "u1ana", 1);
    let (store, keys) = (ana.store(), ana.keys());
    let key = keys.ensure_bill_key("placeholder").unwrap();
    let create = create_bill(&ana.host(), "Dinner", "EUR", "equal", &"A".repeat(43)).unwrap();
    let bill_id = create["id"].as_str().unwrap().to_owned();
    keys.store_bill_key(&bill_id, &key).unwrap();
    store.merge(&bill_id, vec![create]).unwrap();

    // A stranger who holds the key writes a join nobody vouched for.
    let stranger = Device::new("zzz", "u1zzz", 200);
    stranger.wallet.tick();
    let theirs = join_bill(&stranger.host(), Some("Zed"), Some("u1zzz"), None, None).unwrap();
    relay
        .push(
            &channel_for_bill(&bill_id),
            &[Sealing.seal(&theirs, &key).unwrap()],
        )
        .unwrap();

    let result = SplitsSync::new(&store, &keys, &relay)
        .pull(&bill_id)
        .unwrap();
    assert_eq!(result.unopenable, 0);
    assert!(ids(&result.entries).contains(&theirs["id"].as_str().unwrap().to_owned()));
}

/// A relay that forgets the bill while it fetches, the way a person removing
/// a bill during a poll does.
struct ForgetsWhileFetching<'a> {
    inner: InMemorySplitsRelay,
    keys: &'a SplitsKeys<'a>,
    store: &'a BillStore<'a>,
    bill_id: String,
}

impl SplitsRelay for ForgetsWhileFetching<'_> {
    fn push(&self, channel: &str, blobs: &[String]) -> splitz_host::Result<()> {
        self.inner.push(channel, blobs)
    }

    fn fetch(&self, channel: &str) -> splitz_host::Result<Vec<String>> {
        let blobs = self.inner.fetch(channel)?;
        self.keys.forget_bill(&self.bill_id)?;
        self.store.forget(&self.bill_id)?;
        Ok(blobs)
    }
}

#[test]
fn a_bill_forgotten_while_a_sync_fetches_is_not_written_back() {
    let ana = Device::new("ana", "u1ana", 1);
    let (store, keys) = (ana.store(), ana.keys());
    let host = ana.host();
    let create = create_bill(&host, "Dinner", "EUR", "equal", &"A".repeat(43)).unwrap();
    let bill_id = create["id"].as_str().unwrap().to_owned();
    keys.ensure_bill_key(&bill_id).unwrap();
    store.merge(&bill_id, vec![create]).unwrap();

    let relay = ForgetsWhileFetching {
        inner: InMemorySplitsRelay::default(),
        keys: &keys,
        store: &store,
        bill_id: bill_id.clone(),
    };
    let sync = SplitsSync::new(&store, &keys, &relay);
    sync.push(&bill_id, None, None).unwrap();
    match sync.pull(&bill_id) {
        Err(HostError::Sync(why)) => assert!(why.contains("forgotten"), "{why}"),
        other => panic!("expected a sync refusal, got {other:?}"),
    }
    assert!(
        store.bill_ids().unwrap().is_empty(),
        "the bill is not written back without its key"
    );
}

#[test]
fn a_different_key_for_a_bill_already_held_is_refused() {
    let ana = Device::new("ana", "u1ana", 1);
    let keys = ana.keys();
    let held = keys.ensure_bill_key("b1").unwrap();
    let other = keys.generate_key();
    match keys.store_bill_key("b1", &other) {
        Err(HostError::KeyConflict(bill)) => assert_eq!(bill, "b1"),
        other => panic!("expected a key conflict, got {other:?}"),
    }
    assert_eq!(keys.read_bill_key("b1").unwrap(), Some(held.clone()));
    keys.store_bill_key("b1", &held).unwrap();
}

#[test]
fn a_stored_entry_that_is_not_an_entry_is_skipped_not_raised() {
    let ana = Device::new("ana", "u1ana", 1);
    splitz_host::BillStorage::write(
        &ana.storage,
        "splitz_bill_b1",
        r#"[{"kind":"joinBill"},{"kind":"x"},7]"#,
    )
    .unwrap();
    assert!(ana.store().read("b1").unwrap().is_empty());
}
