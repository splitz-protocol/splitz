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
    agreed_price, price_from_binance, price_from_coinbase, AgreeingZecPrices, BinanceZecPrices,
    CoinbaseZecPrices, FirstZecPrices, FixedZecPrices, HostError, HttpTransport, Result, ZecPrices,
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
        ["https://data-api.binance.vision/api/v3/ticker/price?symbol=ZECUSDC"]
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

fn agreeing(a: &Source, b: &Source) -> Option<i64> {
    AgreeingZecPrices::new(a, b)
        .minor_units_per_zec("USD")
        .unwrap()
}

#[test]
fn two_markets_that_agree_give_the_second_ones_figure() {
    let first = FixedZecPrices::new([("USD".to_owned(), 138_819)]);
    let second = FixedZecPrices::new([("USD".to_owned(), 138_905), ("EUR".to_owned(), 122_241)]);
    let prices = AgreeingZecPrices::new(&first, &second);
    assert_eq!(prices.minor_units_per_zec("USD").unwrap(), Some(138_905));
    assert_eq!(prices.minor_units_per_zec("EUR").unwrap(), Some(122_241));
    assert_eq!(prices.minor_units_per_zec("GBP").unwrap(), None);
}

#[test]
fn two_markets_that_disagree_give_no_price() {
    // (152701 - 138819) x 10000 = 138820000 > 138819 x 200 = 27763800:
    // 1000 bp apart, past the 200 allowed.
    assert_eq!(
        agreeing(&source(Some(138_819), false), &source(Some(152_701), false)),
        None
    );
    assert_eq!(
        agreeing(&source(Some(152_701), false), &source(Some(138_819), false)),
        None
    );
}

#[test]
fn the_tolerance_is_inclusive_in_basis_points_of_the_lower() {
    // 10000 x 1.02 = 10200: exactly 200 bp, then one past it.
    let at = |a, b| agreeing(&source(Some(a), false), &source(Some(b), false));
    assert_eq!(at(10_000, 10_200), Some(10_200));
    assert_eq!(at(10_200, 10_000), Some(10_000));
    assert_eq!(at(10_000, 10_201), None);
    assert_eq!(at(10_201, 10_000), None);
}

#[test]
fn a_tolerance_the_caller_chose_is_the_one_applied() {
    let (a, b) = (source(Some(10_000), false), source(Some(10_201), false));
    let mut prices = AgreeingZecPrices::new(&a, &b);
    prices.tolerance_bp = 201;
    assert_eq!(prices.minor_units_per_zec("USD").unwrap(), Some(10_201));
    prices.tolerance_bp = 0;
    assert_eq!(prices.minor_units_per_zec("USD").unwrap(), None);
    assert_eq!(agreed_price(Some(7), Some(7), 0), Some(7));
}

#[test]
fn figures_at_the_2_pow_53_bound_do_not_overflow() {
    let top = 9_007_199_254_740_991;
    assert_eq!(agreed_price(Some(top), Some(top), 200), Some(top));
    assert_eq!(agreed_price(Some(top / 2), Some(top), 200), None);
    assert_eq!(
        agreed_price(Some(top), Some(top / 2), u32::MAX),
        Some(top / 2)
    );
}

#[test]
fn one_market_answering_stands_alone() {
    assert_eq!(
        agreeing(&source(None, false), &source(Some(138_905), false)),
        Some(138_905)
    );
    assert_eq!(
        agreeing(&source(Some(138_819), false), &source(None, false)),
        Some(138_819)
    );
    assert_eq!(
        agreeing(&source(None, true), &source(Some(138_905), false)),
        Some(138_905)
    );
    assert_eq!(
        agreeing(&source(Some(138_819), false), &source(None, true)),
        Some(138_819)
    );
}

#[test]
fn a_figure_that_is_not_positive_is_not_an_answer() {
    assert_eq!(
        agreeing(&source(Some(0), false), &source(Some(138_905), false)),
        Some(138_905)
    );
    assert_eq!(
        agreeing(&source(Some(-5), false), &source(Some(138_905), false)),
        Some(138_905)
    );
    assert_eq!(
        agreeing(&source(Some(138_819), false), &source(Some(0), false)),
        Some(138_819)
    );
    assert_eq!(
        agreeing(&source(Some(0), false), &source(Some(0), false)),
        None
    );
}

#[test]
fn markets_that_cannot_be_reached_leave_the_bill_unpriced() {
    assert_eq!(agreeing(&source(None, true), &source(None, true)), None);
}

#[test]
fn both_markets_are_asked_every_time() {
    let (a, b) = (source(Some(100), false), source(Some(200), false));
    agreeing(&a, &b);
    assert_eq!((a.asked.get(), b.asked.get()), (1, 1));
}
