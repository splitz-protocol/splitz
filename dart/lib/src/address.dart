/// Zcash addresses (SPEC.md §8.6).
///
/// Which network an address belongs to, what kind it is, the receivers a
/// Unified Address carries, and whether a memo can be delivered to it. The
/// encodings are Base58Check for transparent addresses, Bech32 (ZIP 173) for
/// Sapling, Bech32m (BIP 350) for TEX (ZIP 320), and Bech32m over F4Jumble for
/// a revision 0 Unified Address (ZIP 316).
///
/// Receiver bytes are checked for length, not decoded as curve points.
library;

import 'dart:convert';

import 'errors.dart';
import 'sha256.dart';

/// The network an address belongs to. [name] is the wire name.
///
/// Regtest transparent addresses share testnet's lead bytes, so they answer
/// [AddressNetwork.test].
enum AddressNetwork { main, test, regtest }

/// The encoding an address uses. [name] is the wire name.
///
/// [tex] is a transparent-source-only address (ZIP 320); [unified] is a
/// revision 0 Unified Address (ZIP 316).
enum AddressKind { p2pkh, p2sh, tex, sapling, unified }

/// ZIP 316 receiver typecodes.
const int typecodeP2pkh = 0x00;
const int typecodeP2sh = 0x01;
const int typecodeSapling = 0x02;
const int typecodeOrchard = 0x03;

/// What §8.6 answers for one address.
class ParsedAddress {
  const ParsedAddress({
    required this.network,
    required this.kind,
    required this.receivers,
    required this.canReceiveMemo,
  });

  final AddressNetwork network;
  final AddressKind kind;

  /// A Unified Address's typecodes in encoding order, which is ascending.
  /// Typecodes this library does not name are kept by number. Empty for every
  /// other kind.
  final List<int> receivers;

  /// Whether a memo reaches the recipient: false for transparent and TEX,
  /// true for Sapling, and for a Unified Address true when it carries a
  /// Sapling or Orchard receiver.
  final bool canReceiveMemo;
}

const String _charset = 'qpzry9x8gf2tvdw0s3jn54khce6mua7l';
const int _bech32Const = 1;
const int _bech32mConst = 0x2BC830A3;
const String _base58 =
    '123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz';

/// Two lead bytes, a 20-byte hash and a 4-byte checksum.
const int _base58CheckBytes = 26;

/// Bech32 human-readable parts: ZIP 316 and ZIP 320 for main and test;
/// zcash_protocol 0.10 `constants/regtest.rs` for regtest.
const Map<String, AddressNetwork> _saplingHrps = {
  'zs': AddressNetwork.main,
  'ztestsapling': AddressNetwork.test,
  'zregtestsapling': AddressNetwork.regtest,
};
const Map<String, AddressNetwork> _texHrps = {
  'tex': AddressNetwork.main,
  'textest': AddressNetwork.test,
  'texregtest': AddressNetwork.regtest,
};
const Map<String, AddressNetwork> _unifiedHrps = {
  'u': AddressNetwork.main,
  'utest': AddressNetwork.test,
  'uregtest': AddressNetwork.regtest,
};

/// ZIP 316: typecode and length values are at most this.
const int _maxCompactSize = 0x2000000;

/// ZIP 316: the HRP, zero-padded to this many bytes, ends the raw encoding.
const int _uaPadding = 16;

/// The lengths F4Jumble's inverse accepts (ZIP 316 revision 0, "Jumbling").
const int _f4Min = 48;
const int _f4Max = 4194368;

Never _invalid(String message) => raise(SplitCode.addressInvalid, message);

/// Answers §8.6 for [text], or refuses with `address_invalid`.
///
/// The text is taken exactly: no whitespace is trimmed, and Bech32 and
/// Bech32m are read in lower case only.
ParsedAddress parseAddress(String text) {
  final ua = _bech32Decode(text, _unifiedHrps, _bech32mConst);
  if (ua != null) {
    final receivers = _unifiedReceivers(ua.hrp, ua.bytes) ??
        _invalid('Not a revision 0 Unified Address ZIP 316 admits');
    return ParsedAddress(
      network: _unifiedHrps[ua.hrp]!,
      kind: AddressKind.unified,
      receivers: List.unmodifiable(receivers),
      canReceiveMemo: receivers.contains(typecodeSapling) ||
          receivers.contains(typecodeOrchard),
    );
  }
  final sapling = _bech32Decode(text, _saplingHrps, _bech32Const);
  if (sapling != null) {
    if (sapling.bytes.length != 43) {
      _invalid('A Sapling address carries 43 bytes');
    }
    return ParsedAddress(
      network: _saplingHrps[sapling.hrp]!,
      kind: AddressKind.sapling,
      receivers: const [],
      canReceiveMemo: true,
    );
  }
  final tex = _bech32Decode(text, _texHrps, _bech32mConst);
  if (tex != null) {
    if (tex.bytes.length != 20) _invalid('A TEX address carries 20 bytes');
    return ParsedAddress(
      network: _texHrps[tex.hrp]!,
      kind: AddressKind.tex,
      receivers: const [],
      canReceiveMemo: false,
    );
  }
  final payload = _base58Check(text);
  if (payload == null || payload.length != 22) _invalid('Not a Zcash address');
  final lead = (payload[0] << 8) | payload[1];
  final (network, kind) = switch (lead) {
    0x1CB8 => (AddressNetwork.main, AddressKind.p2pkh),
    0x1CBD => (AddressNetwork.main, AddressKind.p2sh),
    0x1D25 => (AddressNetwork.test, AddressKind.p2pkh),
    0x1CBA => (AddressNetwork.test, AddressKind.p2sh),
    _ => _invalid('Not a Zcash transparent address'),
  };
  return ParsedAddress(
    network: network,
    kind: kind,
    receivers: const [],
    canReceiveMemo: false,
  );
}

int _polymod(Iterable<int> values) {
  const gen = [0x3B6A57B2, 0x26508E6D, 0x1EA119FA, 0x3D4233DD, 0x2A1462B3];
  var chk = 1;
  for (final v in values) {
    final top = chk >> 25;
    chk = ((chk & 0x1FFFFFF) << 5) ^ v;
    for (var i = 0; i < 5; i++) {
      if ((top >> i) & 1 == 1) chk ^= gen[i];
    }
  }
  return chk;
}

/// The prefix and bytes of a string under one of [hrps], or null.
///
/// Lower case only, since the prefixes and the alphabet are compared as
/// written: ZIP 173 has encoders write lower case, and the decoder wallets use
/// (zcash_address 0.13) refuses upper. The 5-bit groups regroup into bytes,
/// and the leftover bits number at most four and are zero (ZIP 173,
/// "Decoding").
({String hrp, List<int> bytes})? _bech32Decode(
    String text, Map<String, AddressNetwork> hrps, int constant) {
  final sep = text.lastIndexOf('1');
  if (sep < 1) return null;
  final hrp = text.substring(0, sep);
  final data = text.substring(sep + 1);
  if (!hrps.containsKey(hrp) || data.length < 6) return null;
  final values = <int>[];
  for (final unit in data.codeUnits) {
    final i = _charset.indexOf(String.fromCharCode(unit));
    if (i < 0) return null;
    values.add(i);
  }
  final expanded = [
    for (final c in hrp.codeUnits) c >> 5,
    0,
    for (final c in hrp.codeUnits) c & 31,
  ];
  if (_polymod([...expanded, ...values]) != constant) return null;
  var acc = 0;
  var bits = 0;
  final out = <int>[];
  for (final v in values.sublist(0, values.length - 6)) {
    acc = ((acc << 5) | v) & 0xFFF;
    bits += 5;
    if (bits >= 8) {
      bits -= 8;
      out.add((acc >> bits) & 0xFF);
    }
  }
  if (bits > 4 || acc & ((1 << bits) - 1) != 0) return null;
  return (hrp: hrp, bytes: out);
}

/// The payload of a Base58Check string, its four checksum bytes removed.
List<int>? _base58Check(String text) {
  // Big-endian base-256 digits, grown one base-58 digit at a time.
  final body = <int>[];
  for (final unit in text.codeUnits) {
    var carry = _base58.indexOf(String.fromCharCode(unit));
    if (carry < 0) return null;
    for (var i = body.length - 1; i >= 0; i--) {
      carry += body[i] * 58;
      body[i] = carry & 0xFF;
      carry >>= 8;
    }
    while (carry > 0) {
      body.insert(0, carry & 0xFF);
      carry >>= 8;
    }
    // Lead, payload and checksum are 26 bytes; anything longer is not a
    // transparent address, and stopping here keeps the work linear.
    if (body.length > _base58CheckBytes) return null;
  }
  var zeros = 0;
  while (zeros < text.length && text.codeUnitAt(zeros) == 0x31) {
    zeros++;
  }
  final raw = [...List.filled(zeros, 0), ...body];
  if (raw.length < 4) return null;
  final payload = raw.sublist(0, raw.length - 4);
  final check = sha256(sha256(payload));
  for (var i = 0; i < 4; i++) {
    if (check[i] != raw[raw.length - 4 + i]) return null;
  }
  return payload;
}

/// A canonical compactSize at [at], with the offset past it.
(int, int)? _compactSize(List<int> raw, int at) {
  if (at >= raw.length) return null;
  final flag = raw[at];
  if (flag < 253) {
    return flag > _maxCompactSize ? null : (flag, at + 1);
  }
  final (size, least) = switch (flag) {
    253 => (2, 253),
    254 => (4, 0x10000),
    _ => (8, 0x100000000),
  };
  if (at + 1 + size > raw.length) return null;
  // Read as unsigned: an eight-byte value with its top bit set is past the
  // limit, and must not read as a negative number that is under it.
  var value = 0;
  for (var i = size - 1; i >= 0; i--) {
    if (value > _maxCompactSize) return null;
    value = (value << 8) | raw[at + 1 + i];
  }
  if (value < least || value > _maxCompactSize) return null;
  return (value, at + 1 + size);
}

/// The typecodes of a revision 0 Unified Address, in encoding order.
List<int>? _unifiedReceivers(String hrp, List<int> jumbled) {
  if (jumbled.length < _f4Min || jumbled.length > _f4Max) return null;
  final raw = _f4JumbleInv(jumbled);
  final padding = [
    ...ascii.encode(hrp),
    ...List.filled(_uaPadding - hrp.length, 0),
  ];
  final bodyEnd = raw.length - _uaPadding;
  for (var i = 0; i < _uaPadding; i++) {
    if (raw[bodyEnd + i] != padding[i]) return null;
  }
  final body = raw.sublist(0, bodyEnd);
  var at = 0;
  final codes = <int>[];
  while (at < body.length) {
    final typecode = _compactSize(body, at);
    if (typecode == null) return null;
    final length = _compactSize(body, typecode.$2);
    if (length == null) return null;
    final end = length.$2 + length.$1;
    if (end > body.length) return null;
    final expected = switch (typecode.$1) {
      typecodeP2pkh || typecodeP2sh => 20,
      typecodeSapling || typecodeOrchard => 43,
      _ => null,
    };
    if (expected != null && expected != length.$1) return null;
    codes.add(typecode.$1);
    at = end;
  }
  // Ascending, so one comparison refuses a repeat and a reordering alike.
  for (var i = 1; i < codes.length; i++) {
    if (codes[i] <= codes[i - 1]) return null;
  }
  if (codes.contains(typecodeP2pkh) && codes.contains(typecodeP2sh)) {
    return null;
  }
  // Revision 0 admits no MUST-understand metadata.
  if (codes.any((c) => c >= 0xE0 && c <= 0xFC)) return null;
  if (!codes.contains(typecodeSapling) && !codes.contains(typecodeOrchard)) {
    return null;
  }
  return codes;
}

/// ZIP 316 F4Jumble⁻¹. `m.length` is within [_f4Min, _f4Max].
List<int> _f4JumbleInv(List<int> m) {
  final leftLen = m.length ~/ 2 < 64 ? m.length ~/ 2 : 64;
  final left = m.sublist(0, leftLen);
  final right = m.sublist(leftLen);

  void hRound(int i) {
    final personal = [...ascii.encode('UA_F4Jumble_H'), i, 0, 0];
    final hash = _blake2b(right, left.length, personal);
    for (var k = 0; k < left.length; k++) {
      left[k] ^= hash[k];
    }
  }

  void gRound(int i) {
    for (var j = 0; j * 64 < right.length; j++) {
      final personal = [
        ...ascii.encode('UA_F4Jumble_G'),
        i,
        j & 0xFF,
        j >> 8,
      ];
      final hash = _blake2b(left, 64, personal);
      for (var k = 0; k < 64 && j * 64 + k < right.length; k++) {
        right[j * 64 + k] ^= hash[k];
      }
    }
  }

  hRound(1);
  gRound(1);
  hRound(0);
  gRound(0);
  return [...left, ...right];
}

const List<int> _iv = [
  0x6A09E667F3BCC908,
  0xBB67AE8584CAA73B,
  0x3C6EF372FE94F82B,
  0xA54FF53A5F1D36F1,
  0x510E527FADE682D1,
  0x9B05688C2B3E6C1F,
  0x1F83D9ABFB41BD6B,
  0x5BE0CD19137E2179,
];

const List<List<int>> _sigma = [
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

int _word(List<int> bytes, int at) {
  var w = 0;
  for (var i = 7; i >= 0; i--) {
    w = (w << 8) | bytes[at + i];
  }
  return w;
}

int _rotr(int x, int n) => (x >>> n) | (x << (64 - n));

/// Unkeyed BLAKE2b (RFC 7693) with a 16-byte personalization, [outLen] bytes
/// of output, 1 to 64. Words are the VM's 64-bit integers, whose arithmetic
/// wraps.
List<int> _blake2b(List<int> message, int outLen, List<int> personal) {
  final h = List<int>.of(_iv);
  h[0] ^= 0x01010000 ^ outLen;
  h[6] ^= _word(personal, 0);
  h[7] ^= _word(personal, 8);

  final blocks = message.isEmpty ? 1 : (message.length + 127) ~/ 128;
  var counter = 0;
  final block = List<int>.filled(128, 0);
  final m = List<int>.filled(16, 0);
  final v = List<int>.filled(16, 0);

  void g(int a, int b, int c, int d, int x, int y) {
    v[a] = v[a] + v[b] + x;
    v[d] = _rotr(v[d] ^ v[a], 32);
    v[c] = v[c] + v[d];
    v[b] = _rotr(v[b] ^ v[c], 24);
    v[a] = v[a] + v[b] + y;
    v[d] = _rotr(v[d] ^ v[a], 16);
    v[c] = v[c] + v[d];
    v[b] = _rotr(v[b] ^ v[c], 63);
  }

  for (var b = 0; b < blocks; b++) {
    final start = b * 128;
    final end = start + 128 < message.length ? start + 128 : message.length;
    block.fillRange(0, 128, 0);
    block.setRange(0, end - start, message, start);
    counter += end - start;
    for (var i = 0; i < 16; i++) {
      m[i] = _word(block, i * 8);
    }
    for (var i = 0; i < 8; i++) {
      v[i] = h[i];
      v[i + 8] = _iv[i];
    }
    v[12] ^= counter;
    if (b + 1 == blocks) v[14] = ~v[14];
    for (final s in _sigma) {
      g(0, 4, 8, 12, m[s[0]], m[s[1]]);
      g(1, 5, 9, 13, m[s[2]], m[s[3]]);
      g(2, 6, 10, 14, m[s[4]], m[s[5]]);
      g(3, 7, 11, 15, m[s[6]], m[s[7]]);
      g(0, 5, 10, 15, m[s[8]], m[s[9]]);
      g(1, 6, 11, 12, m[s[10]], m[s[11]]);
      g(2, 7, 8, 13, m[s[12]], m[s[13]]);
      g(3, 4, 9, 14, m[s[14]], m[s[15]]);
    }
    for (var i = 0; i < 8; i++) {
      h[i] ^= v[i] ^ v[i + 8];
    }
  }
  return [
    for (var i = 0; i < outLen; i++) (h[i ~/ 8] >> (8 * (i % 8))) & 0xFF,
  ];
}
