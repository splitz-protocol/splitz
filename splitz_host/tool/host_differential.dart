/// Answers `tools/differential/host_generate.py`'s operations.
///
/// One JSON object per line in, one per line out. The Rust host answers the
/// same list, and the runner diffs them: nothing in `vectors/` can cover a
/// curve operation, because a vector cannot carry a private key.
library;

import 'dart:convert';
import 'dart:io';

import 'package:crypto/crypto.dart' as hashing;
import 'package:splitz_core/splitz_core.dart' as protocol;
import 'package:splitz_host/splitz_host.dart';

final signer = SplitsSigner();
final sealing = SplitsSealing();

/// The two implementations word their refusals differently; what has to match
/// is *which* refusal. Both drivers map to this vocabulary.
String tag(Object e) {
  if (e is! SealingException) return 'other';
  const prefixes = {
    'Empty blob': 'empty',
    'Blob is format v': 'version',
    'Blob is too short': 'short',
    'Malformed base64url': 'malformed_b64',
    'Key is ': 'key_length',
    'Could not open': 'auth',
    'Opened blob is not UTF-8': 'not_utf8',
    'Opened blob is not JSON': 'not_json',
    'Opened blob is not an entry': 'not_entry',
    'An entry is not sealable': 'not_sealable',
  };
  for (final entry in prefixes.entries) {
    if (e.message.startsWith(entry.key)) return entry.value;
  }
  return 'unclassified';
}

/// Flips a bit of a blob: 1 in the ciphertext, 2 in the version byte.
String tamperBlob(String blob, int how) {
  final List<int> bytes;
  try {
    bytes = SplitsSigner.decode(blob).toList();
  } on FormatException {
    return blob;
  }
  if (how == 1 && bytes.length > 3) {
    bytes[bytes.length - 3] ^= 0x01;
  } else if (how == 2 && bytes.isNotEmpty) {
    bytes[0] = (bytes[0] + 1) & 0xff;
  }
  return SplitsSigner.encode(bytes);
}

/// Changes one character of a signature, so it is well formed and wrong.
String tamper(String sig) => (sig[0] == 'A' ? 'B' : 'A') + sig.substring(1);

Future<Object?> answer(Map<String, dynamic> op) async {
  switch (op['op'] as String) {
    case 'public_key':
      return signer.publicKeyFromSeed(
        SplitsSigner.decode(op['seed'] as String),
      );
    case 'sign_entry':
      final entry = (op['entry'] as Map).cast<String, dynamic>();
      final String message;
      try {
        message = protocol.signingMessage(entry);
      } on protocol.SplitError catch (e) {
        return {'error': e.code};
      }
      final sign = signer.signerFor(SplitsSigner.decode(op['seed'] as String));
      return {'message': message, 'sig': await sign(utf8.encode(message))};
    case 'verify':
      final entry = (op['entry'] as Map).cast<String, dynamic>();
      final signWith = SplitsSigner.decode(op['signWith'] as String);
      String message;
      try {
        message = protocol.signingMessage(entry);
      } on protocol.SplitError {
        return {'signable': false};
      }
      var sig = await signer.signerFor(signWith)(utf8.encode(message));
      if (op['tamper'] == true) sig = tamper(sig);
      final signed = <String, dynamic>{...entry, 'sig': sig};
      final trueKey = await signer.publicKeyFromSeed(signWith);
      return {
        'signable': true,
        'againstGivenKey': await signer.verifyEntry(
          signed,
          op['key'] as String,
        ),
        'againstTrueKey': await signer.verifyEntry(signed, trueKey),
      };
    case 'identity_seed':
      final viewingKey = op['viewingKey'] as String;
      // Random on both sides when there is no viewing key, so there is
      // nothing to compare.
      if (viewingKey.isEmpty) return null;
      return SplitsSigner.encode(
        hashing.sha256
            .convert(utf8.encode('${SplitsKeys.identityDomain}:$viewingKey'))
            .bytes,
      );
    case 'seal_open':
      final key = op['key'] as String;
      final String blob;
      try {
        blob = await sealing.seal(
          (op['entry'] as Map).cast<String, dynamic>(),
          key,
        );
      } on SealingException catch (e) {
        return {'sealed': false, 'why': tag(e)};
      }
      final presented = tamperBlob(blob, op['tamper'] as int);
      final openWith = (op['openWith'] as String?) ?? key;
      try {
        final entry = await sealing.open(presented, openWith);
        return {'sealed': true, 'blob': blob, 'opened': true, 'entry': entry};
      } on SealingException catch (e) {
        return {'sealed': true, 'blob': blob, 'opened': false, 'why': tag(e)};
      }
    case 'open_raw':
      try {
        final entry = await sealing.open(
          op['blob'] as String,
          op['key'] as String,
        );
        return {'opened': true, 'entry': entry};
      } on SealingException catch (e) {
        return {'opened': false, 'why': tag(e)};
      }
    case 'well_formed_key':
      return SplitsKeys.isWellFormedKey(op['key'] as String);
    case 'b64_round_trip':
      final text = op['text'] as String;
      try {
        final raw = SplitsSigner.decode(text);
        return {'bytes': raw.length, 'reencoded': SplitsSigner.encode(raw)};
      } on FormatException {
        return null;
      }
    default:
      return {'error': 'unknown op ${op['op']}'};
  }
}

Future<void> main() async {
  final out = StringBuffer();
  for (final line
      in await stdin
          .transform(utf8.decoder)
          .transform(const LineSplitter())
          .toList()) {
    if (line.trim().isEmpty) continue;
    final op = (jsonDecode(line) as Map).cast<String, dynamic>();
    out.writeln(jsonEncode(await answer(op)));
  }
  stdout.write(out);
}
