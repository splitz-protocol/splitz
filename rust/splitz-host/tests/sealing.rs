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

/// A frame of `version` sealing `clear` as it is, padded or not.
fn seal_raw(version: u8, clear: &[u8], key: &str) -> String {
    use chacha20poly1305::aead::Aead;
    use chacha20poly1305::{KeyInit, XChaCha20Poly1305, XNonce};
    let cipher = XChaCha20Poly1305::new(base64url_decode(key).unwrap().as_slice().into());
    let nonce = splitz_core::sealed_nonce(clear);
    let body = cipher.encrypt(XNonce::from_slice(&nonce), clear).unwrap();
    let mut raw = vec![version];
    raw.extend_from_slice(&nonce);
    raw.extend_from_slice(&body);
    base64url_encode(&raw)
}

fn sealed_bytes(blob: &str) -> usize {
    base64url_decode(blob).unwrap().len() - 41
}

#[test]
fn a_blob_is_as_long_as_its_bucket_not_its_entry() {
    let key = a_key(3);
    assert_eq!(
        sealed_bytes(&Sealing.seal(&an_entry("Ana"), &key).unwrap()),
        1024
    );
    assert_eq!(
        sealed_bytes(&Sealing.seal(&an_entry(&"A".repeat(600)), &key).unwrap()),
        1024
    );
    assert_eq!(
        sealed_bytes(&Sealing.seal(&an_entry(&"A".repeat(1200)), &key).unwrap()),
        2048
    );
    assert_eq!(
        base64url_decode(&Sealing.seal(&an_entry("Ana"), &key).unwrap()).unwrap()[0],
        2
    );
}

#[test]
fn a_version_1_blob_sealed_unpadded_still_opens() {
    let key = a_key(3);
    let entry = an_entry("Ana");
    let v1 = seal_raw(1, &splitz_core::sealed_plaintext(&entry).unwrap(), &key);
    assert_eq!(Sealing.open(&v1, &key).unwrap(), entry);
}

#[test]
fn a_version_2_blob_padded_any_other_way_is_refused() {
    let key = a_key(3);
    let plain = splitz_core::sealed_plaintext(&an_entry("Ana")).unwrap();
    let mut short = plain.clone();
    short.push(0x80);
    short.resize(1022, 0);
    let mut no_marker = plain.clone();
    no_marker.resize(1024, 0);
    // One plaintext has one padding: not a bucket past the smallest.
    let mut past = plain.clone();
    past.push(0x80);
    past.resize(2048, 0);
    for clear in [plain.clone(), short, no_marker, past] {
        assert!(
            Sealing.open(&seal_raw(2, &clear, &key), &key).is_err(),
            "{}",
            clear.len()
        );
    }
    let good = splitz_core::pad_sealed(&plain).unwrap();
    assert_eq!(
        Sealing.open(&seal_raw(2, &good, &key), &key).unwrap(),
        an_entry("Ana")
    );
}

#[test]
fn a_version_of_zero_is_refused() {
    let key = a_key(3);
    let v0 = seal_raw(
        0,
        &splitz_core::sealed_plaintext(&an_entry("Ana")).unwrap(),
        &key,
    );
    assert!(Sealing.open(&v0, &key).is_err());
}

#[test]
fn an_entry_no_bucket_holds_is_refused_one_byte_under_is_not() {
    let key = a_key(3);
    // `{"n":"…"}` is 8 bytes around the string; the marker is one more.
    let sized = |bytes: usize| json!({"n": "x".repeat(bytes - 8)});
    assert_eq!(
        splitz_core::sealed_size_refusal(&sized(32767)).unwrap(),
        None
    );
    assert_eq!(
        sealed_bytes(&Sealing.seal(&sized(32767), &key).unwrap()),
        32768
    );
    assert_eq!(
        splitz_core::sealed_size_refusal(&sized(32768)).unwrap(),
        Some("sealed_too_large")
    );
    assert!(Sealing.seal(&sized(32768), &key).is_err());
}
