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
