//! §15.5's pages, from the client's side: a fetch reads every page, and an
//! answer that does not follow on from the one before is refused.

use std::cell::RefCell;

use splitz_host::{HostError, HttpSplitsRelay, HttpTransport, SplitsRelay};

const CHANNEL: &str = "0000000000000000000000000000000000000000000000000000000000000000";

/// A relay whose answers `answer` writes from the page asked for, recording
/// each URL.
struct Pages<F: Fn(u64) -> String> {
    answer: F,
    urls: RefCell<Vec<String>>,
}

impl<F: Fn(u64) -> String> HttpTransport for Pages<F> {
    fn post(&self, _url: &str, _body: &str) -> Result<String, String> {
        Ok(r#"{"ok":true}"#.to_owned())
    }

    fn get(&self, url: &str) -> Result<String, String> {
        self.urls.borrow_mut().push(url.to_owned());
        let from = url.rsplit("?from=").next().unwrap().parse().unwrap();
        Ok((self.answer)(from))
    }
}

fn pages<F: Fn(u64) -> String>(answer: F) -> Pages<F> {
    Pages {
        answer,
        urls: RefCell::new(Vec::new()),
    }
}

/// §15.5's page of `blobs` from `start`, at `page_bytes`.
fn page(blobs: &[String], start: u64, page_bytes: usize) -> String {
    let mut taken = Vec::new();
    let mut size = 0;
    let mut end = start as usize;
    while end < blobs.len() {
        let cost = blobs[end].len() + 3;
        if !taken.is_empty() && size + cost > page_bytes {
            break;
        }
        taken.push(blobs[end].clone());
        size += cost;
        end += 1;
    }
    if end < blobs.len() {
        serde_json::json!({ "blobs": taken, "next": end }).to_string()
    } else {
        serde_json::json!({ "blobs": taken }).to_string()
    }
}

fn transient(result: Result<impl std::fmt::Debug, HostError>) -> bool {
    matches!(
        result,
        Err(HostError::Relay {
            transient: true,
            ..
        })
    )
}

#[test]
fn a_page_is_addressed_by_its_first_blob() {
    assert_eq!(
        HttpSplitsRelay::page_url("https://relay.example", CHANNEL, 127).unwrap(),
        format!("https://relay.example/c/{CHANNEL}?from=127")
    );
    assert!(matches!(
        HttpSplitsRelay::page_url("https://relay.example?x=1", CHANNEL, 0),
        Err(HostError::Relay {
            transient: false,
            ..
        })
    ));
}

#[test]
fn a_fetch_reads_every_page_in_order() {
    let blobs: Vec<String> = (0..9).map(|i| format!("blob{i}").repeat(i + 1)).collect();
    let server = pages(|from| page(&blobs, from, 40));
    let relay = HttpSplitsRelay::new("https://relay.example", &server).unwrap();
    assert_eq!(relay.fetch(CHANNEL).unwrap(), blobs);
    let urls = server.urls.borrow();
    assert!(urls.len() > 2, "{} page(s)", urls.len());
    assert!(urls[0].ends_with("?from=0"));
}

#[test]
fn a_relay_that_ignores_the_query_is_read_in_one_answer() {
    let server = pages(|_| r#"{"blobs":["a","b","c"]}"#.to_owned());
    let relay = HttpSplitsRelay::new("https://relay.example", &server).unwrap();
    assert_eq!(relay.fetch(CHANNEL).unwrap(), ["a", "b", "c"]);
    assert_eq!(server.urls.borrow().len(), 1);
}

#[test]
fn an_answer_that_does_not_follow_on_is_refused() {
    for answer in [
        r#"{"blobs":["a"],"next":5}"#,
        r#"{"blobs":["a"],"next":0}"#,
        r#"{"blobs":[],"next":0}"#,
        r#"{"blobs":["a"],"next":"1"}"#,
        r#"{"blobs":["a"],"next":1.0}"#,
        r#"{"blobs":["a"],"next":-1}"#,
    ] {
        assert!(
            transient(HttpSplitsRelay::fetch_page(answer, 0)),
            "answer {answer}"
        );
    }
    // `from` plus the page would not fit a u64.
    assert!(transient(HttpSplitsRelay::fetch_page(
        r#"{"blobs":["a"],"next":0}"#,
        u64::MAX
    )));
}

#[test]
fn the_last_page_and_a_page_past_the_end_carry_no_next() {
    let last = HttpSplitsRelay::fetch_page(r#"{"blobs":["c"]}"#, 2).unwrap();
    assert_eq!(last.blobs, ["c"]);
    assert_eq!(last.next, None);
    let past = HttpSplitsRelay::fetch_page(r#"{"blobs":[]}"#, 9).unwrap();
    assert!(past.blobs.is_empty());
    assert_eq!(past.next, None);
}

#[test]
fn a_page_that_follows_on_is_read_and_a_non_string_dropped() {
    let page = HttpSplitsRelay::fetch_page(r#"{"blobs":["a",7,"b"],"next":13}"#, 10).unwrap();
    assert_eq!(page.blobs, ["a", "b"]);
    assert_eq!(page.next, Some(13));
}

#[test]
fn a_relay_that_never_stops_answering_pages_is_refused() {
    let server = pages(|from| format!(r#"{{"blobs":["x{from}"],"next":{}}}"#, from + 1));
    let relay = HttpSplitsRelay::new("https://relay.example", &server).unwrap();
    assert!(transient(relay.fetch(CHANNEL)));
    assert_eq!(
        server.urls.borrow().len(),
        HttpSplitsRelay::MAX_PAGES as usize
    );
}

#[test]
fn a_channel_of_the_most_pages_a_relay_holds_is_read_whole() {
    let last = u64::from(HttpSplitsRelay::MAX_PAGES) - 1;
    let server = pages(|from| {
        if from < last {
            format!(r#"{{"blobs":["x{from}"],"next":{}}}"#, from + 1)
        } else {
            format!(r#"{{"blobs":["x{from}"]}}"#)
        }
    });
    let relay = HttpSplitsRelay::new("https://relay.example", &server).unwrap();
    assert_eq!(
        relay.fetch(CHANNEL).unwrap().len(),
        HttpSplitsRelay::MAX_PAGES as usize
    );
}
