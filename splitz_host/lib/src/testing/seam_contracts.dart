/// §15's rules, run against a wallet's own implementation of each seam.
///
/// A wallet implements the seams this package declares — a keychain, a store,
/// a relay, a price source — and §15 states what each must do. These run
/// those rules against the real thing and answer what it did that §15 says it
/// must not. An empty answer is the only passing one.
///
/// Each check writes under the [runId] it is given and removes what it wrote,
/// so it can run against a wallet's real store. A relay cannot be emptied, so
/// its check uses a channel no bill has: pass a [runId] unique to the run.
///
/// Not checked here, because nothing in one process can show it: that a value
/// outlives the process that wrote it (§15.3), and that a read raises on a
/// value that is there and cannot be read (§15.4).
library;

import 'package:splitz_core/splitz_core.dart' as splitz;

import '../pricing.dart';
import '../relay.dart';
import '../store.dart';
import '../wallet.dart';

/// One thing a seam did that §15 says it must not.
class SeamFinding {
  const SeamFinding(this.seam, this.rule, this.saw);

  /// `SecretStore`, `BillStorage`, `SplitsRelay` or `ZecPrices`.
  final String seam;

  /// The rule, as §15 states it.
  final String rule;

  /// What the seam did instead.
  final String saw;

  @override
  String toString() => '$seam: $rule — $saw';
}

/// Runs [body], and answers a finding for anything it raised.
Future<void> _run(
  List<SeamFinding> out,
  String seam,
  String rule,
  Future<String?> Function() body,
) async {
  try {
    final saw = await body();
    if (saw != null) out.add(SeamFinding(seam, rule, saw));
  } catch (e) {
    out.add(SeamFinding(seam, rule, 'raised $e'));
  }
}

/// §15.3's rules, against [store].
Future<List<SeamFinding>> checkSecretStore(
  SecretStore store, {
  required String runId,
}) async {
  const seam = 'SecretStore';
  final out = <SeamFinding>[];
  final key = 'splitz-contract/$runId/secret';
  await _run(out, seam, 'a key never written reads as empty', () async {
    final got = await store.read(key);
    return got == null ? null : 'read "$got"';
  });
  await _run(out, seam, 'a value written reads back', () async {
    await store.write(key, 'first');
    final got = await store.read(key);
    return got == 'first' ? null : 'read "$got"';
  });
  await _run(out, seam, 'a value written again replaces the first', () async {
    await store.write(key, 'second');
    final got = await store.read(key);
    return got == 'second' ? null : 'read "$got"';
  });
  await _run(out, seam, 'a deleted key reads as empty', () async {
    await store.delete(key);
    final got = await store.read(key);
    return got == null ? null : 'read "$got"';
  });
  await _run(
    out,
    seam,
    'deleting a key that is not there is not an error',
    () async {
      await store.delete(key);
      return null;
    },
  );
  return out;
}

/// §15.4's rules, against [storage].
Future<List<SeamFinding>> checkBillStorage(
  BillStorage storage, {
  required String runId,
}) async {
  const seam = 'BillStorage';
  final out = <SeamFinding>[];
  final prefix = 'splitz-contract/$runId/';
  final a = '${prefix}a';
  final b = '${prefix}b';
  final sibling = 'splitz-contract/$runId-sibling/a';
  // Past what a short-string path holds, with a line break and characters
  // outside ASCII: an entry log is all three.
  final long = '${'é' * 40000}\n{"a":1}';
  try {
    await _run(out, seam, 'a key never written reads as empty', () async {
      final got = await storage.read(a);
      return got == null ? null : 'read ${got.length} characters';
    });
    await _run(out, seam, 'a value written reads back whole', () async {
      await storage.write(a, long);
      final got = await storage.read(a);
      return got == long ? null : 'read ${got?.length} characters';
    });
    await _run(
      out,
      seam,
      'keys answers every key under the prefix, and only '
      'those',
      () async {
        await storage.write(b, 'b');
        await storage.write(sibling, 'sibling');
        final keys = splitz.sortedUtf8(await storage.keys(prefix));
        return keys.length == 2 && keys[0] == a && keys[1] == b
            ? null
            : 'answered $keys';
      },
    );
    await _run(out, seam, 'a sweep leaves every finished write', () async {
      final removed = await storage.sweepUnfinishedWrites();
      if (removed < 0) return 'reported $removed removed';
      final got = await storage.read(a);
      return got == long ? null : 'read ${got?.length} characters after it';
    });
    await _run(
      out,
      seam,
      'a deleted key reads as empty and is not listed',
      () async {
        await storage.delete(b);
        final got = await storage.read(b);
        final keys = await storage.keys(prefix);
        return got == null && !keys.contains(b)
            ? null
            : 'read "$got", listed $keys';
      },
    );
  } finally {
    for (final key in [a, b, sibling]) {
      try {
        await storage.delete(key);
      } catch (_) {
        // Cleaning up what the check wrote; a failure here was reported above.
      }
    }
  }
  return out;
}

/// §15.5's rules, against [relay], on a channel derived from [runId].
Future<List<SeamFinding>> checkSplitsRelay(
  SplitsRelay relay, {
  required String runId,
}) async {
  const seam = 'SplitsRelay';
  final out = <SeamFinding>[];
  final channel = splitz.channelFor('splitz-contract-$runId');
  final other = splitz.channelFor('splitz-contract-$runId-other');
  final one = 'contract-$runId-one';
  final two = 'contract-$runId-two';
  await _run(
    out,
    seam,
    'a channel nothing was pushed to answers empty',
    () async {
      final got = await relay.fetch(channel);
      return got.isEmpty ? null : 'answered ${got.length} blob(s)';
    },
  );
  await _run(out, seam, 'fetch answers every blob pushed', () async {
    await relay.push(channel, [one, two]);
    final got = await relay.fetch(channel);
    return got.contains(one) && got.contains(two) ? null : 'answered $got';
  });
  await _run(out, seam, 'pushing a blob again changes nothing', () async {
    await relay.push(channel, [one]);
    final got = await relay.fetch(channel);
    final copies = got.where((b) => b == one).length;
    return copies == 1 && got.length == 2 ? null : 'answered $got';
  });
  await _run(out, seam, 'another channel holds none of them', () async {
    final got = await relay.fetch(other);
    return got.isEmpty ? null : 'answered $got';
  });
  return out;
}

/// §15.6's rules, against [prices]. [priced] is a currency the source is
/// expected to price; its answer may still be empty.
Future<List<SeamFinding>> checkZecPrices(
  ZecPrices prices, {
  String priced = 'USD',
}) async {
  const seam = 'ZecPrices';
  final out = <SeamFinding>[];
  await _run(
    out,
    seam,
    'a code nobody prices answers empty, not an error',
    () async {
      final got = await prices.minorUnitsPerZec('ZZZ');
      return got == null ? null : 'answered $got';
    },
  );
  await _run(
    out,
    seam,
    'an answer is a positive whole number of minor units '
    'an IEEE-754 double holds exactly',
    () async {
      final got = await prices.minorUnitsPerZec(priced);
      return got == null || (got > 0 && got <= maxMinorUnitsPerZec)
          ? null
          : 'answered $got';
    },
  );
  return out;
}
