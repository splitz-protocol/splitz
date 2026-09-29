//! The CoinGecko price source held to the provider's real answers.
//!
//! `tools/contracts/coingecko_cases.json` pairs answers — one captured from
//! the live service, the rest built in its shape — with what they must read
//! as, computed by `tools/contracts/coingecko.py` in exact decimals.

use std::cell::RefCell;

use serde_json::Value;
use splitz_host::{price_from_coingecko, CoinGeckoZecPrices, HostError, HttpTransport, ZecPrices};

fn contracts() -> String {
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/contracts").to_owned()
}

#[test]
fn every_case_reads_as_the_reference_says() {
    let doc: Value = serde_json::from_str(
        &std::fs::read_to_string(format!("{}/coingecko_cases.json", contracts())).unwrap(),
    )
    .unwrap();
    let cases = doc["cases"].as_array().unwrap();
    assert_eq!(cases.len() as u64, doc["count"].as_u64().unwrap());
    let mut failures = Vec::new();
    for c in cases {
        let name = c["name"].as_str().unwrap();
        let got =
            price_from_coingecko(c["body"].as_str().unwrap(), c["currency"].as_str().unwrap());
        match (c.get("error"), got) {
            (Some(_), Err(HostError::Price(_))) => {}
            (None, Ok(v)) if Value::from(v) == c["expect"] => {}
            (_, other) => failures.push(format!("{name}: {other:?}, want {c}")),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

struct Captured {
    asked: RefCell<Vec<String>>,
    answer: Result<String, String>,
}

impl HttpTransport for Captured {
    fn post(&self, _url: &str, _body: &str) -> Result<String, String> {
        Err("not used".to_owned())
    }
    fn get(&self, url: &str) -> Result<String, String> {
        self.asked.borrow_mut().push(url.to_owned());
        self.answer.clone()
    }
}

#[test]
fn the_client_asks_for_one_currency_lower_cased_under_its_origin() {
    let transport = Captured {
        asked: RefCell::new(Vec::new()),
        answer: Ok(std::fs::read_to_string(format!(
            "{}/fixtures/coingecko_price.json",
            contracts()
        ))
        .unwrap()),
    };
    let prices = CoinGeckoZecPrices::new("https://api.coingecko.com/api/v3/", &transport);
    assert_eq!(prices.minor_units_per_zec("EUR").unwrap(), Some(122_241));
    assert_eq!(
        transport.asked.borrow().as_slice(),
        ["https://api.coingecko.com/api/v3/simple/price?ids=zcash&vs_currencies=eur"]
    );
}

#[test]
fn the_client_does_not_ask_for_a_currency_it_cannot_scale() {
    let transport = Captured {
        asked: RefCell::new(Vec::new()),
        answer: Ok(r#"{"zcash":{}}"#.to_owned()),
    };
    let prices = CoinGeckoZecPrices::new("https://api.coingecko.com/api/v3", &transport);
    assert_eq!(prices.minor_units_per_zec("XAU").unwrap(), None);
    assert_eq!(prices.minor_units_per_zec("usd").unwrap(), None);
    assert!(transport.asked.borrow().is_empty());
}

#[test]
fn a_fetch_that_fails_is_refused_rather_than_read_as_unpriced() {
    let transport = Captured {
        asked: RefCell::new(Vec::new()),
        answer: Err("unreachable".to_owned()),
    };
    let prices = CoinGeckoZecPrices::new("https://api.coingecko.com/api/v3", &transport);
    assert!(matches!(
        prices.minor_units_per_zec("USD"),
        Err(HostError::Price(_))
    ));
}
