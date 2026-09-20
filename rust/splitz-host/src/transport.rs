//! The two calls this crate makes, and neither of them itself.
//!
//! Injected rather than made here, so bill sync and a swap take the same
//! network route as the rest of the wallet. On a build that routes through Tor
//! they go over Tor and fail closed while Tor is starting or broken, instead
//! of being the one path that quietly leaves in the clear.

/// Posts and fetches, as the embedding wallet already does them.
pub trait HttpTransport {
    /// Posts `body` to `url` and returns the response body, or why not.
    fn post(&self, url: &str, body: &str) -> Result<String, String>;
    /// Fetches `url` and returns the response body, or why not.
    fn get(&self, url: &str) -> Result<String, String>;
}

/// One query parameter's value, encoded the way a URL carries it.
///
/// Unreserved characters (`A-Z a-z 0-9 - . _ ~`) stand; a space is `+`;
/// everything else is percent-encoded from its UTF-8 bytes. Stated here rather
/// than taken from a URL library, because two libraries disagree about the
/// sub-delimiters and a status query that addresses a different swap is not a
/// cosmetic difference.
pub fn query_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(byte as char)
            }
            b' ' => out.push('+'),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

/// One path or key component, encoded the way a URL carries it.
///
/// Unreserved for this purpose is `A-Z a-z 0-9` and `! ' ( ) * - . _ ~`;
/// everything else, a space included, is percent-encoded from its UTF-8
/// bytes. The set is stated rather than taken from a library so that a value
/// stored under a key on one implementation is found under it on the other.
pub fn component_encode(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for byte in value.bytes() {
        match byte {
            b'A'..=b'Z'
            | b'a'..=b'z'
            | b'0'..=b'9'
            | b'!'
            | b'\''
            | b'('
            | b')'
            | b'*'
            | b'-'
            | b'.'
            | b'_'
            | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}
