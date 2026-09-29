//! Zcash addresses (SPEC.md §8.6).
//!
//! Which network an address belongs to, what kind it is, the receivers a
//! Unified Address carries, and whether a memo can be delivered to it. The
//! encodings are Base58Check for transparent addresses, Bech32 (ZIP 173) for
//! Sapling, Bech32m (BIP 350) for TEX (ZIP 320), and Bech32m over F4Jumble
//! for a revision 0 Unified Address (ZIP 316).
//!
//! Receiver bytes are checked for length, not decoded as curve points.

use crate::error::{code, Result, SplitError};
use crate::sha256::sha256;

/// The network an address belongs to.
///
/// Regtest transparent addresses share testnet's lead bytes, so they answer
/// [`AddressNetwork::Test`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressNetwork {
    Main,
    Test,
    Regtest,
}

impl AddressNetwork {
    /// The wire name: `main`, `test` or `regtest`.
    pub fn as_str(self) -> &'static str {
        match self {
            AddressNetwork::Main => "main",
            AddressNetwork::Test => "test",
            AddressNetwork::Regtest => "regtest",
        }
    }
}

/// The encoding an address uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AddressKind {
    /// Transparent, pay to public key hash.
    P2pkh,
    /// Transparent, pay to script hash.
    P2sh,
    /// A transparent-source-only address (ZIP 320).
    Tex,
    Sapling,
    /// A revision 0 Unified Address (ZIP 316).
    Unified,
}

impl AddressKind {
    /// The wire name: `p2pkh`, `p2sh`, `tex`, `sapling` or `unified`.
    pub fn as_str(self) -> &'static str {
        match self {
            AddressKind::P2pkh => "p2pkh",
            AddressKind::P2sh => "p2sh",
            AddressKind::Tex => "tex",
            AddressKind::Sapling => "sapling",
            AddressKind::Unified => "unified",
        }
    }
}

/// ZIP 316 receiver typecodes.
pub const TYPECODE_P2PKH: u32 = 0x00;
pub const TYPECODE_P2SH: u32 = 0x01;
pub const TYPECODE_SAPLING: u32 = 0x02;
pub const TYPECODE_ORCHARD: u32 = 0x03;

/// What §8.6 answers for one address.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParsedAddress {
    pub network: AddressNetwork,
    pub kind: AddressKind,
    /// A Unified Address's typecodes in encoding order, which is ascending.
    /// Typecodes this crate does not name are kept by number. Empty for every
    /// other kind.
    pub receivers: Vec<u32>,
    /// Whether a memo reaches the recipient: false for transparent and TEX,
    /// true for Sapling, and for a Unified Address true when it carries a
    /// Sapling or Orchard receiver.
    pub can_receive_memo: bool,
}

const CHARSET: &[u8; 32] = b"qpzry9x8gf2tvdw0s3jn54khce6mua7l";
const BECH32_CONST: u32 = 1;
const BECH32M_CONST: u32 = 0x2BC8_30A3;
const BASE58: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";
/// Two lead bytes, a 20-byte hash and a 4-byte checksum.
const BASE58CHECK_BYTES: usize = 26;

/// Bech32 human-readable parts: ZIP 316 and ZIP 320 for main and test;
/// zcash_protocol 0.10 `constants/regtest.rs` for regtest.
const SAPLING_HRPS: [(&str, AddressNetwork); 3] = [
    ("zs", AddressNetwork::Main),
    ("ztestsapling", AddressNetwork::Test),
    ("zregtestsapling", AddressNetwork::Regtest),
];
const TEX_HRPS: [(&str, AddressNetwork); 3] = [
    ("tex", AddressNetwork::Main),
    ("textest", AddressNetwork::Test),
    ("texregtest", AddressNetwork::Regtest),
];
const UNIFIED_HRPS: [(&str, AddressNetwork); 3] = [
    ("u", AddressNetwork::Main),
    ("utest", AddressNetwork::Test),
    ("uregtest", AddressNetwork::Regtest),
];

/// Base58Check lead bytes (zcash_protocol 0.10 `constants/{mainnet,testnet}.rs`).
const TRANSPARENT_LEADS: [([u8; 2], AddressNetwork, AddressKind); 4] = [
    ([0x1C, 0xB8], AddressNetwork::Main, AddressKind::P2pkh),
    ([0x1C, 0xBD], AddressNetwork::Main, AddressKind::P2sh),
    ([0x1D, 0x25], AddressNetwork::Test, AddressKind::P2pkh),
    ([0x1C, 0xBA], AddressNetwork::Test, AddressKind::P2sh),
];

/// ZIP 316: typecode and length values are at most this.
const MAX_COMPACT_SIZE: u64 = 0x0200_0000;
/// ZIP 316: the HRP, zero-padded to this many bytes, ends the raw encoding.
const UA_PADDING: usize = 16;
/// The lengths F4Jumble's inverse accepts (ZIP 316 revision 0, "Jumbling").
const F4_MIN: usize = 48;
const F4_MAX: usize = 4_194_368;

fn invalid(message: &str) -> SplitError {
    SplitError::new(code::ADDRESS_INVALID, message)
}

/// Answers §8.6 for `text`, or refuses with `address_invalid`.
///
/// The text is taken exactly: no whitespace is trimmed, and Bech32 and
/// Bech32m are read in lower case only.
pub fn parse_address(text: &str) -> Result<ParsedAddress> {
    if let Some((network, raw)) = bech32_decode(text, &UNIFIED_HRPS, BECH32M_CONST) {
        let hrp = UNIFIED_HRPS
            .iter()
            .find(|(_, n)| *n == network)
            .map(|(h, _)| *h)
            .unwrap_or_default();
        let receivers = unified_receivers(hrp, raw)
            .ok_or_else(|| invalid("Not a revision 0 Unified Address ZIP 316 admits"))?;
        let can_receive_memo = receivers
            .iter()
            .any(|&t| t == TYPECODE_SAPLING || t == TYPECODE_ORCHARD);
        return Ok(ParsedAddress {
            network,
            kind: AddressKind::Unified,
            receivers,
            can_receive_memo,
        });
    }
    if let Some((network, raw)) = bech32_decode(text, &SAPLING_HRPS, BECH32_CONST) {
        if raw.len() != 43 {
            return Err(invalid("A Sapling address carries 43 bytes"));
        }
        return Ok(ParsedAddress {
            network,
            kind: AddressKind::Sapling,
            receivers: Vec::new(),
            can_receive_memo: true,
        });
    }
    if let Some((network, raw)) = bech32_decode(text, &TEX_HRPS, BECH32M_CONST) {
        if raw.len() != 20 {
            return Err(invalid("A TEX address carries 20 bytes"));
        }
        return Ok(ParsedAddress {
            network,
            kind: AddressKind::Tex,
            receivers: Vec::new(),
            can_receive_memo: false,
        });
    }
    let payload = base58check(text)
        .filter(|p| p.len() == 22)
        .ok_or_else(|| invalid("Not a Zcash address"))?;
    let (_, network, kind) = TRANSPARENT_LEADS
        .iter()
        .find(|(lead, _, _)| payload[..2] == lead[..])
        .ok_or_else(|| invalid("Not a Zcash transparent address"))?;
    Ok(ParsedAddress {
        network: *network,
        kind: *kind,
        receivers: Vec::new(),
        can_receive_memo: false,
    })
}

fn polymod(values: impl Iterator<Item = u8>) -> u32 {
    const GEN: [u32; 5] = [
        0x3B6A_57B2,
        0x2650_8E6D,
        0x1EA1_19FA,
        0x3D42_33DD,
        0x2A14_62B3,
    ];
    let mut chk: u32 = 1;
    for v in values {
        let top = chk >> 25;
        chk = ((chk & 0x01FF_FFFF) << 5) ^ u32::from(v);
        for (i, g) in GEN.iter().enumerate() {
            if (top >> i) & 1 == 1 {
                chk ^= g;
            }
        }
    }
    chk
}

/// The network and bytes of a string under one of `hrps`, or `None`.
///
/// Lower case only, since the prefixes and the alphabet are compared as
/// written: ZIP 173 has encoders write lower case, and the decoder wallets use
/// (zcash_address 0.13) refuses upper. The 5-bit groups regroup into bytes,
/// and the leftover bits number at most four and are zero (ZIP 173,
/// "Decoding").
fn bech32_decode(
    text: &str,
    hrps: &[(&str, AddressNetwork)],
    constant: u32,
) -> Option<(AddressNetwork, Vec<u8>)> {
    let bytes = text.as_bytes();
    let sep = bytes.iter().rposition(|&b| b == b'1')?;
    if sep == 0 {
        return None;
    }
    let (hrp, data) = (&bytes[..sep], &bytes[sep + 1..]);
    let network = hrps.iter().find(|(h, _)| h.as_bytes() == hrp)?.1;
    if data.len() < 6 {
        return None;
    }
    let values: Vec<u8> = data
        .iter()
        .map(|c| CHARSET.iter().position(|x| x == c).map(|i| i as u8))
        .collect::<Option<_>>()?;
    let expanded = hrp
        .iter()
        .map(|c| c >> 5)
        .chain(std::iter::once(0))
        .chain(hrp.iter().map(|c| c & 31));
    if polymod(expanded.chain(values.iter().copied())) != constant {
        return None;
    }
    let mut acc: u32 = 0;
    let mut bits = 0;
    let mut out = Vec::with_capacity(values.len() * 5 / 8);
    for &v in &values[..values.len() - 6] {
        acc = ((acc << 5) | u32::from(v)) & 0xFFF;
        bits += 5;
        if bits >= 8 {
            bits -= 8;
            out.push((acc >> bits) as u8);
        }
    }
    if bits > 4 || acc & ((1 << bits) - 1) != 0 {
        return None;
    }
    Some((network, out))
}

/// The payload of a Base58Check string, its four checksum bytes removed.
fn base58check(text: &str) -> Option<Vec<u8>> {
    // Big-endian base-256 digits, grown one base-58 digit at a time.
    let mut body: Vec<u8> = Vec::new();
    for c in text.bytes() {
        let mut carry = BASE58.iter().position(|&x| x == c)? as u32;
        for byte in body.iter_mut().rev() {
            carry += u32::from(*byte) * 58;
            *byte = carry as u8;
            carry >>= 8;
        }
        while carry > 0 {
            body.insert(0, carry as u8);
            carry >>= 8;
        }
        // Lead, payload and checksum are 26 bytes; anything longer is not a
        // transparent address, and stopping here keeps the work linear.
        if body.len() > BASE58CHECK_BYTES {
            return None;
        }
    }
    let zeros = text.bytes().take_while(|&b| b == b'1').count();
    let mut raw = vec![0u8; zeros];
    raw.extend(body);
    if raw.len() < 4 {
        return None;
    }
    let (payload, check) = raw.split_at(raw.len() - 4);
    (sha256(&sha256(payload))[..4] == *check).then(|| payload.to_vec())
}

/// A canonical compactSize at `at`, with the offset past it.
fn compact_size(raw: &[u8], at: usize) -> Option<(u64, usize)> {
    let flag = *raw.get(at)?;
    let (value, width) = match flag {
        0..=252 => (u64::from(flag), 1),
        _ => {
            let (size, least) = match flag {
                253 => (2, 253),
                254 => (4, 0x1_0000),
                _ => (8, 0x1_0000_0000),
            };
            let bytes = raw.get(at + 1..at + 1 + size)?;
            let value = bytes
                .iter()
                .rev()
                .fold(0u64, |acc, &b| (acc << 8) | u64::from(b));
            if value < least {
                return None;
            }
            (value, 1 + size)
        }
    };
    (value <= MAX_COMPACT_SIZE).then_some((value, at + width))
}

/// The typecodes of a revision 0 Unified Address, in encoding order.
fn unified_receivers(hrp: &str, mut raw: Vec<u8>) -> Option<Vec<u32>> {
    if !(F4_MIN..=F4_MAX).contains(&raw.len()) {
        return None;
    }
    f4jumble_inv(&mut raw);
    let mut padding = [0u8; UA_PADDING];
    padding[..hrp.len()].copy_from_slice(hrp.as_bytes());
    let (body, tail) = raw.split_at(raw.len() - UA_PADDING);
    if tail != padding {
        return None;
    }
    let mut at = 0;
    let mut codes: Vec<u32> = Vec::new();
    while at < body.len() {
        let (typecode, next) = compact_size(body, at)?;
        let (length, next) = compact_size(body, next)?;
        let end = next.checked_add(length as usize)?;
        if end > body.len() {
            return None;
        }
        let typecode = typecode as u32;
        let expected = match typecode {
            TYPECODE_P2PKH | TYPECODE_P2SH => Some(20),
            TYPECODE_SAPLING | TYPECODE_ORCHARD => Some(43),
            _ => None,
        };
        if expected.is_some_and(|n| n != length) {
            return None;
        }
        codes.push(typecode);
        at = end;
    }
    // Ascending, so one comparison refuses a repeat and a reordering alike.
    if codes.windows(2).any(|w| w[1] <= w[0]) {
        return None;
    }
    if codes.contains(&TYPECODE_P2PKH) && codes.contains(&TYPECODE_P2SH) {
        return None;
    }
    // Revision 0 admits no MUST-understand metadata.
    if codes.iter().any(|c| (0xE0..=0xFC).contains(c)) {
        return None;
    }
    if !codes.contains(&TYPECODE_SAPLING) && !codes.contains(&TYPECODE_ORCHARD) {
        return None;
    }
    Some(codes)
}

/// ZIP 316 F4Jumble⁻¹, in place. `m.len()` is within `F4_MIN..=F4_MAX`.
fn f4jumble_inv(m: &mut [u8]) {
    let left_len = (m.len() / 2).min(64);
    let (left, right) = m.split_at_mut(left_len);
    h_round(left, right, 1);
    g_round(left, right, 1);
    h_round(left, right, 0);
    g_round(left, right, 0);
}

fn h_round(left: &mut [u8], right: &[u8], i: u8) {
    let mut personal = *b"UA_F4Jumble_H\0\0\0";
    personal[13] = i;
    let hash = blake2b(right, left.len(), &personal);
    left.iter_mut().zip(hash).for_each(|(a, b)| *a ^= b);
}

fn g_round(left: &[u8], right: &mut [u8], i: u8) {
    for (j, chunk) in right.chunks_mut(64).enumerate() {
        let mut personal = *b"UA_F4Jumble_G\0\0\0";
        personal[13] = i;
        personal[14] = (j & 0xFF) as u8;
        personal[15] = (j >> 8) as u8;
        let hash = blake2b(left, 64, &personal);
        chunk.iter_mut().zip(hash).for_each(|(a, b)| *a ^= b);
    }
}

const BLAKE2B_IV: [u64; 8] = [
    0x6A09_E667_F3BC_C908,
    0xBB67_AE85_84CA_A73B,
    0x3C6E_F372_FE94_F82B,
    0xA54F_F53A_5F1D_36F1,
    0x510E_527F_ADE6_82D1,
    0x9B05_688C_2B3E_6C1F,
    0x1F83_D9AB_FB41_BD6B,
    0x5BE0_CD19_137E_2179,
];

const BLAKE2B_SIGMA: [[usize; 16]; 12] = [
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
    [11, 8, 12, 0, 5, 2, 15, 13, 10, 14, 3, 6, 7, 1, 9, 4],
    [7, 9, 3, 1, 13, 12, 11, 14, 2, 6, 5, 10, 4, 0, 15, 8],
    [9, 0, 5, 7, 2, 4, 10, 15, 14, 1, 11, 12, 6, 8, 3, 13],
    [2, 12, 6, 10, 0, 11, 8, 3, 4, 13, 7, 5, 15, 14, 1, 9],
    [12, 5, 1, 15, 14, 13, 4, 10, 0, 7, 6, 3, 9, 2, 8, 11],
    [13, 11, 7, 14, 12, 1, 3, 9, 5, 0, 15, 4, 8, 6, 2, 10],
    [6, 15, 14, 9, 11, 3, 0, 8, 12, 2, 13, 7, 1, 4, 10, 5],
    [10, 2, 8, 4, 7, 6, 1, 5, 15, 11, 9, 14, 3, 12, 13, 0],
    [0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15],
    [14, 10, 4, 8, 9, 15, 13, 6, 1, 12, 0, 2, 11, 7, 5, 3],
];

/// Unkeyed BLAKE2b (RFC 7693) with a 16-byte personalization, `out_len`
/// bytes of output, 1 to 64.
fn blake2b(message: &[u8], out_len: usize, personal: &[u8; 16]) -> Vec<u8> {
    let mut h = BLAKE2B_IV;
    h[0] ^= 0x0101_0000 ^ out_len as u64;
    h[6] ^= u64::from_le_bytes(personal[..8].try_into().expect("8 bytes"));
    h[7] ^= u64::from_le_bytes(personal[8..].try_into().expect("8 bytes"));

    let mut counter: u128 = 0;
    let blocks = message.len().div_ceil(128).max(1);
    for b in 0..blocks {
        let chunk = &message[b * 128..message.len().min(b * 128 + 128)];
        let mut block = [0u8; 128];
        block[..chunk.len()].copy_from_slice(chunk);
        counter += chunk.len() as u128;
        compress(&mut h, &block, counter, b + 1 == blocks);
    }
    h.iter()
        .flat_map(|w| w.to_le_bytes())
        .take(out_len)
        .collect()
}

fn compress(h: &mut [u64; 8], block: &[u8; 128], counter: u128, last: bool) {
    let m: Vec<u64> = block
        .chunks(8)
        .map(|c| u64::from_le_bytes(c.try_into().expect("8 bytes")))
        .collect();
    let mut v = [0u64; 16];
    v[..8].copy_from_slice(h);
    v[8..].copy_from_slice(&BLAKE2B_IV);
    v[12] ^= counter as u64;
    v[13] ^= (counter >> 64) as u64;
    if last {
        v[14] = !v[14];
    }
    fn g(v: &mut [u64; 16], a: usize, b: usize, c: usize, d: usize, x: u64, y: u64) {
        v[a] = v[a].wrapping_add(v[b]).wrapping_add(x);
        v[d] = (v[d] ^ v[a]).rotate_right(32);
        v[c] = v[c].wrapping_add(v[d]);
        v[b] = (v[b] ^ v[c]).rotate_right(24);
        v[a] = v[a].wrapping_add(v[b]).wrapping_add(y);
        v[d] = (v[d] ^ v[a]).rotate_right(16);
        v[c] = v[c].wrapping_add(v[d]);
        v[b] = (v[b] ^ v[c]).rotate_right(63);
    }
    for s in &BLAKE2B_SIGMA {
        g(&mut v, 0, 4, 8, 12, m[s[0]], m[s[1]]);
        g(&mut v, 1, 5, 9, 13, m[s[2]], m[s[3]]);
        g(&mut v, 2, 6, 10, 14, m[s[4]], m[s[5]]);
        g(&mut v, 3, 7, 11, 15, m[s[6]], m[s[7]]);
        g(&mut v, 0, 5, 10, 15, m[s[8]], m[s[9]]);
        g(&mut v, 1, 6, 11, 12, m[s[10]], m[s[11]]);
        g(&mut v, 2, 7, 8, 13, m[s[12]], m[s[13]]);
        g(&mut v, 3, 4, 9, 14, m[s[14]], m[s[15]]);
    }
    for i in 0..8 {
        h[i] ^= v[i] ^ v[i + 8];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// RFC 7693 Appendix A: BLAKE2b-512("abc").
    #[test]
    fn blake2b_matches_rfc_7693() {
        let mut want = String::new();
        for b in blake2b(b"abc", 64, &[0; 16]) {
            want.push_str(&format!("{b:02x}"));
        }
        assert_eq!(
            want,
            "ba80a53f981c4d0d6a2797b69f12f6e94c212f14685ac4b74b12bb6fdbffa2d1\
             7d87c5392aab792dc252d5de4533cc9518d38aa8dbf1925ab92386edd4009923"
        );
    }

    /// The upper bound on what F4Jumble⁻¹ accepts is a refusal, not a
    /// personalization counter that wraps.
    #[test]
    fn a_unified_address_past_the_jumble_limit_is_refused() {
        let raw = vec![0u8; F4_MAX + 1];
        assert_eq!(unified_receivers("u", raw), None);
    }
}
