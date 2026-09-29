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
