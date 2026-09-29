//! Checks this crate's payment request URIs against librustzcash.
//!
//! Every other lane compares implementations written from one specification by
//! one author. They can all be wrong together: if this reading of section 8.2's
//! parameter order or section 8.3's `qchar` set is mistaken, three
//! implementations share the mistake and every lane stays green.
//!
//! This lane compares against the canonical implementation instead. The crate
//! is stock from crates.io, not a fork, so the check is reproducible by
//! anybody. It is a dev-dependency: a consumer of `splitz-core` still takes one
//! dependency, `serde_json`.
//!
//! Note that `zip321` parses an address into a `ZcashAddress`, so every
//! address in the corpus has to be a real one. Filler would fail here for a
//! reason that has nothing to do with the URI.

use serde_json::Value;
use std::fs;
use zip321::{Payment, TransactionRequest};

/// Panics when the corpus is not on disk.
///
/// Returning early instead would report `ok` for a test that asserted
/// nothing, and `cargo test` captures stdout on a pass, so the notice would
/// never be read.
fn require_corpus() {
    let dir = std::env::var("SPLITZ_VECTORS")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../vectors").to_owned());
    assert!(
        std::path::Path::new(&dir).is_dir(),
        "no corpus at {dir}. Point SPLITZ_VECTORS at a checkout to run the \
         oracle."
    );
}

fn load(name: &str) -> Value {
    // The corpus lives at the repository root, two levels above this package;
    // SPLITZ_VECTORS points at a checkout when the crate is consumed on its own.
    let dir = std::env::var("SPLITZ_VECTORS")
        .unwrap_or_else(|_| concat!(env!("CARGO_MANIFEST_DIR"), "/../../vectors").to_owned());
    let path = format!("{dir}/{name}");
    serde_json::from_str(&fs::read_to_string(&path).expect("vectors are readable"))
        .expect("vectors are JSON")
}

/// The zatoshi amounts the corpus says a case's payments carry, in order.
fn expected_zatoshi(case: &Value) -> Vec<i64> {
    case["payments"]
        .as_array()
        .map(|a| a.iter().filter_map(|p| p["zatoshi"].as_i64()).collect())
        .unwrap_or_default()
}

/// Every URI the corpus says this crate produces must parse in librustzcash,
/// and must parse back to the same recipients and the same amounts.
#[test]
fn zip321_uris_round_trip_through_librustzcash() {
    require_corpus();
    let doc = load("zip321.json");
    let mut checked = 0;
    let mut skipped_bulk = 0;
    let mut failures = Vec::new();

    for case in doc["cases"].as_array().expect("cases is a list") {
        let name = case["name"].as_str().unwrap_or("<unnamed>");
        let Some(uri) = case["expect"].as_str() else {
            // A refusal, or one of the two bulk cases carried as a count.
            if case.get("expectLength").is_some() {
                skipped_bulk += 1;
            }
            continue;
        };

        let parsed = match TransactionRequest::from_uri(uri) {
            Ok(r) => r,
            Err(e) => {
                failures.push(format!("{name}: librustzcash refuses our URI: {e:?}"));
                continue;
            }
        };

        let want = expected_zatoshi(case);
        let got: Vec<i64> = parsed
            .payments()
            .values()
            .map(|p| p.amount().map(u64::from).unwrap_or(0) as i64)
            .collect();
        if got != want {
            failures.push(format!("{name}: amounts {got:?}, corpus says {want:?}"));
            continue;
        }

        let addresses: Vec<String> = parsed
            .payments()
            .values()
            .map(|p| p.recipient_address().encode())
            .collect();
        let want_addresses: Vec<String> = case["payments"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|p| p["address"].as_str().unwrap_or_default().to_owned())
                    .collect()
            })
            .unwrap_or_default();
        if addresses != want_addresses {
            failures.push(format!("{name}: recipients differ"));
            continue;
        }

        checked += 1;
    }

    assert!(
        failures.is_empty(),
        "{} of {} URIs diverge from librustzcash\n\n{}",
        failures.len(),
        checked + failures.len(),
        failures.join("\n")
    );
    assert!(
        checked >= 10,
        "only {checked} URIs were checked against the oracle; the lane is not \
         exercising anything"
    );
    println!("{checked} URIs round-trip through librustzcash ({skipped_bulk} bulk cases skipped)");
}

/// Section 8.4 claims a parser predating `fiat` treats it as an unrecognised
/// parameter, ignores it, and constructs exactly the payment `amount`
/// specifies. `fiat` is proposed, not released, so the stock crate is exactly
/// such a parser and can settle the claim.
#[test]
fn a_parser_predating_fiat_ignores_it() {
    require_corpus();
    let doc = load("zip321.json");
    let mut checked = 0;

    for case in doc["cases"].as_array().expect("cases is a list") {
        if case["includeFiat"].as_bool() != Some(true) {
            continue;
        }
        let Some(uri) = case["expect"].as_str() else {
            continue;
        };
        assert!(uri.contains("fiat"), "{}: no fiat to ignore", case["name"]);

        let parsed = TransactionRequest::from_uri(uri).unwrap_or_else(|e| {
            panic!(
                "{}: a parser predating fiat refused the URI: {e:?}",
                case["name"]
            )
        });
        let got: Vec<i64> = parsed
            .payments()
            .values()
            .map(|p| p.amount().map(u64::from).unwrap_or(0) as i64)
            .collect();
        assert_eq!(
            got,
            expected_zatoshi(case),
            "{}: fiat changed what the parser built",
            case["name"]
        );
        checked += 1;
    }

    assert!(checked > 0, "no fiat-bearing case was checked");
    println!("{checked} fiat-bearing URIs parse unchanged by a parser that predates fiat");
}

/// The obligations corpus renders whole payer obligations, several of them
/// multi-output. Those URIs go through the oracle too.
#[test]
fn obligation_uris_round_trip_through_librustzcash() {
    require_corpus();
    let doc = load("obligations.json");
    let mut checked = 0;
    let mut failures = Vec::new();

    for case in doc["cases"].as_array().expect("cases is a list") {
        let Some(uri) = case["expect"]["uri"].as_str() else {
            continue;
        };
        match TransactionRequest::from_uri(uri) {
            Err(e) => failures.push(format!("{}: {e:?}", case["name"])),
            Ok(parsed) => {
                let got: Vec<i64> = parsed
                    .payments()
                    .values()
                    .map(|p| p.amount().map(u64::from).unwrap_or(0) as i64)
                    .collect();
                let want = expected_zatoshi(&case["expect"]);
                if got != want {
                    failures.push(format!(
                        "{}: amounts {got:?}, corpus says {want:?}",
                        case["name"]
                    ));
                } else {
                    checked += 1;
                }
            }
        }
    }

    assert!(failures.is_empty(), "{failures:#?}");
    assert!(checked > 0, "no obligation URI was checked");
    println!("{checked} obligation URIs round-trip through librustzcash");
}

/// Renders the same payment twice — once with this crate, once with
/// librustzcash — and compares the two URIs byte for byte.
///
/// This crate does the rendering, not the corpus: comparing the corpus against
/// librustzcash would leave a defect in this crate's own renderer invisible
/// here, caught only transitively by the conformance lane. A mistaken reading
/// of §8.2's parameter order parses cleanly and is exactly what this catches.
///
/// Only the cases librustzcash can express are compared. It has no `fiat`
/// parameter — that is proposed, not released — so a case emitting one cannot
/// be built on their side, and `a_parser_predating_fiat_ignores_it` covers
/// those instead.
#[test]
fn rendering_matches_librustzcash_byte_for_byte() {
    require_corpus();
    let doc = load("zip321.json");
    let mut checked = 0;
    let mut unbuildable = 0;
    let mut failures = Vec::new();

    for case in doc["cases"].as_array().expect("cases is a list") {
        let name = case["name"].as_str().unwrap_or("<unnamed>");
        let Some(ours) = case["expect"].as_str() else {
            continue;
        };
        if case["includeFiat"].as_bool() == Some(true) || case.get("paymentCount").is_some() {
            unbuildable += 1;
            continue;
        }

        let mut payments = Vec::new();
        let mut buildable = true;
        for raw in case["payments"].as_array().cloned().unwrap_or_default() {
            let Ok(address) = raw["address"]
                .as_str()
                .unwrap_or_default()
                .parse::<zcash_address::ZcashAddress>()
            else {
                buildable = false;
                break;
            };
            let Ok(amount) =
                zcash_protocol::value::Zatoshis::from_u64(raw["zatoshi"].as_u64().unwrap_or(0))
            else {
                buildable = false;
                break;
            };
            let memo = raw["memo"]
                .as_str()
                .and_then(|m| zcash_protocol::memo::MemoBytes::from_bytes(m.as_bytes()).ok());
            // Our label is cut to 96 UTF-8 bytes on a character boundary
            // (§8.3); theirs is not, so the input has to be pre-cut for the
            // comparison to be about rendering rather than about truncation.
            let label = raw["label"].as_str().map(|l| {
                let mut end = l.len().min(96);
                while end > 0 && !l.is_char_boundary(end) {
                    end -= 1;
                }
                l[..end].to_owned()
            });
            let message = raw["message"].as_str().map(str::to_owned);
            match Payment::new(address, Some(amount), memo, label, message, vec![]) {
                Ok(p) => payments.push(p),
                Err(_) => {
                    buildable = false;
                    break;
                }
            }
        }
        if !buildable || payments.is_empty() {
            unbuildable += 1;
            continue;
        }

        let Ok(theirs) = TransactionRequest::new(payments) else {
            unbuildable += 1;
            continue;
        };

        // Ours, rendered here rather than read from the corpus.
        let mine: Vec<splitz_core::Zip321Payment> = case["payments"]
            .as_array()
            .cloned()
            .unwrap_or_default()
            .iter()
            .map(|raw| splitz_core::Zip321Payment {
                address: raw["address"].as_str().unwrap_or_default().to_owned(),
                zatoshi: raw["zatoshi"].as_i64().unwrap_or(0),
                memo: raw["memo"].as_str().map(|m| m.as_bytes().to_vec()),
                label: raw["label"].as_str().map(str::to_owned),
                message: raw["message"].as_str().map(str::to_owned),
                ..Default::default()
            })
            .collect();
        let Ok(rendered_by_us) = splitz_core::render_uri(&mine, false) else {
            unbuildable += 1;
            continue;
        };

        let rendered = theirs.to_uri();
        if rendered != rendered_by_us {
            failures.push(format!(
                "{name}:\n  ours   {rendered_by_us}\n  theirs {rendered}"
            ));
        } else if rendered_by_us != ours {
            failures.push(format!(
                "{name}: this crate and the corpus disagree\n  crate  {rendered_by_us}\n  corpus {ours}"
            ));
        } else {
            checked += 1;
        }
    }

    assert!(
        failures.is_empty(),
        "{} of {} URIs differ from librustzcash byte for byte\n\n{}",
        failures.len(),
        checked + failures.len(),
        failures.join("\n\n")
    );
    assert!(
        checked >= 5,
        "only {checked} URIs were compared byte for byte"
    );
    println!(
        "{checked} URIs are byte-identical to librustzcash ({unbuildable} not expressible there)"
    );
}

/// Every address the corpus carries decodes as a mainnet Unified Address.
///
/// §8.3 checks only what the ZIP 321 grammar admits and says outright that a
/// wallet MUST put every address through its own decoder (ZIP 316). Nothing in
/// this repository decodes one, so that clause had no external check: filler
/// that merely looks like an address would satisfy every other lane and fail
/// the first wallet to run the corpus.
///
/// `zcash_address` is the canonical decoder and is already a dev-dependency.
#[test]
fn corpus_addresses_are_real_unified_addresses() {
    require_corpus();
    use std::collections::BTreeSet;

    // Every distinct address in every vector file, taken from the files rather
    // than from a list here, so a new one cannot be added without decoding.
    let mut addresses: BTreeSet<String> = BTreeSet::new();
    fn walk(v: &Value, out: &mut BTreeSet<String>) {
        match v {
            Value::String(s) if s.starts_with("u1") && s.len() > 20 => {
                out.insert(s.clone());
            }
            Value::Array(a) => a.iter().for_each(|x| walk(x, out)),
            Value::Object(o) => o.values().for_each(|x| walk(x, out)),
            _ => {}
        }
    }
    // Only from cases the corpus accepts. A refusal case deliberately carries
    // a corrupted address — one has a `-` substituted to exercise
    // `zip321_bad_address` — and that is a fixture, not an address this
    // protocol would ever emit.
    for name in [
        "zip321.json",
        "obligations.json",
        "bill-json.json",
        "balances.json",
        "log.json",
        "coverage.json",
    ] {
        for case in load(name)["cases"].as_array().expect("cases is a list") {
            if case.get("error").is_none() {
                walk(case, &mut addresses);
            }
        }
    }

    assert!(
        addresses.len() >= 5,
        "only {} addresses found; the walk is not reaching the corpus",
        addresses.len()
    );

    let mut failures = Vec::new();
    for a in &addresses {
        match zcash_address::ZcashAddress::try_from_encoded(a) {
            Err(e) => failures.push(format!("{a}: does not decode: {e}")),
            Ok(parsed) => {
                // The `u1` human-readable part is ZIP 316's mainnet Unified
                // Address prefix; testnet is `utest1` and regtest `uregtest1`,
                // and the decoder will not accept one under the other's HRP.
                if parsed.encode() != *a {
                    failures.push(format!("{a}: does not round-trip through the decoder"));
                } else if !parsed.can_receive_memo() {
                    // §8.3: ZIP 321 refuses a URI carrying a memo at the same
                    // index as an address that cannot hold one, and takes the
                    // unrelated shielded outputs with it. An address in this
                    // corpus must be able to carry the memos the corpus sets.
                    failures.push(format!("{a}: decodes, but cannot receive a memo"));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
    println!(
        "{} corpus addresses decode as mainnet Unified Addresses",
        addresses.len()
    );
}

/// Section 8.7 reads back only what section 8.2 writes. Every request it
/// accepts must mean the same payments to librustzcash: the same recipients,
/// amounts, labels, messages and memos, in the same order.
#[test]
fn requests_read_back_as_librustzcash_reads_them() {
    require_corpus();
    let doc = load("request.json");
    let mut checked = 0;
    let mut failures = Vec::new();
    for case in doc["cases"].as_array().expect("cases is a list") {
        let name = case["name"].as_str().unwrap_or("<unnamed>");
        let (Some(uri), Some(want)) = (case["uri"].as_str(), case["expect"].as_array()) else {
            continue;
        };
        let parsed = match TransactionRequest::from_uri(uri) {
            Ok(r) => r,
            Err(e) => {
                failures.push(format!("{name}: librustzcash refuses it: {e:?}"));
                continue;
            }
        };
        let got: Vec<Value> = parsed
            .payments()
            .values()
            .map(|p: &Payment| {
                let mut o = serde_json::Map::new();
                o.insert("address".into(), p.recipient_address().encode().into());
                o.insert(
                    "zatoshi".into(),
                    (p.amount().map(u64::from).unwrap_or(0) as i64).into(),
                );
                if let Some(m) = p.memo() {
                    let raw = m.as_slice();
                    let end = raw.iter().rposition(|b| *b != 0).map_or(0, |i| i + 1);
                    o.insert(
                        "memo".into(),
                        splitz_core::zip321::base64url(&raw[..end]).into(),
                    );
                }
                if let Some(l) = p.label() {
                    o.insert("label".into(), l.clone().into());
                }
                if let Some(m) = p.message() {
                    o.insert("message".into(), m.clone().into());
                }
                Value::Object(o)
            })
            .collect();
        // `fiat` is this protocol's parameter; librustzcash ignores it.
        let want: Vec<Value> = want
            .iter()
            .map(|p| {
                let mut p = p.as_object().expect("a payment").clone();
                p.remove("fiat");
                Value::Object(p)
            })
            .collect();
        if got != want {
            failures.push(format!(
                "{name}: librustzcash reads {got:?}, the corpus {want:?}"
            ));
            continue;
        }
        checked += 1;
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        checked >= 8,
        "only {checked} requests were checked against the oracle"
    );
    println!("{checked} requests read back as librustzcash reads them");
}

/// What `zcash_address` says an address is, in the corpus's own terms.
struct Oracle(Value);

impl zcash_address::TryFromAddress for Oracle {
    type Error = ();

    fn try_from_sprout(
        net: zcash_protocol::consensus::NetworkType,
        _: [u8; 64],
    ) -> Result<Self, zcash_address::ConversionError<()>> {
        Ok(Oracle(oracle_answer(net, "sprout", vec![], true)))
    }

    fn try_from_sapling(
        net: zcash_protocol::consensus::NetworkType,
        _: [u8; 43],
    ) -> Result<Self, zcash_address::ConversionError<()>> {
        Ok(Oracle(oracle_answer(net, "sapling", vec![], true)))
    }

    fn try_from_unified(
        net: zcash_protocol::consensus::NetworkType,
        ua: zcash_address::unified::Address,
    ) -> Result<Self, zcash_address::ConversionError<()>> {
        use zcash_address::unified::{Container, Receiver};
        let codes = ua
            .items_as_parsed()
            .iter()
            .map(|r| match r {
                Receiver::P2pkh(_) => 0,
                Receiver::P2sh(_) => 1,
                Receiver::Sapling(_) => 2,
                Receiver::Orchard(_) => 3,
                Receiver::Unknown { typecode, .. } => *typecode,
            })
            .collect();
        Ok(Oracle(oracle_answer(
            net,
            "unified",
            codes,
            ua.can_receive_memo(),
        )))
    }

    fn try_from_transparent_p2pkh(
        net: zcash_protocol::consensus::NetworkType,
        _: [u8; 20],
    ) -> Result<Self, zcash_address::ConversionError<()>> {
        Ok(Oracle(oracle_answer(net, "p2pkh", vec![], false)))
    }

    fn try_from_transparent_p2sh(
        net: zcash_protocol::consensus::NetworkType,
        _: [u8; 20],
    ) -> Result<Self, zcash_address::ConversionError<()>> {
        Ok(Oracle(oracle_answer(net, "p2sh", vec![], false)))
    }

    fn try_from_tex(
        net: zcash_protocol::consensus::NetworkType,
        _: [u8; 20],
    ) -> Result<Self, zcash_address::ConversionError<()>> {
        Ok(Oracle(oracle_answer(net, "tex", vec![], false)))
    }
}

fn oracle_answer(
    net: zcash_protocol::consensus::NetworkType,
    kind: &str,
    receivers: Vec<u32>,
    memo: bool,
) -> Value {
    use zcash_protocol::consensus::NetworkType;
    let network = match net {
        NetworkType::Main => "main",
        NetworkType::Test => "test",
        NetworkType::Regtest => "regtest",
    };
    serde_json::json!({
        "network": network,
        "kind": kind,
        "receivers": receivers,
        "canReceiveMemo": memo,
    })
}

/// Every case in `address.json` against `zcash_address`, the decoder
/// librustzcash wallets use, written independently of this crate.
///
/// Where §8.6 is stricter than that crate, the case is named here with the
/// rule that separates them, and the test asserts the crate really does
/// accept it: an entry that stops diverging fails rather than lingering.
#[test]
fn addresses_agree_with_zcash_address() {
    require_corpus();
    // Case name -> the §8.6 rule `zcash_address` 0.13 does not apply.
    const STRICTER: &[(&str, &str)] = &[
        (
            "a_leading_space",
            "the text is taken exactly; nothing is trimmed",
        ),
        (
            "a_trailing_newline",
            "the text is taken exactly; nothing is trimmed",
        ),
        (
            "sapling_padding_bits_not_zero",
            "ZIP 173: leftover bits are zero",
        ),
        (
            "unified_padding_bits_not_zero",
            "ZIP 173: leftover bits are zero",
        ),
        (
            "sapling_with_a_whole_extra_group",
            "ZIP 173: at most four leftover bits",
        ),
        (
            "unified_with_only_an_unknown_typecode",
            "ZIP 316: a revision 0 UA carries typecode 0x02 or 0x03",
        ),
        (
            "unified_with_p2pkh_and_an_unknown_typecode",
            "ZIP 316: a revision 0 UA carries typecode 0x02 or 0x03",
        ),
        (
            "unified_with_a_must_understand_typecode",
            "ZIP 316: a revision 0 UA carries no typecode in 0xE0..=0xFC",
        ),
        (
            "unified_with_the_last_must_understand_typecode",
            "ZIP 316: a revision 0 UA carries no typecode in 0xE0..=0xFC",
        ),
        ("a_sprout_address_main", "ZIP 211: Sprout receives no funds"),
        ("a_sprout_address_test", "ZIP 211: Sprout receives no funds"),
    ];

    let doc = load("address.json");
    let cases = doc["cases"].as_array().expect("cases is a list");
    let mut failures = Vec::new();
    let mut agreed = 0;
    for case in cases {
        let name = case["name"].as_str().unwrap_or("<unnamed>");
        let text = case["address"].as_str().expect("address is text");
        let oracle = zcash_address::ZcashAddress::try_from_encoded(text)
            .ok()
            .map(|a| a.convert::<Oracle>().expect("every kind converts").0);
        let stricter = STRICTER.iter().find(|(n, _)| *n == name);
        match (case.get("expect"), oracle, stricter) {
            (Some(want), Some(got), None) if *want == got => agreed += 1,
            (None, None, None) => agreed += 1,
            (None, Some(_), Some(_)) => {}
            (_, got, Some((_, rule))) => failures.push(format!(
                "{name}: listed as stricter ({rule}) but zcash_address answers {got:?}"
            )),
            (want, got, None) => failures.push(format!(
                "{name}: corpus {}, zcash_address {got:?}",
                want.map_or("refuses".to_owned(), Value::to_string)
            )),
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_eq!(agreed + STRICTER.len(), cases.len());
    println!(
        "{agreed} of {} address cases agree with zcash_address; {} are refused by the stricter rules of §8.6",
        cases.len(),
        STRICTER.len()
    );
}

/// §8.3 refuses a memo §8.6 says cannot be delivered, and so does
/// librustzcash: a case the corpus refuses with `zip321_memo_undeliverable`
/// holds a payment `zip321::Payment::new` refuses with `TransparentMemo`, and
/// no other case does.
#[test]
fn undeliverable_memos_are_the_ones_librustzcash_refuses() {
    require_corpus();
    let doc = load("zip321.json");
    let (mut refused, mut built) = (0, 0);
    let mut failures = Vec::new();
    for case in doc["cases"].as_array().expect("cases is a list") {
        let name = case["name"].as_str().unwrap_or("<unnamed>");
        let ours = case["error"].as_str() == Some("zip321_memo_undeliverable");
        let mut theirs = false;
        for raw in case["payments"].as_array().cloned().unwrap_or_default() {
            let Some(memo) = raw["memo"].as_str() else {
                continue;
            };
            let Ok(address) = raw["address"]
                .as_str()
                .unwrap_or_default()
                .parse::<zcash_address::ZcashAddress>()
            else {
                continue;
            };
            let memo = zcash_protocol::memo::MemoBytes::from_bytes(memo.as_bytes()).ok();
            let amount = zcash_protocol::value::Zatoshis::from_u64(7004).ok();
            match Payment::new(address, amount, memo, None, None, vec![]) {
                Err(zip321::PaymentError::TransparentMemo) => {
                    refused += 1;
                    theirs = true;
                }
                Ok(_) => built += 1,
                Err(e) => failures.push(format!("{name}: {e}")),
            }
        }
        if ours != theirs {
            failures.push(format!(
                "{name}: memo_undeliverable here is {ours}, TransparentMemo there is {theirs}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert!(
        refused >= 4 && built >= 3,
        "only {refused} refused and {built} built; the lane is not reaching the rule"
    );
    println!("{refused} memos librustzcash refuses are refused here; {built} it builds are built");
}
