//! The swap client held to the provider's published schema and to its real
//! answers (see tests/oneclick/mod.rs).

mod oneclick;

use oneclick::{declares, fixture, fixture_text, quote_request_problems, schemas};
use serde_json::{json, Value};
use splitz_host::{
    assets_from_tokens, quote_from_response, quote_request_body, status_from_response, SwapState,
    TradableAsset,
};

fn usdc_on_base() -> TradableAsset {
    TradableAsset {
        asset_id: "nep141:base-0x833589fcd6edb6e08f4c7c32d4f71b54bda02913.omft.near".to_owned(),
        symbol: "USDC".to_owned(),
        chain: "base".to_owned(),
        decimals: 6,
    }
}

fn request(referral: Option<&str>) -> Value {
    let body = quote_request_body(
        "nep141:zec.omft.near",
        &usdc_on_base(),
        1_000_000,
        "0x1111111111111111111111111111111111111111",
        "t1Hsc1LR8yKnbbe3twRp88p6vFfC5t7DLbs",
        "2026-10-28T19:40:00.000Z",
        referral,
    )
    .unwrap();
    serde_json::from_str(&body).unwrap()
}

#[test]
fn what_the_client_sends_is_a_quote_request_the_schema_accepts() {
    for referral in [Some("a-wallet"), None] {
        assert_eq!(
            quote_request_problems(&request(referral)),
            Vec::<String>::new()
        );
    }
}

#[test]
fn the_check_refuses_what_the_provider_refuses() {
    let mut body = request(None);
    body.as_object_mut().unwrap().remove("slippageTolerance");
    assert_eq!(
        quote_request_problems(&body),
        vec!["slippageTolerance is required"]
    );
    assert!(fixture("quote_refused.json")["message"]
        .as_str()
        .unwrap()
        .contains("slippageTolerance should not be empty"));
    body["slippageTolerance"] = json!(0.5);
    assert!(quote_request_problems(&body)[0].contains("integer"));
}

#[test]
fn every_field_the_client_reads_is_one_the_schema_declares() {
    for path in [
        "TokenResponse.assetId",
        "TokenResponse.symbol",
        "TokenResponse.blockchain",
        "TokenResponse.decimals",
        "QuoteResponse.quote",
        "QuoteResponse.correlationId",
        "Quote.depositAddress",
        "Quote.depositMemo",
        "Quote.amountOut",
        "Quote.minAmountOut",
        "Quote.deadline",
        "GetExecutionStatusResponse.status",
        "GetExecutionStatusResponse.swapDetails.destinationChainTxHashes[].hash",
        "GetExecutionStatusResponse.swapDetails.refundReason",
        "BadRequestResponse.message",
    ] {
        assert!(declares(path), "{path}");
    }
    assert!(!declares("GetExecutionStatusResponse.destinationTxHash"));
}

#[test]
fn a_live_quote_reads_into_the_quote_the_provider_issued() {
    let raw = fixture("quote.json");
    let q = quote_from_response(
        &fixture_text("quote.json"),
        &raw["quoteRequest"].to_string(),
        &usdc_on_base(),
        1_000_000,
        "2026-10-28T19:40:00.000Z",
    )
    .unwrap();
    assert_eq!(
        q.deposit_address,
        raw["quote"]["depositAddress"].as_str().unwrap()
    );
    assert_eq!(q.amount_out, raw["quote"]["amountOut"].as_str().unwrap());
    // The floor once slippage is applied: what the payee is guaranteed.
    assert_eq!(
        q.min_amount_out.as_deref(),
        raw["quote"]["minAmountOut"].as_str()
    );
    assert_eq!(
        q.deadline,
        splitz_core::canonical_instant(raw["quote"]["deadline"].as_str().unwrap()).unwrap()
    );
    assert_eq!(q.reference.as_deref(), raw["correlationId"].as_str());
}

#[test]
fn a_dry_quote_issues_no_deposit_address_and_is_refused_as_a_quote() {
    let refused = quote_from_response(
        &fixture_text("quote_dry.json"),
        &fixture("quote_dry.json")["quoteRequest"].to_string(),
        &usdc_on_base(),
        1_000_000,
        "2026-10-28T19:40:00.000Z",
    );
    assert!(format!("{refused:?}").contains("depositAddress"));
}

#[test]
fn the_token_list_carries_zec_on_its_own_chain() {
    let assets = assets_from_tokens(&fixture_text("tokens.json")).unwrap();
    let zec: Vec<_> = assets
        .iter()
        .filter(|a| a.asset_id == "nep141:zec.omft.near")
        .collect();
    assert_eq!(zec.len(), 1);
    assert_eq!(zec[0].chain, "zec");
    assert_eq!(zec[0].decimals, 8);
    assert!(assets.iter().any(|a| a.asset_id == usdc_on_base().asset_id));
}

#[test]
fn a_live_status_of_an_issued_address_reads() {
    assert_eq!(fixture("status.json")["status"], "PENDING_DEPOSIT");
    let status = status_from_response(&fixture_text("status.json")).unwrap();
    assert_eq!(status.state, SwapState::AwaitingDeposit);
    assert_eq!(status.destination_tx_hash, None);
}

#[test]
fn every_status_the_api_lists_maps_to_a_stated_answer() {
    let expected = [
        ("KNOWN_DEPOSIT_TX", SwapState::AwaitingDeposit),
        ("PENDING_DEPOSIT", SwapState::AwaitingDeposit),
        ("INCOMPLETE_DEPOSIT", SwapState::AwaitingDeposit),
        ("PROCESSING", SwapState::Processing),
        ("SUCCESS", SwapState::Delivered),
        ("REFUNDED", SwapState::Failed),
        ("FAILED", SwapState::Failed),
    ];
    let mut listed: Vec<String> = schemas()["GetExecutionStatusResponse"]["properties"]["status"]
        ["enum"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap().to_owned())
        .collect();
    let mut ours: Vec<String> = expected.iter().map(|(k, _)| (*k).to_owned()).collect();
    listed.sort();
    ours.sort();
    assert_eq!(listed, ours, "the API added or removed a status");
    for (word, state) in expected {
        let mut raw = fixture("status.json");
        raw["status"] = json!(word);
        assert_eq!(
            status_from_response(&raw.to_string()).unwrap().state,
            state,
            "{word}"
        );
    }
}
