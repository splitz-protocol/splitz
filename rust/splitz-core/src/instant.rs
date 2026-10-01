//! Canonical instants (SPEC.md §9.3).
//!
//! RFC 3339, UTC, exactly three fractional digits, `Z` suffix. Canonical
//! instants are fixed width, so their lexicographic order is their
//! chronological order and a log sorts without a calendar.

use crate::error::{code, Result, SplitError};

fn days_in_month(year: u32, month: u32) -> u32 {
    match month {
        2 => {
            let leap =
                (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400);
            if leap {
                29
            } else {
                28
            }
        }
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        _ => 30,
    }
}

fn bad(text: &str, why: &str) -> SplitError {
    SplitError::new(code::BILL_TYPE_ERROR, format!("{why}: \"{text}\""))
}

/// Normalises `text` to canonical form.
///
/// Fractional digits beyond the third are truncated, never rounded. A numeric
/// offset, a space separator, a leap second, a day the month does not have, a
/// year outside 0001 to 9999 and a bare date are all refused: a looser rule
/// lets two readers give one entry two different sort keys.
pub fn canonical_instant(text: &str) -> Result<String> {
    let bytes = text.as_bytes();
    // YYYY-MM-DDTHH:MM:SS is nineteen characters before the optional fraction.
    if bytes.len() < 20 {
        return Err(bad(text, "Not a canonical instant"));
    }
    let digits = |from: usize, to: usize| -> Option<u32> {
        text.get(from..to)?
            .parse::<u32>()
            .ok()
            .filter(|_| text.as_bytes()[from..to].iter().all(u8::is_ascii_digit))
    };
    if bytes[4] != b'-' || bytes[7] != b'-' || bytes[13] != b':' || bytes[16] != b':' {
        return Err(bad(text, "Not a canonical instant"));
    }
    if !matches!(bytes[10], b'T' | b't') {
        return Err(bad(text, "Not a canonical instant"));
    }

    let year = digits(0, 4).ok_or_else(|| bad(text, "Not a date"))?;
    let month = digits(5, 7).ok_or_else(|| bad(text, "Not a date"))?;
    let day = digits(8, 10).ok_or_else(|| bad(text, "Not a date"))?;
    let hour = digits(11, 13).ok_or_else(|| bad(text, "Not a time of day"))?;
    let minute = digits(14, 16).ok_or_else(|| bad(text, "Not a time of day"))?;
    let second = digits(17, 19).ok_or_else(|| bad(text, "Not a time of day"))?;

    let tail = &text[19..];
    let fraction = if let Some(rest) = tail.strip_prefix('.') {
        let taken: String = rest.chars().take_while(char::is_ascii_digit).collect();
        if taken.is_empty() {
            return Err(bad(text, "Not a canonical instant"));
        }
        if !matches!(rest.as_bytes()[taken.len()..], [b'Z'] | [b'z']) {
            return Err(bad(text, "Not a canonical instant"));
        }
        taken
    } else {
        if !matches!(tail.as_bytes(), [b'Z'] | [b'z']) {
            // A numeric offset is not UTC.
            return Err(bad(text, "Not a canonical instant"));
        }
        "000".to_owned()
    };

    if year < 1 || !(1..=12).contains(&month) {
        return Err(bad(text, "Not a date"));
    }
    if day < 1 || day > days_in_month(year, month) {
        return Err(bad(text, "No such day"));
    }
    // A leap second is refused: it is not a second this grammar has.
    if hour > 23 || minute > 59 || second > 59 {
        return Err(bad(text, "Not a time of day"));
    }

    let mut millis = fraction;
    millis.push_str("000");
    millis.truncate(3);
    Ok(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis}Z"
    ))
}
