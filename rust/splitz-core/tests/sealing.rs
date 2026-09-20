//! §11.3's producing half, against its own reader.
//!
//! The cipher is the host's, so what is asserted here is every value the
//! specification fixes: the plaintext, the nonce derived from it, the frame,
//! and the channel. A wallet deriving any of these a second time is a second
//! place for them to drift, and two devices that drift produce blobs neither
//! can open.

use serde_json::json;

use splitz_core::{
    canonical_json, channel_for, frame_sealed, parse_sealed_frame, sealed_nonce, sealed_plaintext,
    sha256, sha256_hex, NONCE_BYTES, SEALED_VERSION, TAG_BYTES,
};

/// Stands in for the cipher's output: this crate never produces one.
fn fake_body(length: usize) -> Vec<u8> {
    (0..length).map(|i| (i % 251) as u8).collect()
}

#[test]
fn the_plaintext_is_canonical_json_so_two_orderings_seal_identically() {
    let one = json!({"b": 2, "a": 1});
    let other = json!({"a": 1, "b": 2});
    assert_eq!(
        sealed_plaintext(&one).unwrap(),
        sealed_plaintext(&other).unwrap()
    );
    assert_eq!(
        String::from_utf8(sealed_plaintext(&one).unwrap()).unwrap(),
        canonical_json(&one).unwrap()
    );
}

#[test]
fn the_nonce_is_sha256_of_the_plaintext_truncated() {
    let plaintext = b"an entry";
    assert_eq!(sealed_nonce(plaintext).len(), NONCE_BYTES);
    assert_eq!(
        sealed_nonce(plaintext)[..],
        sha256(plaintext)[..NONCE_BYTES]
    );
}

#[test]
fn one_entry_always_seals_under_one_nonce() {
    // Idempotence is what keeps a channel finite: a relay stores the blob once
    // however many times it is pushed.
    let entry = json!({"kind": "joinBill", "n": 1});
    let a = sealed_nonce(&sealed_plaintext(&entry).unwrap());
    let b = sealed_nonce(&sealed_plaintext(&entry).unwrap());
    assert_eq!(a, b);
}

#[test]
fn one_byte_of_difference_gives_a_different_nonce() {
    // The one condition the cipher requires: two different plaintexts never
    // share a nonce.
    assert_ne!(sealed_nonce(b"an entry"), sealed_nonce(b"an entrz"));
}

#[test]
fn what_is_framed_is_what_the_reader_reads_back() {
    let plaintext = sealed_plaintext(&json!({"kind": "joinBill"})).unwrap();
    let nonce = sealed_nonce(&plaintext);
    let body = fake_body(TAG_BYTES + 40);

    let framed = frame_sealed(&nonce, &body).unwrap();
    let read = parse_sealed_frame(&framed).unwrap();

    assert_eq!(read.version, SEALED_VERSION);
    assert_eq!(read.body_bytes, body.len());
}

#[test]
fn a_nonce_of_the_wrong_length_is_refused() {
    let err = frame_sealed(&fake_body(NONCE_BYTES - 1), &fake_body(TAG_BYTES)).unwrap_err();
    assert_eq!(err.code, "sealed_malformed");
}

#[test]
fn a_body_too_short_to_hold_a_tag_is_refused() {
    // The reader refuses such a frame, so producing one would emit a blob
    // nothing can open.
    let err = frame_sealed(&fake_body(NONCE_BYTES), &fake_body(TAG_BYTES - 1)).unwrap_err();
    assert_eq!(err.code, "sealed_malformed");
}

#[test]
fn the_shortest_frame_this_writes_is_one_the_reader_accepts() {
    let framed = frame_sealed(&fake_body(NONCE_BYTES), &fake_body(TAG_BYTES)).unwrap();
    assert_eq!(parse_sealed_frame(&framed).unwrap().body_bytes, TAG_BYTES);
}

#[test]
fn the_channel_is_the_bill_id_digest_not_the_bill_id() {
    // The id is a live address printed in every invite; a relay that only ever
    // sees traffic cannot run the digest backwards.
    let bill_id = "HqA9d4fLlNHBmVZHGH3s6w";
    assert_eq!(channel_for(bill_id), sha256_hex(bill_id.as_bytes()));
    assert!(!channel_for(bill_id).contains(bill_id));
}

#[test]
fn the_channel_is_lower_case_hex_of_64_characters() {
    let channel = channel_for("HqA9d4fLlNHBmVZHGH3s6w");
    assert_eq!(channel.len(), 64);
    assert!(channel
        .chars()
        .all(|c| c.is_ascii_digit() || ('a'..='f').contains(&c)));
}

#[test]
fn every_participant_computes_the_same_channel() {
    assert_eq!(channel_for("bill-1"), channel_for("bill-1"));
    assert_ne!(channel_for("bill-1"), channel_for("bill-2"));
}
