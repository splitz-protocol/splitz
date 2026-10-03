//! A swap's deposit and its record through the binding (§9.2, §14.3, §15.7):
//! everything a wallet with no Dart needs to send one.

use serde_json::Value;
use splitz_ffi::{
    fiat_to_zatoshi, format_base_units, swap_deposit, swap_payment_entry, ExchangeRate, HostFacts,
    SwapQuote, TradableAsset,
};

fn eur() -> ExchangeRate {
    ExchangeRate {
        currency: "EUR".to_owned(),
        minor_units_per_zec: 51_234,
        at: "2026-10-28T19:30:00.000Z".to_owned(),
        source: None,
    }
}

fn quote(memo: Option<&str>, floor: Option<&str>) -> SwapQuote {
    SwapQuote {
        deposit_address: "t1deposit000000000000000000000000".to_owned(),
        recipient: Some("0xbenbase".to_owned()),
        deposit_memo: memo.map(str::to_owned),
        amount_in_zatoshi: 7_807_316,
        amount_out: "39990000".to_owned(),
        min_amount_out: floor.map(str::to_owned),
        asset: TradableAsset {
            asset_id: "nep141:base-usdc".to_owned(),
            symbol: "USDC".to_owned(),
            chain: "base".to_owned(),
            decimals: 6,
        },
        deadline: "2026-10-29T23:00:00.000Z".to_owned(),
        reference: Some("intent-1".to_owned()),
    }
}

#[test]
fn a_debt_is_sized_in_zatoshi_at_the_bills_rate_rounding_up() {
    // 4000 cents at 51234 cents a ZEC: 4000 * 10^8 / 51234 = 7807315.45…,
    // rounded up.
    assert_eq!(fiat_to_zatoshi(4000, eur()).unwrap(), 7_807_316);
    let lower = ExchangeRate {
        currency: "eur".to_owned(),
        ..eur()
    };
    assert!(fiat_to_zatoshi(4000, lower).is_err());
}

#[test]
fn a_deposit_is_one_request_and_a_note_carrying_the_swap() {
    let plan = swap_deposit(
        "bill-1".to_owned(),
        quote(None, None),
        "ben".to_owned(),
        4000,
        eur(),
        "2026-10-28T19:31:00.000Z".to_owned(),
    )
    .unwrap();
    assert_eq!(
        plan.uri,
        "zcash:t1deposit000000000000000000000000?amount=0.07807316&label=swap%20to%20USDC"
    );
    let note: Value = serde_json::from_str(&plan.note).unwrap();
    assert_eq!(note["uri"], Value::from(plan.uri.clone()));
    assert_eq!(note["swap"]["reference"], Value::from("intent-1"));
    assert_eq!(note["zatoshi"], Value::from(7_807_316));

    for (memo, amount) in [(Some("123"), 4000), (None, 0)] {
        assert!(swap_deposit(
            "bill-1".to_owned(),
            quote(memo, None),
            "ben".to_owned(),
            amount,
            eur(),
            "t".to_owned(),
        )
        .is_err());
    }
}

#[test]
fn a_swap_record_states_its_reference_zatoshi_rate_and_chain() {
    let facts = HostFacts {
        me: "ana".to_owned(),
        now: "2026-10-28T19:32:00.000Z".to_owned(),
        nonce: vec![7; 16],
    };
    let seed = splitz_host::base64url_encode(&[1; 32]);
    let entry = swap_payment_entry(
        facts,
        "bill-1".to_owned(),
        quote(None, Some("39500000")),
        "ben".to_owned(),
        4000,
        eur(),
        seed,
    )
    .unwrap();
    let payment = serde_json::from_str::<Value>(&entry).unwrap()["payment"].clone();
    assert_eq!(payment["id"], Value::from("ana:intent-1"));
    assert_eq!(payment["method"], Value::from("swap"));
    assert_eq!(payment["reference"], Value::from("intent-1"));
    assert_eq!(payment["zatoshi"], Value::from(7_807_316));
    assert_eq!(
        payment["paidAtRate"]["minorUnitsPerZec"],
        Value::from(51_234)
    );
    assert_eq!(payment["note"], Value::from("at least 39.5 USDC on base"));
}

#[test]
fn base_units_read_as_whole_tokens() {
    assert_eq!(
        format_base_units("39990000".to_owned(), 6).as_deref(),
        Some("39.99")
    );
    assert_eq!(format_base_units("x".to_owned(), 6), None);
    assert_eq!(format_base_units("1".to_owned(), 256), None);
}
