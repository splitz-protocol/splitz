//! §9.1's transparent payout, against every case `vectors/address.json` holds.

use serde_json::Value;
use splitz_core::is_transparent_address;

#[test]
fn an_address_is_transparent_exactly_when_section_8_6_decodes_it_as_one() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../vectors/address.json");
    let doc: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let (mut yes, mut no) = (0, 0);
    for c in doc["cases"].as_array().unwrap() {
        let kind = c["expect"]["kind"].as_str();
        let transparent = matches!(kind, Some("p2pkh" | "p2sh" | "tex"));
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
