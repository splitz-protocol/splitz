/// The generated binding, loaded inside Flutter's test harness.
///
/// `tools/ffi/dart.sh` drives the binding under `dart run`. A Flutter app is a
/// different host: the test harness runs on the Flutter tester rather than the
/// standalone VM, and a wallet that cannot open the library there cannot hold
/// the crate at all. This asserts it can, which is the precondition for a
/// Flutter package wrapping `splitz-ffi`.
///
/// The path comes from `--dart-define`, because `flutter test` passes no
/// arguments to the test.
import 'dart:convert';
import 'dart:typed_data';

import 'package:flutter_test/flutter_test.dart';
import 'package:splitz_flutter_consumer/splitz_ffi.dart';

const libraryPath = String.fromEnvironment('SPLITZ_LIBRARY');

/// The facts §15.1 says a wallet owns, for one call.
HostFacts facts(String me, String at, int nonce) => HostFacts(
  me: me,
  payTo: 'u1$me',
  now: at,
  nonce: Uint8List.fromList(List.generate(16, (i) => (nonce + i) & 0xff)),
);

/// The Ed25519 seed a wallet keeps in the platform keychain: 32 bytes,
/// unpadded base64url.
const seed = 'AQIDBAUGBwgJCgsMDQ4PEBESExQVFhcYGRobHB0eHyA';

void main() {
  setUpAll(() {
    expect(
      libraryPath,
      isNotEmpty,
      reason: 'pass --dart-define=SPLITZ_LIBRARY=<path to the cdylib>',
    );
    configureDefaultBindings(libraryPath: libraryPath);
  });

  test('the crate answers through the binding', () {
    final key = identityKeyFromSeed(seed);
    expect(key.length, 43);

    final create = createBillEntry(
      facts('ana', '2026-10-28T19:31:00.000Z', 1),
      'Dinner',
      'EUR',
      'equal',
      key,
      seed,
    );
    expect(create, contains('"kind":"createBill"'));

    final folded = foldEntries(
      facts('ana', '2026-10-28T19:32:00.000Z', 2),
      (jsonDecode(create) as Map)['id'] as String,
      [create],
    );
    expect(folded.bill.name, 'Dinner');
    expect(folded.setAside, isEmpty);
    expect(folded.identities.bound.keys, contains('ana'));
  });

  test('one payer owes half of a priced bill', () {
    final key = identityKeyFromSeed(seed);
    final create = createBillEntry(
      facts('ana', '2026-10-28T19:31:00.000Z', 1),
      'Dinner',
      'EUR',
      'equal',
      key,
      seed,
    );
    // Every other entry is signed on the bill it belongs to (§10.6).
    final billId = (jsonDecode(create) as Map)['id'] as String;
    final log = [
      create,
      joinBillEntry(
        facts('ana', '2026-10-28T19:32:00.000Z', 2),
        billId,
        'Ana',
        'u1ana',
        key,
        seed,
      ),
      joinBillEntry(
        facts('ben', '2026-10-28T19:33:00.000Z', 3),
        billId,
        'Ben',
        'u1ben',
        key,
        seed,
      ),
      addExpenseEntry(
        facts('ana', '2026-10-28T19:34:00.000Z', 4),
        billId,
        'x1',
        'ana',
        9000,
        '{"type":"equal","among":["ana","ben"]}',
        'dinner',
        seed,
      ),
      setRateEntry(
        facts('ana', '2026-10-28T19:35:00.000Z', 5),
        billId,
        'EUR',
        300000,
        'a feed',
        seed,
      ),
    ];
    final merged = mergeEntries(const [], log).entries;
    final owed = obligationOf(
      facts('ben', '2026-10-28T19:36:00.000Z', 6),
      (jsonDecode(log.first) as Map)['id'] as String,
      merged,
      const [],
    );
    expect(owed, isNotNull);
    expect(owed!.settlements.single.amount, 4500);
    expect(owed.request.uri, startsWith('zcash:u1ana'));
    expect(owed.request.withheldMinorUnits, 0);
  });

  test('a refusal crosses as its §12 code', () {
    expect(readScanned('not a bill').refusedCode, isNotNull);
  });
}
