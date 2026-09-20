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
