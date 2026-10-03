//! What a wallet hands the binding about itself: the identity a mnemonic
//! derives (§15.1), the order a received transaction's id is in (§14.7), and
//! the currency a bill is opened in (§2.1).

use splitz_ffi::{
    create_bill_entry, identity_seed_from_mnemonic, txid_in_send_order, HostFacts, SplitzError,
};

const MNEMONIC: &str = concat!(
    "abandon abandon abandon abandon abandon abandon ",
    "abandon abandon abandon abandon abandon about"
);

#[test]
fn a_mnemonic_derives_the_seed_every_wallet_derives() {
    // Pinned against seeds computed outside this crate, in Python.
    let seed = |passphrase: &str, account: u32| {
        identity_seed_from_mnemonic(MNEMONIC.to_owned(), passphrase.to_owned(), account).unwrap()
    };
    assert_eq!(seed("", 0), "Bsu7QAZyG9usbFpPUrQUo5ni3MtX7JeU-rUHoWKUPKY");
    assert_eq!(
        seed("TREZOR", 0),
        "NoXrjBFhw-SnV_XZyW7Pe9JyZ5s3G17lAWIl76cxbVU"
    );
    assert_eq!(seed("", 1), "jWQijb3QECEvHgOUb4_W7UPiUujN7D-7Nq0jYg5mrKo");
}

#[test]
fn an_empty_mnemonic_and_an_index_past_zip32_are_refused() {
    for (mnemonic, account) in [("", 0), (MNEMONIC, 0x8000_0000)] {
        let refused =
            identity_seed_from_mnemonic(mnemonic.to_owned(), String::new(), account).unwrap_err();
        assert!(matches!(
            refused,
            SplitzError::Host {
                transient: false,
                ..
            }
        ));
    }
}

#[test]
fn a_txid_in_digest_order_is_reversed() {
    let digest = "00112233445566778899aabbccddeeff0123456789abcdef0f1e2d3c4b5a6978";
    assert_eq!(
        txid_in_send_order(digest.to_owned()).as_deref(),
        Some("78695a4b3c2d1e0fefcdab8967452301ffeeddccbbaa99887766554433221100")
    );
    assert_eq!(txid_in_send_order("abcd".to_owned()), None);
}

fn facts() -> HostFacts {
    HostFacts {
        me: "ana".to_owned(),
        now: "2026-10-28T19:30:00.000Z".to_owned(),
        nonce: vec![7; 16],
    }
}

fn open(currency: &str) -> Result<String, SplitzError> {
    let seed = splitz_host::base64url_encode(&[1; 32]);
    let key = splitz_ffi::identity_key_from_seed(seed.clone()).unwrap();
    create_bill_entry(
        facts(),
        "Dinner".to_owned(),
        currency.to_owned(),
        "equal".to_owned(),
        key,
        None,
        seed,
    )
}

#[test]
fn a_bill_is_opened_only_in_a_currency_amounts_can_be_typed_in() {
    assert!(open("EUR").is_ok());
    assert!(open("JPY").is_ok());
    // Well formed, and ISO 4217 gives it no minor unit.
    assert!(matches!(open("XAU"), Err(SplitzError::Host { .. })));
    // Malformed: the §12 code every reader refuses it with.
    match open("eur") {
        Err(SplitzError::Protocol { code, .. }) => assert_eq!(code, "bill_bad_currency"),
        other => panic!("{other:?}"),
    }
}

#[test]
fn a_typed_figure_is_read_at_its_currencys_exponent() {
    use splitz_ffi::{parse_amount_in, parse_minor_units};
    assert_eq!(parse_amount_in("12.34".into(), "EUR".into()), Some(1234));
    assert_eq!(parse_amount_in("1,000".into(), "KWD".into()), None);
    assert_eq!(parse_amount_in("12".into(), "XAU".into()), None);
    assert_eq!(parse_minor_units("12.5".into(), 2), Some(1250));
    assert_eq!(parse_minor_units(".".into(), 2), None);
}

#[test]
fn a_note_naming_its_transaction_is_held_while_it_waits_or_once_mined() {
    use splitz_ffi::{pending_send_named_refusal, NamedSendRefusal, TransactionState};
    let named = r#"{"billId":"b1","uri":"zcash:u1ben?amount=0.1","carried":{"ben":1000},"at":"2026-10-28T19:30:00.000Z","txid":"abab"}"#;
    let ask = |state| pending_send_named_refusal("b1".into(), named.into(), state);
    assert_eq!(
        ask(Some(TransactionState::Waiting)),
        Some(NamedSendRefusal::Waiting)
    );
    assert_eq!(
        ask(Some(TransactionState::Mined)),
        Some(NamedSendRefusal::Mined)
    );
    assert_eq!(ask(Some(TransactionState::Expired)), None);
    assert_eq!(ask(None), None);
}
