//! §15.5's work: the stamp every implementation finds, and a push stamped
//! again when the relay asks for more.

use std::cell::RefCell;

use serde_json::{json, Value};
use splitz_host::{
    leading_zero_bits, relay_work_bits, relay_work_holds, relay_work_stamp, HttpSplitsRelay,
    HttpTransport, SplitsRelay,
};

const CHANNEL: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

fn blobs(of: &[&str]) -> Vec<String> {
    of.iter().map(|s| s.to_string()).collect()
}

#[test]
fn the_stamp_is_the_one_tools_relay_test_server_pins() {
    assert_eq!(relay_work_stamp(CHANNEL, &blobs(&["x"]), 16), "127719");
    assert_eq!(relay_work_stamp(CHANNEL, &blobs(&["x", "y"]), 16), "103390");
    assert!(relay_work_holds(CHANNEL, &blobs(&["x"]), "127719", 16));
    assert!(!relay_work_holds(CHANNEL, &blobs(&["y"]), "127719", 16));
    // One list of blobs re-split into another is another stamp.
    let joined = relay_work_stamp(CHANNEL, &blobs(&["a\nb"]), 16);
    assert!(!relay_work_holds(CHANNEL, &blobs(&["a", "b"]), &joined, 16));
}

#[test]
fn the_bits_grow_with_the_units_a_push_carries() {
    let bits: Vec<u32> = [0, 1, 1024, 1025, 2048, 2049, 65536, 65537]
        .iter()
        .map(|&c| relay_work_bits(c, 16))
        .collect();
    assert_eq!(bits, [16, 16, 16, 17, 17, 18, 22, 23]);
    assert_eq!(relay_work_bits(1, 20), 20);
    assert_eq!(leading_zero_bits(&[0, 0x0f]), 12);
    assert_eq!(leading_zero_bits(&[0x80]), 0);
    assert_eq!(leading_zero_bits(&[0, 0]), 16);
}

/// A relay asking each base in turn, answering as tools/relay/server.py does:
/// 428 with the base when the work is short or missing.
struct Asking {
    asks: Vec<u32>,
    seen: RefCell<Vec<Value>>,
}

impl HttpTransport for Asking {
    fn post(&self, url: &str, body: &str) -> Result<String, String> {
        let push: Value = serde_json::from_str(body).unwrap();
        let call = self.seen.borrow().len().min(self.asks.len() - 1);
        self.seen.borrow_mut().push(push.clone());
        let base = self.asks[call];
        let blobs: Vec<String> = push["blobs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|b| b.as_str().unwrap().to_owned())
            .collect();
        let chars = blobs.iter().map(String::len).sum();
        let channel = url.rsplit('/').next().unwrap();
        let ok = push["work"]
            .as_str()
            .is_some_and(|w| relay_work_holds(channel, &blobs, w, relay_work_bits(chars, base)));
        Ok(if ok {
            json!({"ok": true})
        } else {
            json!({"error": "not enough work", "work": base})
        }
        .to_string())
    }

    fn get(&self, _url: &str) -> Result<String, String> {
        Ok(r#"{"blobs":[]}"#.to_owned())
    }
}

fn pushed(asks: Vec<u32>) -> (bool, Vec<Value>) {
    let transport = Asking {
        asks,
        seen: RefCell::new(Vec::new()),
    };
    let relay = HttpSplitsRelay::new("https://relay.test", &transport).unwrap();
    let ok = relay.push(CHANNEL, &blobs(&["x"])).is_ok();
    (ok, transport.seen.into_inner())
}

#[test]
fn a_push_carries_a_stamp_at_the_base() {
    let (ok, seen) = pushed(vec![16]);
    assert!(ok);
    assert_eq!(seen, [json!({"blobs": ["x"], "work": "127719"})]);
}

#[test]
fn a_push_is_stamped_once_more_at_the_base_a_refusal_asks() {
    let (ok, seen) = pushed(vec![18, 18]);
    assert!(ok);
    assert_eq!(seen.len(), 2);
    assert!(relay_work_holds(
        CHANNEL,
        &blobs(&["x"]),
        seen[1]["work"].as_str().unwrap(),
        18
    ));
}

#[test]
fn a_second_refusal_stands_and_an_ask_past_40_bits_is_not_worked_for() {
    let (ok, seen) = pushed(vec![18, 40]);
    assert!(!ok);
    assert_eq!(seen.len(), 2);
    let (ok, seen) = pushed(vec![41]);
    assert!(!ok);
    assert_eq!(seen.len(), 1);
}
