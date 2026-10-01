//! The optional transport (SPEC.md §15.5).

use std::cell::RefCell;

use splitz_host::{
    channel_for_bill, HostError, HttpSplitsRelay, HttpTransport, InMemorySplitsRelay, SplitsRelay,
    UnconfiguredSplitsRelay,
};

/// A relay server that answers from memory and records what it was asked.
#[derive(Default)]
struct FakeServer {
    channels: RefCell<Vec<(String, Vec<String>)>>,
    urls: RefCell<Vec<String>>,
    unreachable: bool,
    answer: Option<String>,
}

impl FakeServer {
    fn channel_of(url: &str) -> String {
        url.rsplit("/c/").next().unwrap_or_default().to_owned()
    }
}

impl HttpTransport for FakeServer {
    fn post(&self, url: &str, body: &str) -> Result<String, String> {
        self.urls.borrow_mut().push(url.to_owned());
        if self.unreachable {
            return Err("connection refused".to_owned());
        }
        if let Some(answer) = &self.answer {
            return Ok(answer.clone());
        }
        let incoming: serde_json::Value = serde_json::from_str(body).unwrap();
        let channel = Self::channel_of(url);
        let mut held = self.channels.borrow_mut();
        if held.iter().all(|(name, _)| *name != channel) {
            held.push((channel.clone(), Vec::new()));
        }
        let slot = held.iter_mut().find(|(name, _)| *name == channel).unwrap();
        for blob in incoming["blobs"].as_array().unwrap() {
            let blob = blob.as_str().unwrap().to_owned();
            if !slot.1.contains(&blob) {
                slot.1.push(blob);
            }
        }
        Ok(r#"{"ok":true}"#.to_owned())
    }

    fn get(&self, url: &str) -> Result<String, String> {
        self.urls.borrow_mut().push(url.to_owned());
        if self.unreachable {
            return Err("connection refused".to_owned());
        }
        if let Some(answer) = &self.answer {
            return Ok(answer.clone());
        }
        let channel = Self::channel_of(url);
        let held = self.channels.borrow();
        let blobs = held
            .iter()
            .find(|(name, _)| *name == channel)
            .map(|(_, b)| b.clone())
            .unwrap_or_default();
        Ok(serde_json::json!({ "blobs": blobs }).to_string())
    }
}

#[test]
fn the_relay_never_sees_the_bill_id_only_its_hash() {
    let server = FakeServer::default();
    let relay = HttpSplitsRelay::new("https://relay.example", &server).unwrap();
    let bill_id = "a-bill-anyone-who-scanned-the-code-knows";
    let channel = channel_for_bill(bill_id);
    relay.push(&channel, &["blob-1".to_owned()]).unwrap();
    let asked = server.urls.borrow().join(" ");
    assert!(
        !asked.contains(bill_id),
        "the id reached the relay: {asked}"
    );
    assert!(asked.contains(&channel));
    assert_eq!(channel.len(), 64, "a SHA-256 in hex");
}

#[test]
fn pushing_the_same_blob_twice_stores_one_copy() {
    // A retry after a dropped connection must not leave a channel with two of
    // everything; §15.5 makes push idempotent.
    let server = FakeServer::default();
    let relay = HttpSplitsRelay::new("https://relay.example", &server).unwrap();
    relay.push("ch", &["a".to_owned(), "b".to_owned()]).unwrap();
    relay.push("ch", &["a".to_owned(), "b".to_owned()]).unwrap();
    assert_eq!(
        relay.fetch("ch").unwrap(),
        vec!["a".to_owned(), "b".to_owned()]
    );
}

#[test]
fn a_relay_that_is_not_there_is_reported_not_swallowed() {
    let server = FakeServer {
        unreachable: true,
        ..Default::default()
    };
    let relay = HttpSplitsRelay::new("https://relay.example", &server).unwrap();
    match relay.push("ch", &["a".to_owned()]) {
        Err(HostError::Relay { message, transient }) => {
            assert!(message.contains("Could not reach the relay"));
            assert!(transient, "a connection may come back");
        }
        other => panic!("expected a relay refusal, got {other:?}"),
    }
    assert!(relay.fetch("ch").is_err());
}

#[test]
fn an_answer_that_is_not_a_channel_is_refused() {
    for answer in ["not json", "[]", "{}", r#"{"blobs":"nope"}"#] {
        let server = FakeServer {
            answer: Some(answer.to_owned()),
            ..Default::default()
        };
        let relay = HttpSplitsRelay::new("https://relay.example", &server).unwrap();
        assert!(relay.fetch("ch").is_err(), "answer {answer:?}");
    }
}

#[test]
fn a_push_past_one_body_is_split_into_bodies_that_each_fit() {
    // 600 blobs at the per-blob cap are about 39 MB of JSON: past one body.
    let blobs: Vec<String> = (0..600)
        .map(|i| format!("{i:05}{}", "b".repeat(HttpSplitsRelay::MAX_BLOB_CHARS - 5)))
        .collect();
    let bodies = HttpSplitsRelay::push_bodies(&blobs).unwrap();
    assert!(bodies.len() > 1);
    let mut carried = Vec::new();
    for body in &bodies {
        assert!(body.len() <= HttpSplitsRelay::MAX_BODY_BYTES);
        let v: serde_json::Value = serde_json::from_str(body).unwrap();
        for b in v["blobs"].as_array().unwrap() {
            carried.push(b.as_str().unwrap().to_owned());
        }
    }
    assert_eq!(carried, blobs);
    assert!(HttpSplitsRelay::push_bodies(&[]).unwrap().is_empty());
}

#[test]
fn a_blob_over_the_cap_is_refused_before_it_is_sent() {
    let server = FakeServer::default();
    let relay = HttpSplitsRelay::new("https://relay.example", &server).unwrap();
    let too_big = "A".repeat(HttpSplitsRelay::MAX_BLOB_CHARS + 1);
    match relay.push("ch", &[too_big]) {
        Err(HostError::Relay { transient, .. }) => {
            assert!(!transient, "a blob that is too big stays too big")
        }
        other => panic!("expected a refusal, got {other:?}"),
    }
    assert!(server.urls.borrow().is_empty(), "nothing was sent");
}

#[test]
fn a_channel_nobody_has_pushed_to_is_empty_not_an_error() {
    let server = FakeServer::default();
    let relay = HttpSplitsRelay::new("https://relay.example", &server).unwrap();
    assert!(relay.fetch("ch").unwrap().is_empty());
}

#[test]
fn an_origin_carrying_a_query_or_a_fragment_is_refused() {
    // The channel is appended to the path. An origin carrying either would put
    // it after them, addressing something else entirely.
    let server = FakeServer::default();
    for origin in ["https://relay.example?t=1", "https://relay.example#top"] {
        assert!(
            HttpSplitsRelay::new(origin, &server).is_err(),
            "origin {origin}"
        );
    }
    assert!(HttpSplitsRelay::new("https://relay.example/base", &server).is_ok());
}

#[test]
fn a_build_with_no_relay_says_so_rather_than_doing_nothing() {
    let relay = UnconfiguredSplitsRelay;
    for outcome in [
        relay.push("ch", &["a".to_owned()]).err(),
        relay.fetch("ch").err(),
    ] {
        match outcome {
            Some(HostError::Relay { message, transient }) => {
                assert!(message.contains("no bill relay"));
                assert!(!transient, "no retry will configure one");
            }
            other => panic!("expected a refusal, got {other:?}"),
        }
    }
}

#[test]
fn the_in_memory_relay_keeps_insertion_order_and_no_duplicates() {
    let relay = InMemorySplitsRelay::default();
    relay.push("ch", &["b".to_owned(), "a".to_owned()]).unwrap();
    relay.push("ch", &["b".to_owned(), "c".to_owned()]).unwrap();
    assert_eq!(
        relay.fetch("ch").unwrap(),
        vec!["b".to_owned(), "a".to_owned(), "c".to_owned()]
    );
    assert!(relay.fetch("other").unwrap().is_empty());
}

#[test]
fn the_wire_a_foreign_client_speaks_is_the_one_this_client_speaks() {
    assert_eq!(
        HttpSplitsRelay::channel_url("https://relay.example/base", "ch").unwrap(),
        "https://relay.example/base/c/ch"
    );
    assert!(matches!(
        HttpSplitsRelay::channel_url("https://relay.example?t=1", "ch"),
        Err(HostError::Relay {
            transient: false,
            ..
        })
    ));
    assert_eq!(HttpSplitsRelay::push_body(&[]).unwrap(), None);
    assert_eq!(
        HttpSplitsRelay::push_body(&["a".to_owned(), "b".to_owned()]).unwrap(),
        Some(r#"{"blobs":["a","b"]}"#.to_owned())
    );
    // Bodies tools/relay/server.py answers with, a refusal's status aside.
    assert!(HttpSplitsRelay::push_answer(r#"{"ok": true}"#).is_ok());
    for refusal in [
        r#"{"error": "not a channel"}"#,
        r#"{"error": "the relay is full"}"#,
        "",
    ] {
        assert!(
            matches!(
                HttpSplitsRelay::push_answer(refusal),
                Err(HostError::Relay {
                    transient: true,
                    ..
                })
            ),
            "answer {refusal:?}"
        );
    }
    assert_eq!(
        HttpSplitsRelay::fetch_answer(r#"{"blobs": ["a", 7, "b"]}"#).unwrap(),
        vec!["a".to_owned(), "b".to_owned()]
    );
}
