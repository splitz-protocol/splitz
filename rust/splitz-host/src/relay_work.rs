//! The work a relay push carries in place of anything that names its sender
//! (SPEC.md §15.5).

use splitz_core::{sha256, sha256_hex};

/// The fewest leading zero bits a push's stamp needs, before its size and
/// the relay's load add to it.
pub const RELAY_WORK_BASE: u32 = 16;

/// The largest base a relay asks for: [`RELAY_WORK_BASE`] and 8 bits more as
/// its store fills (§15.5). An ask above it is refused rather than worked for.
pub const RELAY_WORK_MAX_BASE: u32 = RELAY_WORK_BASE + 8;

/// The characters of blob one unit of work pays for.
pub const RELAY_WORK_UNIT_CHARS: usize = 1024;

/// The leading zero bits a push of `chars` blob characters needs at a relay
/// asking `base`: one more bit for every doubling of the units it carries,
/// rounded up, so the work grows with what it stores.
pub fn relay_work_bits(chars: usize, base: u32) -> u32 {
    let units = chars.div_ceil(RELAY_WORK_UNIT_CHARS).max(1);
    base.saturating_add(usize::BITS - (units - 1).leading_zeros())
}

/// The hex SHA-256 of every blob's own SHA-256, in order: fixed-length
/// pieces, so no other list of blobs hashes to it.
fn blobs_digest(blobs: &[String]) -> String {
    let each: Vec<u8> = blobs.iter().flat_map(|b| sha256(b.as_bytes())).collect();
    sha256_hex(&each)
}

fn prefix(channel: &str, blobs: &[String]) -> String {
    format!("splitz-relay-work-v1|{channel}|{}|", blobs_digest(blobs))
}

/// The leading zero bits of `digest`.
pub fn leading_zero_bits(digest: &[u8]) -> u32 {
    let mut bits = 0;
    for &byte in digest {
        if byte != 0 {
            return bits + byte.leading_zeros();
        }
        bits += 8;
    }
    bits
}

/// Whether `nonce` stamps `blobs` on `channel` with at least `bits` leading
/// zero bits of SHA-256 over the domain, the channel, the hex SHA-256 of
/// every blob's own SHA-256 in order, and the nonce, separated by `|`.
pub fn relay_work_holds(channel: &str, blobs: &[String], nonce: &str, bits: u32) -> bool {
    let message = format!("{}{nonce}", prefix(channel, blobs));
    leading_zero_bits(&sha256(message.as_bytes())) >= bits
}

/// The first decimal nonce from zero that stamps `blobs` on `channel` with
/// `bits` leading zero bits.
pub fn relay_work_stamp(channel: &str, blobs: &[String], bits: u32) -> String {
    let prefix = prefix(channel, blobs);
    let mut message = prefix.clone().into_bytes();
    (0u64..)
        .find(|n| {
            message.truncate(prefix.len());
            message.extend_from_slice(n.to_string().as_bytes());
            leading_zero_bits(&sha256(&message)) >= bits
        })
        .map(|n| n.to_string())
        .unwrap_or_default()
}
