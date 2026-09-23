//! A bill's key and this account's signing identity (SPEC.md §9.4, §10.7).

use splitz_host::{
    is_well_formed_key, InMemorySecretStore, Randomness, SecretStore, Signer, SplitsKeys,
    WalletAccount, KEY_LENGTH_BYTES,
};
use std::cell::Cell;

/// Predictable bytes, so a test asserts a value rather than a shape. Never a
/// substitute for the platform's entropy: §9.4's nonce has to be unguessable.
struct Counter(Cell<u8>);

impl Counter {
    fn from(start: u8) -> Self {
        Counter(Cell::new(start))
    }
}

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

#[test]
fn a_bill_key_is_32_bytes_and_is_kept_not_rederived() {
    let store = InMemorySecretStore::default();
    let random = Counter::from(0);
    let keys = SplitsKeys::new(&store, &random);
    let first = keys.ensure_bill_key("b1").unwrap();
    assert!(is_well_formed_key(&first));
    assert_eq!(keys.ensure_bill_key("b1").unwrap(), first);
    assert_eq!(keys.read_bill_key("b1").unwrap(), Some(first));
}

#[test]
fn two_bills_do_not_share_a_key() {
    let store = InMemorySecretStore::default();
    let random = Counter::from(0);
    let keys = SplitsKeys::new(&store, &random);
    assert_ne!(
        keys.ensure_bill_key("b1").unwrap(),
        keys.ensure_bill_key("b2").unwrap()
    );
}

#[test]
fn a_key_is_not_derived_from_the_bill_id() {
    // The id is what every invite and every scanned code hands out. A key
    // derived from it would be reproducible by anyone who ever saw one.
    let (store_a, random_a) = (InMemorySecretStore::default(), Counter::from(0));
    let (store_b, random_b) = (InMemorySecretStore::default(), Counter::from(9));
    assert_ne!(
        SplitsKeys::new(&store_a, &random_a)
            .ensure_bill_key("b1")
            .unwrap(),
        SplitsKeys::new(&store_b, &random_b)
            .ensure_bill_key("b1")
            .unwrap()
    );
}

#[test]
fn a_key_of_the_wrong_length_is_refused_at_the_scan_not_at_the_cipher() {
    // §11.1 checks only that an invite's `k` is non-empty base64url. A key
    // that is the right alphabet and the wrong length therefore reaches this
    // untouched, and storing it would move the failure into whatever loop next
    // tries to decrypt.
    let store = InMemorySecretStore::default();
    let random = Counter::from(0);
    let keys = SplitsKeys::new(&store, &random);
    for bad in ["AAAA", "", &"A".repeat(42), &"A".repeat(44)] {
        assert!(
            keys.store_bill_key("b1", bad).is_err(),
            "key of length {}",
            bad.len()
        );
    }
    assert_eq!(keys.read_bill_key("b1").unwrap(), None);
}

#[test]
fn a_key_in_standard_base64s_alphabet_is_not_a_key() {
    // Standard base64 uses `+` and `/` where base64url uses `-` and `_`. A
    // decoder that accepts both turns a key in the wrong alphabet into 32
    // bytes, and the text stored then matches nothing §11.1 will hand over.
    let store = InMemorySecretStore::default();
    let random = Counter::from(0);
    let keys = SplitsKeys::new(&store, &random);
    let wrong_alphabet = format!("+/{}", "A".repeat(41));
    assert_eq!(wrong_alphabet.len(), 43);
    assert!(!is_well_formed_key(&wrong_alphabet));
    assert!(keys.store_bill_key("b1", &wrong_alphabet).is_err());
    // The same 32 bytes in the alphabet §9.4 writes are a key.
    assert!(is_well_formed_key(&format!("-_{}", "A".repeat(41))));
}

#[test]
fn a_well_formed_key_from_an_invite_is_stored_as_it_arrived() {
    let store = InMemorySecretStore::default();
    let random = Counter::from(0);
    let keys = SplitsKeys::new(&store, &random);
    let key = keys.generate_key();
    keys.store_bill_key("b1", &key).unwrap();
    assert_eq!(keys.read_bill_key("b1").unwrap(), Some(key));
}

#[test]
fn forgetting_a_bill_forgets_its_key() {
    let store = InMemorySecretStore::default();
    let random = Counter::from(0);
    let keys = SplitsKeys::new(&store, &random);
    keys.ensure_bill_key("b1").unwrap();
    keys.forget_bill("b1").unwrap();
    assert_eq!(keys.read_bill_key("b1").unwrap(), None);
}

#[test]
fn an_identity_derived_from_a_secret_survives_a_reinstall() {
    let store = InMemorySecretStore::default();
    let random = Counter::from(0);
    let keys = SplitsKeys::new(&store, &random);
    let account = WalletAccount {
        id: "acct-1".to_owned(),
        identity_secret: Some(vec![1, 2, 3]),
    };
    let first = keys.ensure_identity_seed(&account).unwrap();

    // A new device: a fresh keychain, and a wallet database that assigns a
    // different account identifier from the same mnemonic.
    let (fresh_store, fresh_random) = (InMemorySecretStore::default(), Counter::from(200));
    let after_restore = WalletAccount {
        id: "acct-9".to_owned(),
        identity_secret: Some(vec![1, 2, 3]),
    };
    assert_eq!(
        SplitsKeys::new(&fresh_store, &fresh_random)
            .ensure_identity_seed(&after_restore)
            .unwrap(),
        first
    );
    assert!(account.identity_is_recoverable());
}

#[test]
fn two_accounts_do_not_share_an_identity() {
    let store = InMemorySecretStore::default();
    let random = Counter::from(0);
    let keys = SplitsKeys::new(&store, &random);
    let a = WalletAccount {
        id: "acct-1".to_owned(),
        identity_secret: Some(vec![1, 2, 3]),
    };
    let b = WalletAccount {
        id: "acct-2".to_owned(),
        identity_secret: Some(vec![4, 5, 6]),
    };
    assert_ne!(
        keys.ensure_identity_seed(&a).unwrap(),
        keys.ensure_identity_seed(&b).unwrap()
    );
}

#[test]
fn an_account_with_no_secret_still_signs_and_says_it_cannot_recover() {
    let store = InMemorySecretStore::default();
    let random = Counter::from(0);
    let keys = SplitsKeys::new(&store, &random);
    let account = WalletAccount {
        id: "acct-1".to_owned(),
        identity_secret: None,
    };
    let seed = keys.ensure_identity_seed(&account).unwrap();
    assert_eq!(seed.len(), KEY_LENGTH_BYTES);
    assert_eq!(
        keys.ensure_identity_seed(&account).unwrap(),
        seed,
        "it is stored, so it is stable on this device"
    );
    assert!(!account.identity_is_recoverable());

    // And it is a real signing key: §10.6 is satisfied by it.
    let key = Signer.public_key_from_seed(&seed).unwrap();
    assert_eq!(splitz_host::base64url_decode(&key).unwrap().len(), 32);
}

#[test]
fn a_stored_identity_is_returned_unchanged_whatever_the_account_now_says() {
    let store = InMemorySecretStore::default();
    let random = Counter::from(0);
    let keys = SplitsKeys::new(&store, &random);
    let before = WalletAccount {
        id: "acct-1".to_owned(),
        identity_secret: None,
    };
    let seed = keys.ensure_identity_seed(&before).unwrap();
    // A secret arriving later must not silently rotate an identity other
    // participants have already pinned.
    let after = WalletAccount {
        id: "acct-1".to_owned(),
        identity_secret: Some(vec![1, 2, 3]),
    };
    assert_eq!(keys.ensure_identity_seed(&after).unwrap(), seed);
}

#[test]
fn the_seed_is_sha256_of_the_domain_a_zero_byte_and_the_secret() {
    // Pinned against a digest computed outside this crate:
    //   printf 'splitz.identity.v2\x00\x01\x02\x03' | shasum -a 256
    let seed = splitz_host::identity_seed_from(&[1, 2, 3]);
    let hex: String = seed.iter().map(|b| format!("{b:02x}")).collect();
    assert_eq!(
        hex,
        "30da771c99ad554a64185a76297a22e32b18a03a8a8bd49fe2ea50c39aaf3a9b"
    );
}

#[test]
fn an_identity_stored_under_the_viewing_key_derivation_is_not_read() {
    let store = InMemorySecretStore::default();
    store
        .write(
            "splitz_identity_seed_acct-1",
            &splitz_host::base64url_encode(&[7u8; 32]),
        )
        .unwrap();
    let random = Counter::from(0);
    let keys = SplitsKeys::new(&store, &random);
    let account = WalletAccount {
        id: "acct-1".to_owned(),
        identity_secret: Some(vec![1, 2, 3]),
    };
    assert_eq!(
        keys.ensure_identity_seed(&account).unwrap(),
        splitz_host::identity_seed_from(&[1, 2, 3])
    );
}
