//! ZIP 321 payment requests (SPEC.md §8).
//!
//! One canonical rendering, because two wallets that render the same
//! obligation differently cannot check each other.

use crate::error::{code, Result, SplitError};
use crate::money::is_currency;
use crate::rate::ZATOSHI_PER_ZEC;

/// Whether `address` is one §8.3 admits: non-empty and ASCII alphanumeric.
///
/// Syntax only. A wallet still puts every address through its own decoder.
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
/// and for a trailing group of one character, which encodes no byte.
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
    Some(out)
}

fn render_fiat(price: &FiatPrice) -> Result<String> {
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
