//! Ed25519 over §10.6's message, and what §10.7 makes of the answers.

mod support;

use serde_json::{json, Value};
use splitz_core::host::{create_bill, join_bill, sign_entry};
use splitz_core::{check_entry, code};
use splitz_host::{
    base64url_decode, fold_unverified, fold_verified, FoldFailure, Signer, WalletBillHost,
};
use support::{seed_for, FakeWallet};

/// Signs `entry` with `seed` through the host seam, the way a wallet does.
fn signed(wallet: &FakeWallet, seed: &[u8], build: impl Fn(&WalletBillHost) -> Value) -> Value {
    let sign = |message: &[u8]| Signer.sign(seed, message).expect("a seed is 32 bytes");
    let host = WalletBillHost::new(wallet).signing_with(&sign);
    let entry = build(&host);
    sign_entry(&host, &entry).expect("an entry this host wrote is signable")
}

#[test]
fn a_public_key_is_32_bytes_which_is_what_9_4_asks_a_key_to_be() {
    let key = Signer
        .public_key_from_seed(&seed_for("ana"))
        .expect("a seed is 32 bytes");
    assert_eq!(base64url_decode(&key).unwrap().len(), 32);
    assert_eq!(
        key.len(),
        43,
        "32 bytes is 43 unpadded base64url characters"
    );

    // Long enough to be a creatorKey, which §9.4 refuses at any other length.
    let wallet = FakeWallet::ana();
    let host = WalletBillHost::new(&wallet);
    let create = create_bill(&host, "Dinner", "EUR", "equal", &key).unwrap();
    check_entry(&create).unwrap();
}

#[test]
fn signing_is_deterministic_so_a_re_pushed_entry_is_one_blob() {
    let wallet = FakeWallet::ana();
    let seed = seed_for("ana");
    let sign = |message: &[u8]| Signer.sign(&seed, message).unwrap();
    let host = WalletBillHost::new(&wallet).signing_with(&sign);

    let entry = join_bill(&host, Some("Ana"), None, None, None).unwrap();
    let once = sign_entry(&host, &entry).unwrap();
    let twice = sign_entry(&host, &entry).unwrap();
    assert_eq!(once["sig"], twice["sig"]);
    assert_eq!(
        once["id"], entry["id"],
        "§9.5 excludes sig from the digest, so signing cannot move it"
    );
}

#[test]
fn a_real_signature_binds_a_key_to_a_participant_under_10_7() {
    let wallet = FakeWallet::ana();
    let ana_seed = seed_for("ana");
    let ben_seed = seed_for("ben");
    let ana_key = Signer.public_key_from_seed(&ana_seed).unwrap();
    let ben_key = Signer.public_key_from_seed(&ben_seed).unwrap();

    let ben_wallet = FakeWallet::new("ben", Some("u1ben"));

    let mut entries = vec![signed(&wallet, &ana_seed, |host| {
        create_bill(host, "Dinner", "EUR", "equal", &ana_key).unwrap()
    })];
    wallet.tick();
    entries.push(signed(&wallet, &ana_seed, |host| {
        join_bill(host, Some("Ana"), Some("u1ana"), Some(&ana_key), None).unwrap()
    }));
    ben_wallet.tick();
    ben_wallet.tick();
    entries.push(signed(&ben_wallet, &ben_seed, |host| {
        join_bill(host, Some("Ben"), Some("u1ben"), Some(&ben_key), None).unwrap()
    }));

    let folded = fold_verified(&wallet, bill_id(&entries), &entries, None).expect("the log folds");
    assert!(folded.set_aside.is_empty());
    assert_eq!(folded.identities.bound.get("ana"), Some(&ana_key));
    assert_eq!(folded.identities.bound.get("ben"), Some(&ben_key));
    assert!(folded.identities.contested.is_empty());
}

#[test]
fn a_create_signed_by_the_wrong_key_opens_no_bill_at_all() {
    let wallet = FakeWallet::ana();
    let ana_key = Signer.public_key_from_seed(&seed_for("ana")).unwrap();
    // Signs with somebody else's seed while claiming ana's key.
    let impostor = seed_for("zzz");

    let mut entries = vec![signed(&wallet, &impostor, |host| {
        create_bill(host, "Dinner", "EUR", "equal", &ana_key).unwrap()
    })];
    wallet.tick();
    entries.push(signed(&wallet, &impostor, |host| {
        join_bill(host, Some("Ana"), Some("u1ana"), Some(&ana_key), None).unwrap()
    }));

    // Stronger than "unbound". §10.3 sets aside a create whose signature does
    // not verify against the key it itself states, and a log with no surviving
    // create opens nothing — so writing down somebody else's key does not get
    // a bill off the ground, it stops one existing.
    match fold_verified(&wallet, bill_id(&entries), &entries, None) {
        Err(FoldFailure::Refused(e)) => assert_eq!(e.code, code::LOG_NO_CREATE),
        other => panic!("expected log_no_create, got {other:?}"),
    }

    // And the same log folds fine with no verifier: §10.7 then binds nothing
    // rather than refusing, which is the honest answer for a device that
    // cannot check a signature.
    let unchecked =
        fold_unverified(&wallet, bill_id(&entries), &entries).expect("it folds unverified");
    assert_eq!(unchecked.bill.participants.len(), 1);
    assert_eq!(unchecked.bill.participants[0].id, "ana");
    assert!(unchecked.identities.bound.is_empty());
}

#[test]
fn two_keys_claiming_one_id_leaves_that_id_contested() {
    let wallet = FakeWallet::ana();
    let ana_seed = seed_for("ana");
    let ben_seed = seed_for("ben");
    let impostor_seed = seed_for("zzz");
    let ana_key = Signer.public_key_from_seed(&ana_seed).unwrap();
    let ben_key = Signer.public_key_from_seed(&ben_seed).unwrap();
    let impostor_key = Signer.public_key_from_seed(&impostor_seed).unwrap();

    let ben_wallet = FakeWallet::new("ben", Some("u1ben"));
    let impostor_wallet = FakeWallet::new("ben", Some("u1impostor"));

    let mut entries = vec![signed(&wallet, &ana_seed, |host| {
        create_bill(host, "Dinner", "EUR", "equal", &ana_key).unwrap()
    })];
    wallet.tick();
    entries.push(signed(&wallet, &ana_seed, |host| {
        join_bill(host, Some("Ana"), Some("u1ana"), Some(&ana_key), None).unwrap()
    }));
    ben_wallet.tick();
    ben_wallet.tick();
    entries.push(signed(&ben_wallet, &ben_seed, |host| {
        join_bill(host, Some("Ben"), Some("u1ben"), Some(&ben_key), None).unwrap()
    }));
    for _ in 0..4 {
        impostor_wallet.tick();
    }
    entries.push(signed(&impostor_wallet, &impostor_seed, |host| {
        join_bill(
            host,
            Some("Ben"),
            Some("u1impostor"),
            Some(&impostor_key),
            None,
        )
        .unwrap()
    }));

    let folded = fold_verified(&wallet, bill_id(&entries), &entries, None).expect("the log folds");
    assert!(folded.identities.contested.contains("ben"));
    assert!(
        !folded.identities.bound.contains_key("ben"),
        "nothing inside the log says which claim is the person"
    );
}

#[test]
fn an_unsigned_entry_verifies_against_nothing() {
    let unsigned = json!({"id": "e1", "author": "ana"});
    assert!(!Signer.verify_entry(&unsigned, &"A".repeat(43)));
}

#[test]
fn a_malformed_key_or_signature_is_false_not_a_crash() {
    let wallet = FakeWallet::ana();
    let seed = seed_for("ana");
    let entry = signed(&wallet, &seed, |host| {
        join_bill(host, Some("Ana"), None, None, None).unwrap()
    });

    assert!(!Signer.verify_entry(&entry, "not base64url!!"));
    assert!(
        !Signer.verify_entry(&entry, "AAAA"),
        "four characters is three bytes, not a 32-byte key"
    );

    let mut tampered = entry.clone();
    tampered["sig"] = Value::from("!!!!");
    assert!(!Signer.verify_entry(&tampered, &"A".repeat(43)));
}

#[test]
fn every_question_the_fold_asks_was_answered_in_advance() {
    let wallet = FakeWallet::ana();
    let seed = seed_for("ana");
    let key = Signer.public_key_from_seed(&seed).unwrap();

    let mut entries = vec![signed(&wallet, &seed, |host| {
        create_bill(host, "Dinner", "EUR", "equal", &key).unwrap()
    })];
    wallet.tick();
    entries.push(signed(&wallet, &seed, |host| {
        join_bill(host, Some("Ana"), Some("u1ana"), Some(&key), None).unwrap()
    }));

    let verified = Signer.prepare(entries.iter());
    let verify = |entry: &Value, k: &str| verified.verify(entry, k);
    let host = WalletBillHost::new(&wallet).verifying_with(&verify);
    splitz_core::host::BillLog::with_entries(&host, entries.clone())
        .fold()
        .expect("the log folds");
    assert!(
        verified.unanswered().is_empty(),
        "a pair nobody answered would read as an invalid signature"
    );
}

#[test]
fn a_fold_that_asks_an_unanticipated_question_fails_loudly() {
    // The guard's own witness: prepare nothing, then fold a log that does ask.
    let wallet = FakeWallet::ana();
    let seed = seed_for("ana");
    let key = Signer.public_key_from_seed(&seed).unwrap();
    let create = signed(&wallet, &seed, |host| {
        create_bill(host, "Dinner", "EUR", "equal", &key).unwrap()
    });

    let empty = Signer.prepare(std::iter::empty());
    let verify = |entry: &Value, k: &str| empty.verify(entry, k);
    let host = WalletBillHost::new(&wallet).verifying_with(&verify);
    // The fold asks, gets `false` for want of an answer, and drops the create
    // — so it refuses the whole log. The question still went on the record.
    assert!(
        splitz_core::host::BillLog::with_entries(&host, vec![create])
            .fold()
            .is_err()
    );
    assert!(!empty.unanswered().is_empty());
}

/// The bill a test log opens: the id of its create entry.
fn bill_id(entries: &[serde_json::Value]) -> &str {
    entries
        .iter()
        .find(|e| e["kind"] == "createBill")
        .and_then(|e| e["id"].as_str())
        .expect("the log holds a create")
}
