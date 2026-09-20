//! Sealing an entry for a transport (SPEC.md §11.3).

mod support;

use serde_json::{json, Map, Value};
use splitz_core::host::join_bill;
use splitz_host::{
    base64url_decode, base64url_encode, InMemorySecretStore, Randomness, Sealing, SplitsKeys,
    WalletBillHost, BLOB_VERSION,
};
use std::cell::Cell;
use support::FakeWallet;

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

fn a_key(n: u8) -> String {
    let store = InMemorySecretStore::default();
    let random = Counter(Cell::new(n));
    SplitsKeys::new(&store, &random).generate_key()
}

fn an_entry(name: &str) -> Value {
    let wallet = FakeWallet::ana();
    let host = WalletBillHost::new(&wallet);
    join_bill(&host, Some(name), Some("u1ana"), None, None).unwrap()
}

#[test]
fn a_sealed_entry_opens_back_into_the_same_entry() {
    let key = a_key(7);
    let entry = an_entry("Ana");
    let blob = Sealing.seal(&entry, &key).unwrap();
    assert_eq!(Sealing.open(&blob, &key).unwrap(), entry);
}

#[test]
fn the_same_entry_always_seals_to_the_same_blob() {
    // What keeps a channel finite: a relay keyed by blob content stores an
    // entry once however often it is pushed.
    let key = a_key(7);
    let entry = an_entry("Ana");
    assert_eq!(
        Sealing.seal(&entry, &key).unwrap(),
        Sealing.seal(&entry, &key).unwrap()
    );
}

#[test]
fn two_different_entries_never_seal_to_the_same_blob() {
    let key = a_key(7);
    assert_ne!(
        Sealing.seal(&an_entry("Ana"), &key).unwrap(),
        Sealing.seal(&an_entry("Ben"), &key).unwrap()
    );
}

#[test]
fn key_order_in_the_callers_map_does_not_change_the_blob() {
    // The nonce comes from the sealed bytes, and those are canonical. A device
    // building the same entry with its members in another order must reach the
    // same blob, or a relay holds two copies that every device opens perfectly
    // and none recognises as one entry.
    let key = a_key(7);
    let entry = an_entry("Ana");
    let mut reordered = Map::new();
    let members: Vec<&String> = entry.as_object().unwrap().keys().collect();
    for name in members.iter().rev() {
        reordered.insert((*name).clone(), entry[*name].clone());
    }
    let reordered = Value::Object(reordered);
    assert_ne!(
        entry.as_object().unwrap().keys().collect::<Vec<_>>(),
        reordered.as_object().unwrap().keys().collect::<Vec<_>>()
    );
    assert_eq!(
        Sealing.seal(&entry, &key).unwrap(),
        Sealing.seal(&reordered, &key).unwrap()
    );
}

#[test]
fn a_blob_sealed_under_another_key_does_not_open() {
    let mine = a_key(7);
    let theirs = a_key(90);
    assert_ne!(mine, theirs);
    let blob = Sealing.seal(&an_entry("Ana"), &mine).unwrap();
    assert!(Sealing.open(&blob, &theirs).is_err());
}

#[test]
fn an_altered_blob_does_not_open() {
    let key = a_key(7);
    let blob = Sealing.seal(&an_entry("Ana"), &key).unwrap();
    // Flip one bit of the ciphertext, well past the version byte.
    let mut bytes = base64url_decode(&blob).unwrap();
    let last = bytes.len() - 3;
    bytes[last] ^= 0x01;
    assert!(Sealing.open(&base64url_encode(&bytes), &key).is_err());
}

#[test]
fn a_blob_from_a_later_format_is_refused_not_misread() {
    let key = a_key(7);
    let mut bytes = base64url_decode(&Sealing.seal(&an_entry("Ana"), &key).unwrap()).unwrap();
    bytes[0] = BLOB_VERSION + 1;
    assert!(Sealing.open(&base64url_encode(&bytes), &key).is_err());
}

#[test]
fn a_truncated_or_empty_blob_is_refused() {
    let key = a_key(7);
    for blob in ["".to_owned(), base64url_encode(&[BLOB_VERSION])] {
        assert!(Sealing.open(&blob, &key).is_err(), "blob {blob:?}");
    }
}

#[test]
fn a_key_of_the_wrong_length_is_refused_before_the_cipher_sees_it() {
    assert!(Sealing.seal(&an_entry("Ana"), "AAAA").is_err());
}

#[test]
fn anything_a_key_holder_seals_comes_back_entry_shaped_or_not() {
    // Sealing authenticates; it does not judge. An object that is not a valid
    // entry opens perfectly and is refused one layer up, by the protocol's own
    // ingress check, which is the thing that knows what an entry is.
    let key = a_key(7);
    let not_an_entry = json!({ "v": 1 });
    let blob = Sealing.seal(&not_an_entry, &key).unwrap();
    assert_eq!(Sealing.open(&blob, &key).unwrap(), not_an_entry);
}
