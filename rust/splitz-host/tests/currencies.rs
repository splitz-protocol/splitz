//! The ISO 4217 register here is the one `splitz_host` ships, entry for entry.

#[test]
fn the_register_is_the_dart_packages() {
    let dart = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../splitz_host/lib/src/currencies.dart"
    ))
    .unwrap();
    let theirs: Vec<(String, u32)> = dart
        .lines()
        .filter_map(|line| {
            let rest = line.strip_prefix("  '")?;
            let (code, exp) = rest.split_once("': ")?;
            Some((code.to_owned(), exp.strip_suffix(',')?.parse().ok()?))
        })
        .collect();
    let ours: Vec<(String, u32)> = splitz_host::ISO_4217_EXPONENTS
        .iter()
        .map(|(c, e)| ((*c).to_owned(), *e))
        .collect();
    assert_eq!(ours.len(), 165);
    assert_eq!(ours, theirs);
    assert_eq!(splitz_host::currency_exponent("KWD"), Some(3));
    assert_eq!(splitz_host::currency_exponent("JPY"), Some(0));
    assert_eq!(splitz_host::currency_exponent("XAU"), None);
}

/// One table, pinned in both host packages: what a typed figure reads as.
const PARSE_CASES: &[(&str, u32, Option<i64>)] = &[
    ("12.34", 2, Some(1234)),
    ("12,34", 2, Some(1234)),
    ("0.29", 2, Some(29)),
    (" 5 ", 2, Some(500)),
    ("\t5\n", 2, Some(500)),
    ("5.", 2, Some(500)),
    (".5", 2, Some(50)),
    ("007", 2, Some(700)),
    ("1,00", 2, Some(100)),
    ("1,000", 3, None),
    ("1,000", 2, None),
    ("1.000", 3, Some(1000)),
    (".", 2, None),
    ("", 2, None),
    ("  ", 2, None),
    ("1.234", 2, None),
    ("-1", 2, None),
    ("+1", 2, None),
    ("1e3", 2, None),
    ("1.2.3", 2, None),
    ("\u{661}\u{662}", 2, None),
    ("\u{a0}5", 2, None),
    ("\u{ff15}", 2, None),
    ("92233720368547758.07", 2, Some(9223372036854775807)),
    ("92233720368547758.08", 2, None),
    ("9223372036854775807", 0, Some(9223372036854775807)),
    ("9223372036854775808", 0, None),
    ("99999999999999999999999", 0, None),
    ("7", 0, Some(7)),
    ("7.", 0, Some(7)),
    ("7.0", 0, None),
    ("1.5", 3, Some(1500)),
    ("0", 18, Some(0)),
    ("1", 18, Some(1000000000000000000)),
    ("0", 19, None),
    ("0", 39, None),
];

#[test]
fn a_typed_figure_is_read_in_integers_or_refused() {
    for &(text, exponent, want) in PARSE_CASES {
        assert_eq!(
            splitz_host::parse_minor_units(text, exponent),
            want,
            "{text:?} at {exponent}"
        );
    }
}

#[test]
fn an_amount_is_read_at_its_currencys_exponent_and_not_in_one_with_none() {
    assert_eq!(splitz_host::parse_amount_in("12.34", "EUR"), Some(1234));
    assert_eq!(splitz_host::parse_amount_in("1234", "JPY"), Some(1234));
    assert_eq!(splitz_host::parse_amount_in("1.234", "KWD"), Some(1234));
    assert_eq!(splitz_host::parse_amount_in("12.3", "JPY"), None);
    assert_eq!(splitz_host::parse_amount_in("12", "XAU"), None);
    assert_eq!(splitz_host::parse_amount_in("12", "eur"), None);
}
