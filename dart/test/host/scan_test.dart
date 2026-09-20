import 'dart:convert';
import 'dart:io';

import 'package:test/test.dart';
import 'package:splitz_core/host.dart';

/// A `splitz1:` payload carrying [body] verbatim, however malformed.
String payload(Map<String, dynamic> body) =>
    'splitz1:${base64Url.encode(utf8.encode(jsonEncode(body))).replaceAll('=', '')}';

void main() {
  test('an invite member that is not a string is refused, not thrown', () {
    // §11.2 carries the invite verbatim and validates nothing inside it, so
    // every member is whatever a peer wrote. A camera is pointed at this.
    for (final bad in <Object?>[
      5,
      true,
      <String>[],
      <String, dynamic>{},
      null
    ]) {
      final text = payload(<String, dynamic>{
        'v': 1,
        'log': <Object?>[],
        'invite': <String, dynamic>{'v': 1, 'b': bad, 'k': 'Kk'},
      });
      final scanned = readScan(text);
      expect(scanned, isA<ScannedBill>(),
          reason: 'b=$bad must read as a bill with no usable invite');
      expect((scanned as ScannedBill).invite, isNull,
          reason: 'b=$bad is not an invite');
    }
  });

  test('a key that is not a string is refused, not thrown', () {
    for (final bad in <Object?>[7, false, <String>[]]) {
      final text = payload(<String, dynamic>{
        'v': 1,
        'log': <Object?>[],
        'invite': <String, dynamic>{'v': 1, 'b': 'Ab3', 'k': bad},
      });
      final scanned = readScan(text);
      expect((scanned as ScannedBill).invite, isNull);
    }
  });

  group('the protocol corpus, through readScan', () {
    final doc =
        jsonDecode(File('test/host/fixtures/payload.json').readAsStringSync())
            as Map<String, dynamic>;
    final cases = (doc['cases'] as List).cast<Map<String, dynamic>>();

    test('every case is classified, none throws', () {
      expect(cases, isNotEmpty, reason: 'an empty corpus proves nothing');
      var bills = 0, refused = 0;
      for (final c in cases) {
        final name = c['name'] as String;
        late final Scanned scanned;
        expect(
            () => scanned = readScan(c['payload'] as String), returnsNormally,
            reason: '$name must be refused rather than thrown');
        switch (scanned) {
          case ScannedBill():
            bills++;
            expect(c.containsKey('error'), isFalse,
                reason: '$name: the corpus refuses this, readScan accepted it');
          case ScanRefused(:final code):
            refused++;
            expect(code, isNotEmpty, reason: '$name: a refusal names a code');
          case ScannedInvite():
            fail('$name: a payload is not an invite');
        }
      }
      // A probe where every case lands in one bucket is broken, not
      // conclusive.
      expect(bills, greaterThan(0));
      expect(refused, greaterThan(0));
    });

    test('a damaged payload is refused as a payload, not as an invite', () {
      var checked = 0, fellThrough = 0;
      for (final c in cases) {
        final expected = c['error'] as String?;
        if (expected == null) continue;
        final text = c['payload'] as String;
        final scanned = readScan(text);
        expect(scanned, isA<ScanRefused>(), reason: c['name'] as String);
        final code = (scanned as ScanRefused).code;
        final trimmed = text.trim();
        if (trimmed.contains('splitz1:') || trimmed.contains('splitzd1:')) {
          expect(code, expected,
              reason: '${c['name']}: this claims to be a payload, so the '
                  'code a person is shown names what is wrong with it');
          checked++;
        } else {
          // Never claimed to be a payload, so it is answered as the invite it
          // also is not.
          expect(code, isNotEmpty);
          fellThrough++;
        }
      }
      expect(checked, greaterThan(0), reason: 'no payload case was reached');
      expect(fellThrough, greaterThan(0),
          reason: 'no fall-through case was reached, so this proves one rule');
    });
  });
}
