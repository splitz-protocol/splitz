//! §9.1's transparent payout, against every case `vectors/address.json` holds.

use serde_json::Value;
use splitz_core::{is_transparent_address, parse_address};

/// A Unified Address with a P2PKH receiver and one of a typecode §8.6 does
/// not know: refused by §8.6, paid transparently by a wallet that reads it.
const TRANSPARENT_ONLY_UNIFIED: &str = "u1e67lmu7s8etun46sslprau5u5z7v2ffd6p6wwp9xxxga86m9skaslmkptgyxlnewgxvym66jke59fp0np8rhmy0lrrympgy2hm33h0tjkxmg2mc4dcks57s9wm";

/// The vectors §8.6 refuses whose text is still a valid Bech32m string under
/// a Unified Address prefix — a valid BIP 350 checksum, computed apart from
/// this library, over data whose padding bits are zero.
const REFUSED_UNIFIED: [&str; 21] = [
    "an_orchard_receiver_one_byte_short",
    "a_p2pkh_receiver_one_byte_long",
    "unified_with_only_p2pkh",
    "unified_with_only_p2sh",
    "unified_with_only_an_unknown_typecode",
    "unified_with_p2pkh_and_an_unknown_typecode",
    "unified_with_p2pkh_and_p2sh",
    "unified_with_a_duplicate_typecode",
    "unified_with_receivers_out_of_order",
    "unified_with_a_must_understand_typecode",
    "unified_with_the_last_must_understand_typecode",
    "unified_with_a_typecode_past_the_limit",
    "unified_with_a_non_canonical_typecode",
    "unified_with_a_non_canonical_length",
    "unified_with_a_length_past_the_end",
    "unified_with_a_trailing_byte",
    "unified_with_testnet_padding",
    "unified_with_empty_padding",
    "unified_shorter_than_f4jumble_accepts",
    "unified_with_no_items",
    "unified_with_nothing_to_unjumble",
];

#[test]
fn every_vector_is_transparent_when_decoded_so_or_a_refused_unified_address() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../vectors/address.json");
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let (mut yes, mut no) = (0, 0);
    for c in doc["cases"].as_array().unwrap() {
        let kind = c["expect"]["kind"].as_str();
        let name = c["name"].as_str().unwrap();
        let transparent = match kind {
            Some(k) => matches!(k, "p2pkh" | "p2sh" | "tex"),
            None => REFUSED_UNIFIED.contains(&name),
        };
        let address = c["address"].as_str().unwrap();
        assert_eq!(
            is_transparent_address(address),
            transparent,
            "{}",
            c["name"]
        );
        if transparent {
            yes += 1;
        } else {
            no += 1;
        }
    }
    assert!(yes > 0 && no > 0, "{yes} {no}");
}

#[test]
fn a_unified_address_section_8_6_refuses_is_not_presented_as_private() {
    assert!(parse_address(TRANSPARENT_ONLY_UNIFIED).is_err());
    assert!(is_transparent_address(TRANSPARENT_ONLY_UNIFIED));
    // No address at all, and a Unified Address that fails its checksum, are
    // nothing a wallet pays.
    assert!(!is_transparent_address(""));
    assert!(!is_transparent_address("not an address"));
    let mut flipped = TRANSPARENT_ONLY_UNIFIED.to_owned();
    let last = flipped.pop().unwrap();
    flipped.push(if last == 'm' { 'n' } else { 'm' });
    assert!(!is_transparent_address(&flipped));
}
