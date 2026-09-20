//! Ascending order, everywhere (SPEC.md §2.3).
//!
//! The order is the lexicographic order of the UTF-8 encoding, byte by byte.
//! Rust's own `str` comparison is already UTF-8 byte order, so this module is
//! thin — but the rule is stated here because a consumer porting this crate to
//! a language whose native comparison is not byte order needs to know that it
//! decides leftover minor units, settlement ties and log order.

/// Compares `a` and `b` as UTF-8 byte sequences.
pub fn compare_utf8(a: &str, b: &str) -> std::cmp::Ordering {
    a.as_bytes().cmp(b.as_bytes())
}

/// `values` in ascending UTF-8 byte order.
pub fn sorted_utf8<'a>(values: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = values.into_iter().map(str::to_owned).collect();
    out.sort_by(|a, b| compare_utf8(a, b));
    out
}

/// The distinct members of `values`, in ascending UTF-8 byte order.
pub fn unique_sorted_utf8<'a>(values: impl IntoIterator<Item = &'a str>) -> Vec<String> {
    let mut out: Vec<String> = values.into_iter().map(str::to_owned).collect();
    out.sort_by(|a, b| compare_utf8(a, b));
    out.dedup();
    out
}
