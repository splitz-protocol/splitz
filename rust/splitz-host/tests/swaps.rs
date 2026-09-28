//! Settling a debt in an asset that is not ZEC (SPEC.md §9.2, §15.7).

mod oneclick;

use std::cell::RefCell;

use serde_json::{json, Value};
use splitz_host::{
    assets_from_tokens, HostError, HttpTransport, OneClickSwaps, SwapProvider, SwapQuote,
    SwapState, TradableAsset, UnconfiguredSwaps,
};

/// A provider that answers from a script and records what it was asked.
#[derive(Default)]
struct FakeProvider {
    tokens: Option<Value>,
    quote: Option<Value>,
    status: Option<Value>,
    unreachable: bool,
    gets: RefCell<Vec<String>>,
    posts: RefCell<Vec<(String, String)>>,
}

impl HttpTransport for FakeProvider {
    fn post(&self, url: &str, body: &str) -> Result<String, String> {
        if self.unreachable {
            return Err("connection reset".to_owned());
        }
        self.posts
            .borrow_mut()
            .push((url.to_owned(), body.to_owned()));
        // Refused as the provider refuses it: a fake that accepts anything
        // agrees with the client by construction.
        let problems = oneclick::quote_request_problems(&serde_json::from_str(body).unwrap());
        if !problems.is_empty() {
            return Err(format!("answered 400: {}", problems.join(", ")));
        }
        let mut answer = self.quote.clone().unwrap_or_else(|| {
            json!({
                "quote": {"depositAddress": "u1provider", "amountOut": "12340000"},
                "correlationId": "near-intent-7f3a",
            })
        });
        // As the provider answers (tools/contracts/fixtures/quote.json): the
        // request it quoted is echoed, and the quote states the amount in.
        let sent: serde_json::Value = serde_json::from_str(body).unwrap();
        if let Some(object) = answer.as_object_mut() {
            object.entry("quoteRequest").or_insert_with(|| sent.clone());
            if let Some(serde_json::Value::Object(quote)) = object.get_mut("quote") {
                quote
                    .entry("amountIn")
                    .or_insert_with(|| sent["amount"].clone());
            }
        }
        Ok(answer.to_string())
    }

    fn get(&self, url: &str) -> Result<String, String> {
        if self.unreachable {
            return Err("connection reset".to_owned());
        }
        self.gets.borrow_mut().push(url.to_owned());
        if url.contains("/v0/tokens") {
            return Ok(self
                .tokens
                .clone()
                .unwrap_or_else(|| {
                    json!([
                        {"assetId": "nep141:base-usdc", "symbol": "USDC",
                         "blockchain": "base", "decimals": 6},
                        {"assetId": "nep141:arb-usdc", "symbol": "USDC",
                         "blockchain": "arb", "decimals": 6},
                        {"assetId": "nep141:zec", "symbol": "ZEC",
                         "blockchain": "zec", "decimals": 8},
                    ])
                })
                .to_string());
        }
        Ok(self
            .status
            .clone()
            .unwrap_or_else(|| json!({"status": "PENDING_DEPOSIT"}))
            .to_string())
    }
}

const DEADLINE: &str = "2026-10-28T19:40:00.000Z";

fn deadline() -> String {
    DEADLINE.to_owned()
}

fn provider(fake: &FakeProvider) -> OneClickSwaps<'_> {
    OneClickSwaps::new(
        "https://swap.example",
        "nep141:zec",
        fake,
        &deadline,
        Some("a-wallet"),
    )
    .unwrap()
}

fn usdc_on_base() -> TradableAsset {
    TradableAsset {
        asset_id: "nep141:base-usdc".to_owned(),
        symbol: "USDC".to_owned(),
        chain: "base".to_owned(),
        decimals: 6,
    }
}

#[test]
fn an_asset_is_matched_on_both_the_symbol_and_the_chain() {
    // One symbol exists on many chains. A swap matched on the symbol alone
    // delivers the right token to the wrong network.
    let fake = FakeProvider::default();
    let assets = provider(&fake).tradable_assets().unwrap();
    let base: Vec<&TradableAsset> = assets
        .iter()
        .filter(|a| a.answers("usdc", "base"))
        .collect();
    assert_eq!(base.len(), 1);
    assert_eq!(base[0].asset_id, "nep141:base-usdc");
    assert!(!assets[0].answers("USDC", "arb"));
}

#[test]
fn the_token_list_is_read_once() {
    let fake = FakeProvider::default();
    let swaps = provider(&fake);
    swaps.tradable_assets().unwrap();
    swaps.tradable_assets().unwrap();
    assert_eq!(fake.gets.borrow().len(), 1);
}

#[test]
fn the_quote_states_zec_in_the_asset_out_and_where_a_refund_goes() {
    let fake = FakeProvider::default();
    let quote = provider(&fake)
        .quote(&usdc_on_base(), 1_000_000, "0xcara", "u1ana")
        .unwrap();
    assert_eq!(quote.deposit_address, "u1provider");
    assert_eq!(quote.amount_in_zatoshi, 1_000_000);
    assert_eq!(quote.amount_out, "12340000");
    let (_, body) = fake.posts.borrow()[0].clone();
    let sent: Value = serde_json::from_str(&body).unwrap();
    assert_eq!(sent["originAsset"], "nep141:zec");
    assert_eq!(sent["destinationAsset"], "nep141:base-usdc");
    assert_eq!(sent["refundTo"], "u1ana");
    assert_eq!(sent["recipient"], "0xcara");
    // Required by the provider: a quote without it is refused with a 400.
    assert_eq!(
        sent["slippageTolerance"],
        OneClickSwaps::SLIPPAGE_BASIS_POINTS
    );
    assert_eq!(sent["slippageTolerance"], 100);
    assert_eq!(sent["amount"], "1000000");
    assert_eq!(sent["referral"], "a-wallet");
    assert_eq!(sent["deadline"], DEADLINE);
}

#[test]
fn the_reference_is_the_intent_id_not_a_txid() {
    // §9.2: a payment record's `reference` for a swap is the provider's own
    // identifier, and a reader that renders it as a txid is wrong every time.
    let fake = FakeProvider::default();
    let quote = provider(&fake)
        .quote(&usdc_on_base(), 1_000_000, "0xcara", "u1ana")
        .unwrap();
    assert_eq!(quote.reference.as_deref(), Some("near-intent-7f3a"));
    assert_eq!(quote.payment_reference(), "near-intent-7f3a");
}

#[test]
fn with_no_identifier_the_deposit_address_is_the_reference() {
    let fake = FakeProvider {
        quote: Some(json!({
            "quote": {"depositAddress": "u1provider", "amountOut": "1"}
        })),
        ..Default::default()
    };
    let quote = provider(&fake)
        .quote(&usdc_on_base(), 1_000_000, "0xcara", "u1ana")
        .unwrap();
    assert_eq!(quote.reference, None);
    assert_eq!(quote.payment_reference(), "u1provider");
}

#[test]
fn the_providers_own_deadline_wins_over_ours() {
    // Honouring a longer deadline of ours would quote a price it has stopped
    // holding.
    let fake = FakeProvider {
        quote: Some(json!({
            "quote": {"depositAddress": "u1provider", "amountOut": "1",
                      "deadline": "2026-10-28T19:35:00Z"}
        })),
        ..Default::default()
    };
    let quote = provider(&fake)
        .quote(&usdc_on_base(), 1_000_000, "0xcara", "u1ana")
        .unwrap();
    assert_eq!(quote.deadline, "2026-10-28T19:35:00.000Z");
    assert!(!quote.has_expired("2026-10-28T19:34:00.000Z"));
    assert!(quote.has_expired("2026-10-28T19:36:00.000Z"));
}

#[test]
fn a_swap_that_sends_nothing_is_refused() {
    let fake = FakeProvider::default();
    for amount in [0, -1] {
        assert!(provider(&fake)
            .quote(&usdc_on_base(), amount, "0xcara", "u1ana")
            .is_err());
    }
    assert!(fake.posts.borrow().is_empty(), "nothing was asked for");
}

#[test]
fn a_quote_with_no_refund_address_is_refused() {
    // A quote with no refund address risks the whole deposit if the swap
    // fails, which is the one failure the payer cannot recover from.
    let fake = FakeProvider::default();
    assert!(provider(&fake)
        .quote(&usdc_on_base(), 1_000_000, "0xcara", "")
        .is_err());
    assert!(provider(&fake)
        .quote(&usdc_on_base(), 1_000_000, "", "u1ana")
        .is_err());
}

#[test]
fn a_response_with_no_deposit_address_is_refused() {
    let fake = FakeProvider {
        quote: Some(json!({"quote": {"amountOut": "1"}})),
        ..Default::default()
    };
    match provider(&fake).quote(&usdc_on_base(), 1_000_000, "0xcara", "u1ana") {
        Err(HostError::Swap { message, .. }) => assert!(message.contains("depositAddress")),
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn a_transport_failure_is_reported_as_retryable() {
    let fake = FakeProvider {
        unreachable: true,
        ..Default::default()
    };
    match provider(&fake).quote(&usdc_on_base(), 1_000_000, "0xcara", "u1ana") {
        Err(HostError::Swap { message, transient }) => {
            assert!(message.contains("could not be reached"));
            assert!(transient);
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

fn a_quote(memo: Option<&str>) -> SwapQuote {
    SwapQuote {
        deposit_address: "u1provider".to_owned(),
        recipient: None,
        deposit_memo: memo.map(str::to_owned),
        amount_in_zatoshi: 1,
        amount_out: "1".to_owned(),
        min_amount_out: None,
        asset: usdc_on_base(),
        deadline: "2026-01-01T00:00:00.000Z".to_owned(),
        reference: None,
    }
}

#[test]
fn the_provider_vocabulary_maps_onto_the_states() {
    for (word, expected) in [
        ("PENDING_DEPOSIT", SwapState::AwaitingDeposit),
        ("KNOWN_DEPOSIT_TX", SwapState::AwaitingDeposit),
        ("SUCCESS", SwapState::Delivered),
        ("FAILED", SwapState::Failed),
        ("INCOMPLETE_DEPOSIT", SwapState::AwaitingDeposit),
        ("REFUNDED", SwapState::Failed),
        ("PROCESSING", SwapState::Processing),
    ] {
        let fake = FakeProvider {
            status: Some(json!({"status": word})),
            ..Default::default()
        };
        let status = provider(&fake).status_of(&a_quote(None)).unwrap();
        assert_eq!(status.state, expected, "{word}");
    }
}

#[test]
fn a_refund_under_way_is_told_apart_from_a_delivery_under_way() {
    // The provider answered PROCESSING with refundedAmount 103000 for a
    // mainnet deposit short of its quote, before it reported REFUNDED.
    for (word, refunded, expected) in [
        ("PROCESSING", json!("103000"), SwapState::Refunding),
        ("INCOMPLETE_DEPOSIT", json!("5"), SwapState::Refunding),
        ("something_new", json!("1"), SwapState::Refunding),
        (
            "PROCESSING",
            json!("99999999999999999999999"),
            SwapState::Refunding,
        ),
        ("PROCESSING", json!("0"), SwapState::Processing),
        ("PROCESSING", json!("000"), SwapState::Processing),
        ("PROCESSING", json!(""), SwapState::Processing),
        ("PROCESSING", json!("-5"), SwapState::Processing),
        ("PROCESSING", json!("1e3"), SwapState::Processing),
        ("PROCESSING", json!(103000), SwapState::Processing),
        ("SUCCESS", json!("103000"), SwapState::Delivered),
        ("REFUNDED", json!("103000"), SwapState::Failed),
        ("FAILED", json!("103000"), SwapState::Failed),
    ] {
        let fake = FakeProvider {
            status: Some(json!({"status": word, "swapDetails": {"refundedAmount": refunded}})),
            ..Default::default()
        };
        let status = provider(&fake).status_of(&a_quote(None)).unwrap();
        assert_eq!(status.state, expected, "{word} with {refunded}");
    }
}

#[test]
fn a_word_nobody_here_defined_is_not_read_as_delivered() {
    // Reading an unknown word as success would tell a payer their debt is
    // settled on the strength of a string nobody here has defined.
    let fake = FakeProvider {
        status: Some(json!({"status": "SOMETHING_NEW"})),
        ..Default::default()
    };
    let status = provider(&fake).status_of(&a_quote(None)).unwrap();
    assert_eq!(status.state, SwapState::Processing);
    assert_ne!(status.state, SwapState::Delivered);
}

#[test]
fn the_memo_travels_with_the_address() {
    // Some chains lose a deposit sent without its memo.
    let fake = FakeProvider::default();
    provider(&fake).status_of(&a_quote(Some("memo-1"))).unwrap();
    let asked = fake.gets.borrow().last().unwrap().clone();
    assert!(asked.contains("depositMemo=memo-1"), "{asked}");
    assert!(asked.contains("depositAddress=u1provider"), "{asked}");

    let fake = FakeProvider::default();
    provider(&fake).status_of(&a_quote(None)).unwrap();
    assert!(!fake.gets.borrow().last().unwrap().contains("depositMemo"));
}

#[test]
fn a_build_with_no_provider_says_so_rather_than_doing_nothing() {
    let swaps = UnconfiguredSwaps;
    assert!(swaps.tradable_assets().unwrap().is_empty());
    for outcome in [
        swaps.quote(&usdc_on_base(), 1, "0xcara", "u1ana").err(),
        swaps.status_of(&a_quote(None)).err(),
    ] {
        match outcome {
            Some(HostError::Swap { message, transient }) => {
                assert!(message.contains("no swap provider"));
                assert!(!transient, "no retry configures one");
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
    }
}

#[test]
fn an_origin_carrying_a_query_or_a_fragment_is_refused() {
    let fake = FakeProvider::default();
    for origin in ["https://swap.example?t=1", "https://swap.example#top"] {
        assert!(
            OneClickSwaps::new(origin, "nep141:zec", &fake, &deadline, None).is_err(),
            "{origin}"
        );
    }
}

#[test]
fn the_hash_and_the_reason_come_from_swap_details() {
    let status = |body: Value| {
        let fake = FakeProvider {
            status: Some(body),
            ..Default::default()
        };
        provider(&fake).status_of(&a_quote(None)).unwrap()
    };
    let delivered = status(json!({
        "status": "SUCCESS",
        "swapDetails": {"destinationChainTxHashes": [
            {"hash": "0xbase-tx", "explorerUrl": "https://x"}
        ]}
    }));
    assert_eq!(delivered.destination_tx_hash.as_deref(), Some("0xbase-tx"));

    let refunded = status(json!({
        "status": "REFUNDED",
        "swapDetails": {"refundReason": "deposit below the minimum"}
    }));
    assert_eq!(
        refunded.detail.as_deref(),
        Some("deposit below the minimum")
    );

    // Keys the provider does not send at the top level are not read there.
    let stray = status(json!({
        "status": "SUCCESS",
        "destinationTxHash": "0xtop",
        "message": "top-level"
    }));
    assert_eq!(stray.destination_tx_hash, None);
    assert_eq!(stray.detail, None);
}

#[test]
fn a_quote_for_another_recipient_is_refused() {
    // The provider's answer names the request it quoted. One naming another
    // recipient would send this payer's money to them.
    let fake = FakeProvider {
        quote: Some(json!({
            "quote": {"depositAddress": "u1provider", "amountOut": "1"},
            "quoteRequest": {
                "recipient": "0xsomebodyelse",
                "destinationAsset": "nep141:base-usdc",
                "originAsset": "nep141:zec",
                "amount": "1000000",
                "refundTo": "u1ana",
                "swapType": "EXACT_INPUT",
            },
        })),
        ..Default::default()
    };
    match provider(&fake).quote(&usdc_on_base(), 1_000_000, "0xcara", "u1ana") {
        Err(HostError::Swap { message, transient }) => {
            assert!(message.contains("recipient"), "{message}");
            assert!(!transient);
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
}

#[test]
fn a_quote_for_another_amount_in_is_refused() {
    let fake = FakeProvider {
        quote: Some(json!({
            "quote": {"depositAddress": "u1provider", "amountOut": "1", "amountIn": "5000000"},
        })),
        ..Default::default()
    };
    assert!(provider(&fake)
        .quote(&usdc_on_base(), 1_000_000, "0xcara", "u1ana")
        .is_err());
}

#[test]
fn a_token_that_states_no_decimals_is_refused_not_guessed() {
    let listing = |decimals: Option<Value>| {
        let mut token = json!({
            "assetId": "nep141:base-usdc",
            "symbol": "USDC",
            "blockchain": "base",
        });
        if let Some(d) = decimals {
            token["decimals"] = d;
        }
        assets_from_tokens(&json!([token]).to_string())
    };
    assert_eq!(listing(Some(json!(6))).unwrap()[0].decimals, 6);
    for bad in [None, Some(json!("6")), Some(json!(-1)), Some(json!(6.5))] {
        let err = listing(bad.clone()).unwrap_err();
        assert!(err.to_string().contains("decimals"), "{bad:?}: {err}");
    }
}
