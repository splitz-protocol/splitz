/// The work a relay push carries in place of anything that names its sender
/// (SPEC.md §15.5).
library;

import 'dart:convert';

import 'package:crypto/crypto.dart' as crypto;

/// The fewest leading zero bits a push's stamp needs, before its size and the
/// relay's load add to it.
const int relayWorkBase = 16;

/// The largest base a relay asks for: [relayWorkBase] and 8 bits more as its
/// store fills (§15.5). An ask above it is refused rather than worked for.
const int relayWorkMaxBase = relayWorkBase + 8;

/// The characters of blob one unit of work pays for.
const int relayWorkUnitChars = 1024;

/// The leading zero bits a push of [chars] blob characters needs at a relay
/// asking [base]: one more bit for every doubling of the units it carries,
/// rounded up, so the work grows with what it stores.
int relayWorkBits(int chars, {int base = relayWorkBase}) {
  final units = chars <= 0
      ? 1
      : (chars + relayWorkUnitChars - 1) ~/ relayWorkUnitChars;
  return base + (units - 1).bitLength;
}

/// The hex SHA-256 of every blob's own SHA-256, in order: fixed-length
/// pieces, so no other list of blobs hashes to it.
String _blobsDigest(List<String> blobs) => crypto.sha256.convert([
  for (final b in blobs) ...crypto.sha256.convert(utf8.encode(b)).bytes,
]).toString();

/// The bytes a stamp [nonce] is hashed with for [blobs] on [channel]: the
/// domain, the channel, [_blobsDigest] and the nonce, separated by `|`.
List<int> _message(String channel, List<String> blobs, String nonce) =>
    utf8.encode('splitz-relay-work-v1|$channel|${_blobsDigest(blobs)}|$nonce');

/// The leading zero bits of [digest].
int leadingZeroBits(List<int> digest) {
  var bits = 0;
  for (final byte in digest) {
    if (byte == 0) {
      bits += 8;
      continue;
    }
    return bits + 8 - byte.bitLength;
  }
  return bits;
}

/// Whether [nonce] stamps [blobs] on [channel] with at least [bits] leading
/// zero bits of SHA-256.
bool relayWorkHolds(
  String channel,
  List<String> blobs,
  String nonce,
  int bits,
) =>
    leadingZeroBits(
      crypto.sha256.convert(_message(channel, blobs, nonce)).bytes,
    ) >=
    bits;

/// The first decimal nonce from zero that stamps [blobs] on [channel] with
/// [bits] leading zero bits. It hands the event loop back every 4096 tries,
/// so a screen waiting on a push still draws.
Future<String> relayWorkStamp(
  String channel,
  List<String> blobs,
  int bits,
) async {
  final prefix = utf8.encode(
    'splitz-relay-work-v1|$channel|${_blobsDigest(blobs)}|',
  );
  for (var n = 0; ; n++) {
    final digest = crypto.sha256.convert([...prefix, ...utf8.encode('$n')]);
    if (leadingZeroBits(digest.bytes) >= bits) return '$n';
    if (n % 4096 == 4095) await Future<void>.delayed(Duration.zero);
  }
}
