//! ZIP 321 payment requests (SPEC.md §8).
//!
//! One canonical rendering, because two wallets that render the same
//! obligation differently cannot check each other.

use crate::address::parse_address;
use crate::error::{code, Result, SplitError};
use crate::money::is_currency;
use crate::rate::ZATOSHI_PER_ZEC;

/// Whether `address` is one §8.3 admits: non-empty and ASCII alphanumeric.
///
/// Syntax only; [`crate::parse_address`] decodes one (§8.6).
pub fn is_zip321_address(address: &str) -> bool {
    !address.is_empty() && address.bytes().all(|b| b.is_ascii_alphanumeric())
}

/// 21000000 ZEC, in zatoshi.
pub const MAX_ZATOSHI: i64 = 21_000_000 * ZATOSHI_PER_ZEC;

/// The decoded memo cap, in bytes.
pub const MAX_MEMO_BYTES: usize = 512;

/// A display name is cut to this many UTF-8 bytes, on a character boundary.
pub const MAX_LABEL_BYTES: usize = 96;

/// Parameter indices run to 9999, so a request carries at most this many
/// payments.
pub const MAX_PAYMENTS: usize = 10_000;

/// The largest fiat count `fiat` admits, in digits. Eighteen keeps every
/// admissible value inside a signed 64-bit integer; nineteen does not.
pub const MAX_FIAT_DIGITS: usize = 18;

const QCHAR_EXTRA: &[u8] = b"!$'()*+,;:@";

/// A `<CUR>:<minorUnits>` price for one payment's amount.
///
/// It is the value of that payment, not the price of one ZEC.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FiatPrice {
    pub currency: String,
    pub minor_units: i64,
}

/// One output of a payment request.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Zip321Payment {
    pub address: String,
    pub zatoshi: i64,
    pub fiat: Option<FiatPrice>,
    pub memo: Option<Vec<u8>>,
    pub label: Option<String>,
    pub message: Option<String>,
}

/// Renders `zatoshi` as decimal ZEC (§8.1).
///
/// Trailing zeros are removed, so one value has exactly one representation.
pub fn render_amount(zatoshi: i64) -> Result<String> {
    if zatoshi <= 0 {
        return Err(SplitError::new(
            code::ZIP321_AMOUNT_NOT_POSITIVE,
            format!("A payment sends more than nothing, got {zatoshi}"),
        ));
    }
    if zatoshi > MAX_ZATOSHI {
        return Err(SplitError::new(
            code::ZIP321_AMOUNT_TOO_LARGE,
            format!("A payment of {zatoshi} zatoshi exceeds the supply"),
        ));
    }
    let coins = zatoshi / ZATOSHI_PER_ZEC;
    let zats = zatoshi % ZATOSHI_PER_ZEC;
    if zats == 0 {
        return Ok(coins.to_string());
    }
    let frac = format!("{zats:08}");
    Ok(format!("{coins}.{}", frac.trim_end_matches('0')))
}

/// Percent-escapes `text` as ZIP 321 `qchar` (§8.3).
pub fn qchar(text: &str) -> String {
    let mut out = String::new();
    for byte in text.as_bytes() {
        let unreserved = byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~');
        if unreserved || QCHAR_EXTRA.contains(byte) {
            out.push(*byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Cuts `name` to [`MAX_LABEL_BYTES`] on a character boundary (§8.3).
pub fn bounded_label(name: &str) -> &str {
    if name.len() <= MAX_LABEL_BYTES {
        return name;
    }
    let mut end = MAX_LABEL_BYTES;
    while end > 0 && !name.is_char_boundary(end) {
        end -= 1;
    }
    &name[..end]
}

/// Unpadded base64url, as everywhere in this protocol.
pub fn base64url(raw: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_";
    let mut out = String::new();
    for chunk in raw.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        if chunk.len() > 1 {
            out.push(ALPHABET[(n >> 6) as usize & 63] as char);
        }
        if chunk.len() > 2 {
            out.push(ALPHABET[n as usize & 63] as char);
        }
    }
    out
}

/// The base64url alphabet and nothing else — no padding, no whitespace.
pub fn is_b64url(value: &str) -> bool {
    value
        .bytes()
        .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

/// The inverse of [`base64url`]. `None` for anything outside the alphabet,
/// for a trailing group of one character, which encodes no byte, and for a
/// text that is not its bytes' canonical encoding — one whose last character
/// carries bits beyond the last whole byte (§9.4).
pub fn unbase64url(text: &str) -> Option<Vec<u8>> {
    if !is_b64url(text) {
        return None;
    }
    const fn index(b: u8) -> Option<u32> {
        match b {
            b'A'..=b'Z' => Some((b - b'A') as u32),
            b'a'..=b'z' => Some((b - b'a') as u32 + 26),
            b'0'..=b'9' => Some((b - b'0') as u32 + 52),
            b'-' => Some(62),
            b'_' => Some(63),
            _ => None,
        }
    }
    let mut out = Vec::new();
    for chunk in text.as_bytes().chunks(4) {
        if chunk.len() == 1 {
            return None;
        }
        let mut acc: u32 = 0;
        for (i, b) in chunk.iter().enumerate() {
            acc |= index(*b)? << (18 - 6 * i);
        }
        out.push((acc >> 16) as u8);
        if chunk.len() > 2 {
            out.push((acc >> 8) as u8);
        }
        if chunk.len() > 3 {
            out.push(acc as u8);
        }
    }
    (base64url(&out) == text).then_some(out)
}

/// Renders `price` as a `fiat` value (§8.4).
pub fn render_fiat(price: &FiatPrice) -> Result<String> {
    if !is_currency(&price.currency) {
        return Err(SplitError::new(
            code::ZIP321_BAD_CURRENCY_CODE,
            format!(
                "A fiat code is three upper-case letters, got \"{}\"",
                price.currency
            ),
        ));
    }
    if price.minor_units <= 0 {
        return Err(SplitError::new(
            code::ZIP321_FIAT_NOT_POSITIVE,
            format!("A fiat price of {} is not positive", price.minor_units),
        ));
    }
    if price.minor_units.to_string().len() > MAX_FIAT_DIGITS {
        return Err(SplitError::new(
            code::ZIP321_FIAT_TOO_MANY_DIGITS,
            format!(
                "A fiat price of {} exceeds {MAX_FIAT_DIGITS} digits",
                price.minor_units
            ),
        ));
    }
    Ok(format!("{}:{}", price.currency, price.minor_units))
}

/// Renders `payments` as one ZIP 321 URI (§8.2).
pub fn render_uri(payments: &[Zip321Payment], include_fiat: bool) -> Result<String> {
    if payments.is_empty() {
        return Err(SplitError::new(
            code::ZIP321_NO_PAYMENTS,
            "A request carries no payments",
        ));
    }
    if payments.len() > MAX_PAYMENTS {
        return Err(SplitError::new(
            code::ZIP321_TOO_MANY_PAYMENTS,
            format!(
                "A request carries {} payments, more than {MAX_PAYMENTS}",
                payments.len()
            ),
        ));
    }

    // The address is checked before any other parameter of the same payment,
    // so a payment invalid in two ways is refused with the same code
    // everywhere.
    for p in payments {
        if p.address.is_empty() {
            return Err(SplitError::new(
                code::ZIP321_NO_ADDRESS,
                "A payment names no address",
            ));
        }
        if !is_zip321_address(&p.address) {
            return Err(SplitError::new(
                code::ZIP321_BAD_ADDRESS,
                "The ZIP 321 grammar admits only alphanumeric addresses",
            ));
        }
        // A memo goes only to an address that decodes (§8.6) and can
        // receive one; ZIP 321 refuses the whole request otherwise.
        if p.memo.is_some() && !parse_address(&p.address)?.can_receive_memo {
            return Err(SplitError::new(
                code::ZIP321_MEMO_UNDELIVERABLE,
                "A memo cannot be delivered to a transparent or TEX address",
            ));
        }
    }

    let single = payments.len() == 1;
    let mut parts: Vec<String> = Vec::new();
    for (i, p) in payments.iter().enumerate() {
        let sfx = if i == 0 {
            String::new()
        } else {
            format!(".{i}")
        };
        if !(single && i == 0) {
            parts.push(format!("address{sfx}={}", p.address));
        }
        parts.push(format!("amount{sfx}={}", render_amount(p.zatoshi)?));
        if include_fiat {
            if let Some(price) = &p.fiat {
                parts.push(format!("fiat{sfx}={}", render_fiat(price)?));
            }
        }
        if let Some(memo) = &p.memo {
            if memo.len() > MAX_MEMO_BYTES {
                return Err(SplitError::new(
                    code::ZIP321_MEMO_TOO_LARGE,
                    format!("A memo of {} bytes exceeds {MAX_MEMO_BYTES}", memo.len()),
                ));
            }
            parts.push(format!("memo{sfx}={}", base64url(memo)));
        }
        if let Some(label) = &p.label {
            parts.push(format!("label{sfx}={}", qchar(bounded_label(label))));
        }
        if let Some(message) = &p.message {
            parts.push(format!("message{sfx}={}", qchar(message)));
        }
    }

    let head = if single {
        format!("zcash:{}?", payments[0].address)
    } else {
        "zcash:?".to_owned()
    };
    Ok(head + &parts.join("&"))
}

/// The parameters §8.2 writes, and nothing else.
const REQUEST_PARAMS: [&str; 6] = ["address", "amount", "fiat", "memo", "label", "message"];

fn not_canonical(why: impl Into<String>) -> SplitError {
    SplitError::new(code::ZIP321_NOT_CANONICAL, why)
}

/// Reads back a request in exactly the form [`render_uri`] writes (§8.7).
///
/// Not a general ZIP 321 reader: every URI this protocol hands a wallet is one
/// [`render_uri`] produced, so the payments read are rendered again and the
/// result must equal `uri` byte for byte. Anything else — another parameter,
/// another order, another spelling of one amount — is refused with
/// `zip321_not_canonical`, as is anything that does not read at all. A value
/// [`render_uri`] itself refuses is refused with that code.
pub fn read_request(uri: &str) -> Result<Vec<Zip321Payment>> {
    let rest = uri
        .strip_prefix("zcash:")
        .ok_or_else(|| not_canonical("Not a zcash: URI"))?;
    let (path_address, query) = rest
        .split_once('?')
        .ok_or_else(|| not_canonical("A request carries a query"))?;

    let mut by_index: std::collections::BTreeMap<usize, Vec<(&str, &str)>> =
        std::collections::BTreeMap::new();
    for part in query.split('&') {
        let (key, value) = match part.find('=') {
            Some(eq) if eq > 0 => (&part[..eq], &part[eq + 1..]),
            _ => return Err(not_canonical(format!("Not a parameter: \"{part}\""))),
        };
        let (name, index) = match key.split_once('.') {
            None => (key, 0),
            Some((name, digits)) => {
                // `.0` is not written, and an index has no leading zero (§8.2).
                let valid = (1..=4).contains(&digits.len())
                    && digits.bytes().all(|b| b.is_ascii_digit())
                    && !digits.starts_with('0');
                if !valid {
                    return Err(not_canonical(format!(
                        "Not a parameter index: \"{digits}\""
                    )));
                }
                (name, digits.parse::<usize>().expect("four digits"))
            }
        };
        if !REQUEST_PARAMS.contains(&name) {
            return Err(not_canonical(format!(
                "Not a parameter §8.2 writes: \"{name}\""
            )));
        }
        let params = by_index.entry(index).or_default();
        if params.iter().any(|(n, _)| *n == name) {
            return Err(not_canonical(format!("\"{key}\" appears twice")));
        }
        params.push((name, value));
    }

    let mut payments = Vec::with_capacity(by_index.len());
    for i in 0..by_index.len() {
        let params = by_index
            .get(&i)
            .ok_or_else(|| not_canonical(format!("Payment {i} is missing")))?;
        let get = |n: &str| params.iter().find(|(k, _)| *k == n).map(|(_, v)| *v);
        let address = if i == 0 && !path_address.is_empty() {
            if get("address").is_some() {
                return Err(not_canonical("The address is written twice"));
            }
            path_address
        } else {
            get("address").ok_or_else(|| not_canonical(format!("Payment {i} names no address")))?
        };
        let amount =
            get("amount").ok_or_else(|| not_canonical(format!("Payment {i} has no amount")))?;
        payments.push(Zip321Payment {
            address: address.to_owned(),
            zatoshi: read_amount(amount)?,
            fiat: get("fiat").map(read_fiat).transpose()?,
            memo: get("memo").map(read_memo).transpose()?,
            label: get("label").map(unqchar).transpose()?,
            message: get("message").map(unqchar).transpose()?,
        });
    }

    let include_fiat = payments.iter().any(|p| p.fiat.is_some());
    if render_uri(&payments, include_fiat)? != uri {
        return Err(not_canonical("Not the form §8 writes"));
    }
    Ok(payments)
}

/// Decimal ZEC to zatoshi. The canonical spelling is enforced by the
/// comparison [`read_request`] makes afterwards.
fn read_amount(text: &str) -> Result<i64> {
    let bad = || not_canonical(format!("Not an amount: \"{text}\""));
    let (coins, zats) = match text.split_once('.') {
        None => (text, ""),
        Some((c, z)) if !z.is_empty() => (c, z),
        Some(_) => return Err(bad()),
    };
    let digits =
        |s: &str, max: usize| (1..=max).contains(&s.len()) && s.bytes().all(|b| b.is_ascii_digit());
    if !digits(coins, 8) || !(zats.is_empty() || digits(zats, 8)) {
        return Err(bad());
    }
    let coins: i64 = coins.parse().map_err(|_| bad())?;
    let zats: i64 = format!("{zats:0<8}").parse().map_err(|_| bad())?;
    Ok(coins * ZATOSHI_PER_ZEC + zats)
}

fn read_fiat(text: &str) -> Result<FiatPrice> {
    let bad = || not_canonical(format!("Not a fiat price: \"{text}\""));
    let (currency, count) = text.split_once(':').ok_or_else(bad)?;
    let valid = currency.len() == 3
        && currency.bytes().all(|b| b.is_ascii_uppercase())
        && (1..=MAX_FIAT_DIGITS).contains(&count.len())
        && count.bytes().all(|b| b.is_ascii_digit());
    if !valid {
        return Err(bad());
    }
    Ok(FiatPrice {
        currency: currency.to_owned(),
        minor_units: count.parse().map_err(|_| bad())?,
    })
}

fn read_memo(text: &str) -> Result<Vec<u8>> {
    unbase64url(text).ok_or_else(|| not_canonical(format!("Not unpadded base64url: \"{text}\"")))
}

/// Undoes [`qchar`]. Malformed escapes and bytes that are not UTF-8 are
/// refused.
fn unqchar(text: &str) -> Result<String> {
    let raw = text.as_bytes();
    let mut out = Vec::with_capacity(raw.len());
    let mut i = 0;
    while i < raw.len() {
        match raw[i] {
            b'%' => {
                let pair = raw
                    .get(i + 1..i + 3)
                    .filter(|p| p.iter().all(u8::is_ascii_hexdigit))
                    .ok_or_else(|| not_canonical("Not an escape"))?;
                let hex = std::str::from_utf8(pair).expect("hex digits are ASCII");
                out.push(u8::from_str_radix(hex, 16).expect("two hex digits"));
                i += 3;
            }
            b if b < 0x80 => {
                out.push(b);
                i += 1;
            }
            _ => return Err(not_canonical("A raw non-ASCII character")),
        }
    }
    String::from_utf8(out).map_err(|_| not_canonical("Escapes that are not UTF-8"))
}
