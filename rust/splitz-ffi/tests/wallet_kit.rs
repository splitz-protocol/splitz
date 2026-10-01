//! What a wallet on the binding reaches beyond a bill's arithmetic: the
//! payee's confirm-screen check, the fallback price sources, a new bill key,
//! invite links and expiry, and what a peer lacks.

use splitz_ffi::{
    bill_key_problem, binance_price_request, check_payee_review, coinbase_price_request,
    create_bill_entry, delta_for_peer, identity_key_from_seed, invite_expiry, invite_for_bill,
    join_bill_entry, new_bill_key, read_scanned, render_invite_link, zec_price_from_binance,
    zec_price_from_coinbase, ExchangeRate, HostFacts, PaymentRecord, RandomBytes, ReviewRule,
    SplitzError,
};

fn contracts() -> String {
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/contracts").to_owned()
}

const TXID: &str = "1a2b3c4d5e6f708192a3b4c5d6e7f8091a2b3c4d5e6f708192a3b4c5d6e7f809";

fn record(
    method: &str,
    zatoshi: Option<i64>,
    rate: bool,
    reference: Option<&str>,
) -> PaymentRecord {
    PaymentRecord {
        id: "p1".into(),
        from: "ana".into(),
        to: "ben".into(),
        amount: 4000,
        currency: "EUR".into(),
        method: method.into(),
        at: "2026-10-28T19:30:00.000Z".into(),
        zatoshi,
        paid_at_rate: rate.then(|| ExchangeRate {
            currency: "EUR".into(),
            minor_units_per_zec: 51234,
            at: "2026-10-28T19:30:00.000Z".into(),
            source: None,
        }),
        reference: reference.map(str::to_owned),
        note: None,
    }
}

#[test]
fn the_payee_check_names_each_figure_the_screen_leaves_out() {
    let screen: Vec<String> = ["0.07807316 ZEC", "at 512.34 EUR", "tx 1a2b3c4d5e6f…"]
        .map(str::to_owned)
        .to_vec();
    let zec = record("shieldedZec", Some(7_807_316), true, Some(TXID));
    assert!(
        check_payee_review(zec.clone(), screen.clone(), String::new())
            .unwrap()
            .is_empty()
    );
    let found = check_payee_review(zec, screen[..2].to_vec(), String::new()).unwrap();
    assert_eq!(
        found
            .iter()
            .map(|f| (f.rule, f.expected.as_str()))
            .collect::<Vec<_>>(),
        [(ReviewRule::PayeeReference, TXID)]
    );
    let bare = record("swap", None, false, Some("near-intent-7f3a"));
    let found =
        check_payee_review(bare, vec!["near-intent-7f3a".into()], "not recorded".into()).unwrap();
    assert_eq!(
        found.iter().map(|f| f.rule).collect::<Vec<_>>(),
        [ReviewRule::PayeeZec, ReviewRule::PayeeRate]
    );
    assert!(
        check_payee_review(record("cash", None, false, None), vec![], String::new())
            .unwrap()
            .is_empty()
    );
}

#[test]
fn the_fallback_price_sources_read_their_captured_answers() {
    let fixture =
        |name: &str| std::fs::read_to_string(format!("{}/fixtures/{name}", contracts())).unwrap();
    assert_eq!(
        binance_price_request("https://data-api.binance.vision/".into()),
        "https://data-api.binance.vision/api/v3/ticker/price?symbol=ZECUSDC"
    );
    assert_eq!(
        coinbase_price_request("https://api.coinbase.com".into()),
        "https://api.coinbase.com/v2/exchange-rates?currency=ZEC"
    );
    let usd = zec_price_from_binance(fixture("binance_ticker.json"), "USD".into()).unwrap();
    assert!(usd.unwrap() > 0);
    assert_eq!(
        zec_price_from_binance(fixture("binance_ticker.json"), "EUR".into()).unwrap(),
        None
    );
    let kes = zec_price_from_coinbase(fixture("coinbase_rates.json"), "KES".into()).unwrap();
    assert!(kes.unwrap() > 0);
    assert!(matches!(
        zec_price_from_binance("<html>blocked</html>".into(), "USD".into()),
        Err(SplitzError::Host { .. })
    ));
}

#[test]
fn a_new_bill_key_is_thirty_two_random_bytes() {
    let key = new_bill_key(RandomBytes {
        bytes: (0..32u8).collect(),
    })
    .unwrap();
    assert_eq!(bill_key_problem(key), None);
    assert!(matches!(
        new_bill_key(RandomBytes { bytes: vec![7; 31] }),
        Err(SplitzError::Host { .. })
    ));
}

struct Device {
    seed: String,
    key: String,
    me: String,
}

impl Device {
    fn new(byte: u8) -> Self {
        let seed = splitz_host::base64url_encode(&[byte; 32]);
        let key = identity_key_from_seed(seed.clone()).unwrap();
        let me = splitz_ffi::participant_id_for_key(key.clone()).unwrap();
        Self { seed, key, me }
    }

    fn facts(&self, minute: u32) -> HostFacts {
        HostFacts {
            me: self.me.clone(),
            now: format!("2026-10-28T19:{minute:02}:00.000Z"),
            nonce: vec![self.seed.as_bytes()[0]; 16],
        }
    }
}

/// Ana's bill, and Ben joined: two entries.
fn bill() -> (Device, String, Vec<String>) {
    let ana = Device::new(1);
    let ben = Device::new(90);
    let create = create_bill_entry(
        ana.facts(1),
        "Dinner".into(),
        "EUR".into(),
        "equal".into(),
        ana.key.clone(),
        None,
        ana.seed.clone(),
    )
    .unwrap();
    let bill_id = serde_json::from_str::<serde_json::Value>(&create).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let join = join_bill_entry(
        ben.facts(2),
        bill_id.clone(),
        Some("Ben".into()),
        Some("u1ben".into()),
        Some(ben.key.clone()),
        vec![],
        ben.seed.clone(),
    )
    .unwrap();
    (ana, bill_id, vec![create, join])
}

#[test]
fn an_invite_link_reads_back_and_its_expiry_is_compared_with_the_callers_clock() {
    let (ana, bill_id, entries) = bill();
    let key = new_bill_key(RandomBytes { bytes: vec![9; 32] }).unwrap();
    let invite = invite_for_bill(
        ana.facts(3),
        bill_id.clone(),
        entries,
        key.clone(),
        Some("Dinner".into()),
        Some(1_800_000_000),
    )
    .unwrap();
    let link = render_invite_link(invite.clone(), "https://example.org/join".into()).unwrap();
    assert!(link.starts_with("https://example.org/join#"), "{link}");
    let read = read_scanned(link);
    assert_eq!(read.bill_id.as_deref(), Some(bill_id.as_str()));
    assert_eq!(read.bill_key.as_deref(), Some(key.as_str()));

    let before = invite_expiry(invite.clone(), 1_799_999_999).unwrap();
    assert_eq!(
        (before.expiry, before.expired),
        (Some(1_800_000_000), false)
    );
    assert!(
        invite_expiry(invite.clone(), 1_800_000_001)
            .unwrap()
            .expired
    );
    match render_invite_link(invite, "http://example.org/join".into()) {
        Err(SplitzError::Protocol { code, .. }) => assert_eq!(code, "invite_bad_link"),
        other => panic!("expected invite_bad_link, got {other:?}"),
    }
}

#[test]
fn a_delta_carries_what_the_peer_lacks_and_nothing_when_it_lacks_nothing() {
    let (ana, bill_id, entries) = bill();
    // §14.5: a peer names each copy it holds, the id and the signature.
    let ids: Vec<String> = entries
        .iter()
        .map(|e| splitz_ffi::copy_key(e.clone()).unwrap())
        .collect();
    let none = delta_for_peer(ana.facts(4), bill_id.clone(), entries.clone(), ids.clone()).unwrap();
    assert_eq!((none.missing, none.uri, none.too_big_code), (0, None, None));
    let one = delta_for_peer(ana.facts(4), bill_id, entries, ids[..1].to_vec()).unwrap();
    assert_eq!((one.missing, one.too_big_code.as_deref()), (1, None));
    let read = read_scanned(one.uri.expect("one square"));
    assert_eq!(read.entries.len(), 1, "{:?}", read.refused_code);
}

#[test]
fn only_the_payee_is_asked_to_confirm() {
    let (ana, bill_id, mut entries) = bill();
    let ben = Device::new(90);
    entries.push(
        join_bill_entry(
            ana.facts(4),
            bill_id.clone(),
            Some("Ana".into()),
            Some("u1ana".into()),
            Some(ana.key.clone()),
            vec![],
            ana.seed.clone(),
        )
        .unwrap(),
    );
    entries.push(
        splitz_ffi::record_payment_entry(
            ben.facts(5),
            bill_id.clone(),
            splitz_ffi::PaymentDraft {
                payment_id: "tx-1".into(),
                to: ana.me.clone(),
                amount: 100,
                method: "cash".into(),
                reference: None,
                zatoshi: None,
                paid_at_rate: None,
                note: None,
            },
            ben.seed.clone(),
        )
        .unwrap(),
    );
    let mine = splitz_ffi::awaiting_my_confirmation(ana.facts(6), bill_id.clone(), entries.clone())
        .unwrap();
    assert_eq!(
        mine.iter().map(|p| p.id.as_str()).collect::<Vec<_>>(),
        [format!("{}:tx-1", ben.me)]
    );
    assert!(
        splitz_ffi::awaiting_my_confirmation(ben.facts(6), bill_id, entries)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn blobs_sealed_under_a_key_the_bill_did_not_commit_to_are_not_kept() {
    // §9.4: the create states its key's digest, so a log sealed for somebody
    // under another key opens as foreign.
    let ana = Device::new(1);
    let own = new_bill_key(RandomBytes { bytes: vec![7; 32] }).unwrap();
    let stranger = new_bill_key(RandomBytes { bytes: vec![9; 32] }).unwrap();
    let create = create_bill_entry(
        ana.facts(1),
        "Dinner".into(),
        "EUR".into(),
        "equal".into(),
        ana.key.clone(),
        Some(own.clone()),
        ana.seed.clone(),
    )
    .unwrap();
    let bill_id = serde_json::from_str::<serde_json::Value>(&create).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    let foreign = splitz_ffi::open_blobs(
        splitz_ffi::blobs_to_push(vec![create.clone()], stranger.clone()).unwrap(),
        bill_id.clone(),
        stranger,
    );
    assert!(foreign.foreign_key);
    assert!(foreign.entries.is_empty());
    assert_eq!(foreign.unopenable, 1);
    let honest = splitz_ffi::open_blobs(
        splitz_ffi::blobs_to_push(vec![create], own.clone()).unwrap(),
        bill_id,
        own,
    );
    assert!(!honest.foreign_key);
    assert_eq!(honest.entries.len(), 1);
}

#[test]
fn a_create_that_only_states_the_bill_id_does_not_make_its_key_foreign() {
    // §9.4: only the bill's own create, whose id derives, speaks for the key.
    // Anybody holding the key can seal a create stating the bill's id with
    // another keyDigest, or a real create for another bill.
    let ana = Device::new(1);
    let own = new_bill_key(RandomBytes { bytes: vec![7; 32] }).unwrap();
    let stranger = new_bill_key(RandomBytes { bytes: vec![9; 32] }).unwrap();
    let make = |key: &str| {
        create_bill_entry(
            ana.facts(1),
            "Dinner".into(),
            "EUR".into(),
            "equal".into(),
            ana.key.clone(),
            Some(key.to_owned()),
            ana.seed.clone(),
        )
        .unwrap()
    };
    let genuine = make(&own);
    let bill_id = serde_json::from_str::<serde_json::Value>(&genuine).unwrap()["id"]
        .as_str()
        .unwrap()
        .to_owned();
    // The genuine create with its keyDigest swapped: it still states the id.
    let mut swapped = serde_json::from_str::<serde_json::Value>(&make(&stranger)).unwrap();
    swapped["id"] = serde_json::Value::from(bill_id.clone());
    // A real create, for a bill of its own, committing to another key.
    let other = make(&stranger);
    let blobs =
        splitz_ffi::blobs_to_push(vec![genuine, swapped.to_string(), other], own.clone()).unwrap();
    let opened = splitz_ffi::open_blobs(blobs, bill_id, own);
    assert!(!opened.foreign_key);
    assert_eq!(opened.entries.len(), 3);
    assert_eq!(opened.unopenable, 0);
}
