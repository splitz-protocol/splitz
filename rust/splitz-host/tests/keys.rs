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

const MNEMONIC: &str = concat!(
    "abandon abandon abandon abandon abandon abandon ",
    "abandon abandon abandon abandon abandon about"
);

fn seed_of(passphrase: &str, account: u32) -> String {
    let secret = splitz_host::identity_secret_from_mnemonic(MNEMONIC, passphrase, account).unwrap();
    splitz_host::base64url_encode(&splitz_host::identity_seed_from(&secret))
}

#[test]
fn a_mnemonic_derives_the_same_identity_in_every_wallet() {
    // Pinned against seeds computed outside this crate, in Python, from
    // §15.1's layout: sha256(domain, 0, mnemonic, 0, passphrase[, 0, index]).
    assert_eq!(
        seed_of("", 0),
        "Bsu7QAZyG9usbFpPUrQUo5ni3MtX7JeU-rUHoWKUPKY"
    );
    assert_eq!(
        seed_of("TREZOR", 0),
        "NoXrjBFhw-SnV_XZyW7Pe9JyZ5s3G17lAWIl76cxbVU"
    );
    assert_eq!(
        seed_of("", 1),
        "jWQijb3QECEvHgOUb4_W7UPiUujN7D-7Nq0jYg5mrKo"
    );
    assert_eq!(
        seed_of("", splitz_host::MAX_ACCOUNT_INDEX),
        "W0O2YQssPQfLgQT0BSukqsP17xenAYEbLGWwi6Ln7q4"
    );
    assert_eq!(
        seed_of("pässwörd", 5),
        "mQ-gPUEwIqxWeL6LWOJNH3azGQVVfUOET30BZPpzRa0"
    );
}

#[test]
fn account_zero_carries_no_index_and_account_one_does() {
    let zero = splitz_host::identity_secret_from_mnemonic("m", "p", 0).unwrap();
    let one = splitz_host::identity_secret_from_mnemonic("m", "p", 1).unwrap();
    assert_eq!(zero, b"m\0p".to_vec());
    assert_eq!(one, b"m\0p\0\0\0\0\x01".to_vec());
}

#[test]
fn every_spelling_bip39_reads_as_one_wallet_is_one_identity() {
    // "abandon abandon about", 0x00, "caf" + U+00E9: NFKC, single spaces.
    let want = "6162616e646f6e206162616e646f6e2061626f757400636166c3a9";
    let hex = |b: Vec<u8>| b.iter().map(|x| format!("{x:02x}")).collect::<String>();
    for (mnemonic, passphrase) in [
        ("abandon abandon about", "caf\u{e9}"),
        ("abandon abandon about", "cafe\u{301}"),
        ("abandon  abandon\u{3000}about", "caf\u{e9}"),
        (" abandon abandon about\n", "cafe\u{301}"),
    ] {
        let secret = splitz_host::identity_secret_from_mnemonic(mnemonic, passphrase, 0).unwrap();
        assert_eq!(hex(secret), want, "{mnemonic:?} {passphrase:?}");
    }
    // U+FEFF is not white space: it stays, and is another wallet.
    let bom =
        splitz_host::identity_secret_from_mnemonic("abandon\u{feff}abandon about", "", 0).unwrap();
    assert!(!hex(bom).starts_with("6162616e646f6e20"));
    assert!(splitz_host::identity_secret_from_mnemonic(" \u{3000} ", "p", 0).is_err());
}

#[test]
fn an_empty_mnemonic_and_an_index_past_zip32_are_refused() {
    assert!(splitz_host::identity_secret_from_mnemonic("", "p", 0).is_err());
    assert!(splitz_host::identity_secret_from_mnemonic("m", "p", 0x8000_0000).is_err());
    assert!(splitz_host::identity_secret_from_mnemonic("m", "p", u32::MAX).is_err());
    // A zero byte is the separator, so one inside either text would let two
    // inputs join to one secret: account 1 of (m, p) and account 0 of
    // (m, "p\0\0\0\0\x01") are the same bytes.
    assert!(splitz_host::identity_secret_from_mnemonic("m", "p\0\0\0\0\x01", 0).is_err());
    assert!(splitz_host::identity_secret_from_mnemonic("a\0b", "", 0).is_err());
    assert!(splitz_host::identity_secret_from_mnemonic("a", "b\0", 0).is_err());
    // An empty passphrase is the common case and stays accepted.
    assert!(splitz_host::identity_secret_from_mnemonic("m", "", 0).is_ok());
}

#[test]
fn two_accounts_on_one_store_keep_their_own_bill_keys_only_when_scoped() {
    use splitz_host::AccountSecretStore;
    let key = "Ag0fRP6k8Q3m4m0yS5Yq0R1Ff7lM0nq8yLw4X0nNw5c";
    let random = Counter::from(0);

    let shared = InMemorySecretStore::default();
    let (a, b) = (
        SplitsKeys::new(&shared, &random),
        SplitsKeys::new(&shared, &random),
    );
    a.store_bill_key("bill", key).unwrap();
    b.store_bill_key("bill", key).unwrap();
    a.forget_bill("bill").unwrap();
    assert_eq!(
        b.read_bill_key("bill").unwrap(),
        None,
        "unscoped, one deletes both"
    );

    let shared = InMemorySecretStore::default();
    let (sa, sb) = (
        AccountSecretStore::new(&shared, "acct-a").unwrap(),
        AccountSecretStore::new(&shared, "acct-b").unwrap(),
    );
    let (a, b) = (SplitsKeys::new(&sa, &random), SplitsKeys::new(&sb, &random));
    a.store_bill_key("bill", key).unwrap();
    b.store_bill_key("bill", key).unwrap();
    a.forget_bill("bill").unwrap();
    assert_eq!(a.read_bill_key("bill").unwrap(), None);
    assert_eq!(b.read_bill_key("bill").unwrap().as_deref(), Some(key));
    assert!(AccountSecretStore::new(&shared, "").is_err());
}

#[test]
fn a_key_is_forgotten_only_while_it_is_still_the_one_held() {
    let store = InMemorySecretStore::default();
    let random = Counter::from(0);
    let keys = SplitsKeys::new(&store, &random);
    let foreign = keys.generate_key();
    let real = keys.generate_key();
    keys.store_bill_key("b1", &foreign).unwrap();
    // The person took the real invite since the foreign key was read.
    keys.forget_bill("b1").unwrap();
    keys.store_bill_key("b1", &real).unwrap();
    assert!(!keys.forget_bill_if_still("b1", &foreign).unwrap());
    assert_eq!(
        keys.read_bill_key("b1").unwrap().as_deref(),
        Some(real.as_str())
    );
    // Still the one held: forgotten.
    assert!(keys.forget_bill_if_still("b1", &real).unwrap());
    assert_eq!(keys.read_bill_key("b1").unwrap(), None);
    // Nothing held: nothing to forget.
    assert!(!keys.forget_bill_if_still("b1", &real).unwrap());
}
