//! The pure API, called from Rust with the facts a wallet would pass.

use splitz_ffi::{create_bill_entry, HostFacts};

#[test]
fn a_wallet_opens_a_bill_from_facts_alone() {
    let bytes: Vec<u8> = (0..32u8).collect();
    let seed = splitz_host::base64url_encode(&bytes);
    let creator_key = splitz_host::Signer
        .public_key_from_seed(&bytes)
        .expect("a seed is 32 bytes");
    let facts = HostFacts {
        me: "ana".to_owned(),
        pay_to: Some("u1ana".to_owned()),
        now: "2026-10-28T19:30:00.000Z".to_owned(),
        nonce: (0..16u8).collect(),
    };
    let entry = create_bill_entry(
        facts,
        "Dinner".to_owned(),
        "EUR".to_owned(),
        "equal".to_owned(),
        creator_key,
        seed,
    )
    .expect("the entry is written");
    let parsed: serde_json::Value = serde_json::from_str(&entry).unwrap();
    assert_eq!(parsed["kind"], "createBill");
    assert!(parsed["id"].as_str().is_some_and(|id| !id.is_empty()));
    assert!(parsed["sig"].as_str().is_some());
}

#[test]
fn a_seed_of_the_wrong_length_is_an_error_not_a_panic() {
    // Sixteen bytes decode as base64url and are not an Ed25519 seed. Every
    // entry builder signs through a callback that cannot return an error, so
    // the length has to be refused where the seed enters.
    let short = splitz_host::base64url_encode(&[7u8; 16]);
    let facts = HostFacts {
        me: "ana".to_owned(),
        pay_to: Some("u1ana".to_owned()),
        now: "2026-10-28T19:30:00.000Z".to_owned(),
        nonce: (0..16u8).collect(),
    };
    let key = splitz_host::base64url_encode(&[1u8; 32]);
    let result = std::panic::catch_unwind(|| {
        create_bill_entry(
            facts,
            "Dinner".to_owned(),
            "EUR".to_owned(),
            "equal".to_owned(),
            key,
            short,
        )
    });
    match result {
        Ok(Err(splitz_ffi::SplitzError::Host { detail, transient })) => {
            assert!(detail.contains("32 bytes"), "{detail}");
            assert!(!transient);
        }
        other => panic!("expected a Host error, got {other:?}"),
    }
}
