//! The Coinbase and Binance price sources held to the providers' real
//! answers, and the order `FirstZecPrices` asks them in.
//!
//! `tools/contracts/coinbase_cases.json` and `binance_cases.json` pair
//! answers — one captured from each live service, the rest built in its
//! shape — with what they must read as, computed by
//! `tools/contracts/coinbase.py` and `binance.py` in exact decimals.

use std::cell::{Cell, RefCell};

use serde_json::Value;
use splitz_host::{
    price_from_binance, price_from_coinbase, BinanceZecPrices, CoinbaseZecPrices, FirstZecPrices,
    HostError, HttpTransport, Result, ZecPrices,
};

fn contracts() -> String {
    concat!(env!("CARGO_MANIFEST_DIR"), "/../../tools/contracts").to_owned()
}

fn every_case(file: &str, read: fn(&str, &str) -> Result<Option<i64>>) {
    let doc: Value =
        serde_json::from_str(&std::fs::read_to_string(format!("{}/{file}", contracts())).unwrap())
            .unwrap();
    let cases = doc["cases"].as_array().unwrap();
    assert_eq!(cases.len() as u64, doc["count"].as_u64().unwrap());
    let mut failures = Vec::new();
    for c in cases {
        let name = c["name"].as_str().unwrap();
        let got = read(c["body"].as_str().unwrap(), c["currency"].as_str().unwrap());
        match (c.get("error"), got) {
            (Some(_), Err(HostError::Price(_))) => {}
            (None, Ok(v)) if Value::from(v) == c["expect"] => {}
            (_, other) => failures.push(format!("{name}: {other:?}, want {c}")),
        }
    }
    assert!(failures.is_empty(), "{file}:\n{}", failures.join("\n"));
}

#[test]
fn every_coinbase_case_reads_as_the_reference_says() {
    every_case("coinbase_cases.json", price_from_coinbase);
}

#[test]
fn every_binance_case_reads_as_the_reference_says() {
    every_case("binance_cases.json", price_from_binance);
}

struct Captured {
    asked: RefCell<Vec<String>>,
    answer: std::result::Result<String, String>,
}

impl HttpTransport for Captured {
    fn post(&self, _url: &str, _body: &str) -> std::result::Result<String, String> {
        Err("not used".to_owned())
    }
    fn get(&self, url: &str) -> std::result::Result<String, String> {
        self.asked.borrow_mut().push(url.to_owned());
        self.answer.clone()
    }
}

fn fixture(name: &str) -> std::result::Result<String, String> {
    Ok(std::fs::read_to_string(format!("{}/fixtures/{name}", contracts())).unwrap())
}

#[test]
fn the_coinbase_client_asks_once_for_zec_under_its_origin() {
    let transport = Captured {
        asked: RefCell::new(Vec::new()),
        answer: fixture("coinbase_rates.json"),
    };
    let prices = CoinbaseZecPrices::new("https://api.coinbase.com/", &transport);
    assert!(prices.minor_units_per_zec("KES").unwrap().unwrap() > 0);
    assert_eq!(prices.minor_units_per_zec("XAU").unwrap(), None);
    assert_eq!(prices.minor_units_per_zec("usd").unwrap(), None);
    assert_eq!(
        transport.asked.borrow().as_slice(),
        ["https://api.coinbase.com/v2/exchange-rates?currency=ZEC"]
    );
}

#[test]
fn the_binance_client_asks_for_the_ticker_and_prices_usd_only() {
    let transport = Captured {
        asked: RefCell::new(Vec::new()),
        answer: fixture("binance_ticker.json"),
    };
    let prices = BinanceZecPrices::new("https://data-api.binance.vision/", &transport);
    assert!(prices.minor_units_per_zec("USD").unwrap().unwrap() > 0);
    assert_eq!(prices.minor_units_per_zec("EUR").unwrap(), None);
    assert_eq!(
        transport.asked.borrow().as_slice(),
        ["https://data-api.binance.vision/api/v3/ticker/price?symbol=ZECUSDT"]
    );
}

#[test]
fn a_binance_fetch_that_fails_is_refused_rather_than_read_as_unpriced() {
    let transport = Captured {
        asked: RefCell::new(Vec::new()),
        answer: Err("unreachable".to_owned()),
    };
    let prices = BinanceZecPrices::new("https://data-api.binance.vision", &transport);
    assert!(matches!(
        prices.minor_units_per_zec("USD"),
        Err(HostError::Price(_))
    ));
}

/// A source answering `price`, or failing when `fails`.
struct Source {
    price: Option<i64>,
    fails: bool,
    asked: Cell<u32>,
}

fn source(price: Option<i64>, fails: bool) -> Source {
    Source {
        price,
        fails,
        asked: Cell::new(0),
    }
}

impl ZecPrices for Source {
    fn minor_units_per_zec(&self, _currency: &str) -> Result<Option<i64>> {
        self.asked.set(self.asked.get() + 1);
        if self.fails {
            return Err(HostError::Price("unreachable".to_owned()));
        }
        Ok(self.price)
    }
}

#[test]
fn a_source_that_fails_is_passed_over_for_the_next() {
    let (down, up) = (source(None, true), source(Some(139_028), false));
    let first = FirstZecPrices {
        sources: vec![&down, &up],
    };
    assert_eq!(first.minor_units_per_zec("USD").unwrap(), Some(139_028));
    assert_eq!((down.asked.get(), up.asked.get()), (1, 1));
}

#[test]
fn an_earlier_price_stops_the_asking() {
    let (a, b) = (source(Some(100), false), source(Some(200), false));
    let first = FirstZecPrices {
        sources: vec![&a, &b],
    };
    assert_eq!(first.minor_units_per_zec("USD").unwrap(), Some(100));
    assert_eq!(b.asked.get(), 0);
}

#[test]
fn unpriced_by_some_and_failed_by_the_rest_is_unpriced() {
    let (a, b) = (source(None, false), source(None, true));
    let first = FirstZecPrices {
        sources: vec![&a, &b],
    };
    assert_eq!(first.minor_units_per_zec("KES").unwrap(), None);
}

#[test]
fn every_source_failing_is_refused() {
    let (a, b) = (source(None, true), source(None, true));
    let first = FirstZecPrices {
        sources: vec![&a, &b],
    };
    assert!(matches!(
        first.minor_units_per_zec("USD"),
        Err(HostError::Price(_))
    ));
}
