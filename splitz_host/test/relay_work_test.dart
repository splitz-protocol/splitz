/// §15.5's work: the stamp every implementation finds, and a push stamped
/// again when the relay asks for more.
library;

import 'dart:convert';

import 'package:splitz_host/splitz_host.dart';
import 'package:test/test.dart';

const _channel =
    'aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa';

void main() {
  test('the stamp is the one tools/relay/test_server.py pins', () async {
    expect(await relayWorkStamp(_channel, ['x'], 16), '127719');
    expect(await relayWorkStamp(_channel, ['x', 'y'], 16), '103390');
    expect(relayWorkHolds(_channel, ['x'], '127719', 16), isTrue);
    expect(relayWorkHolds(_channel, ['y'], '127719', 16), isFalse);
    // One list of blobs re-split into another is another stamp.
    final joined = await relayWorkStamp(_channel, ['a\nb'], 16);
    expect(relayWorkHolds(_channel, ['a', 'b'], joined, 16), isFalse);
  });

  test('the bits grow with the units a push carries', () {
    expect(
      [
        for (final c in [0, 1, 1024, 1025, 2048, 2049, 65536, 65537])
          relayWorkBits(c),
      ],
      [16, 16, 16, 17, 17, 18, 22, 23],
    );
    expect(relayWorkBits(1, base: 20), 20);
    expect(leadingZeroBits([0, 0x0f]), 12);
    expect(leadingZeroBits([0x80]), 0);
    expect(leadingZeroBits([0, 0]), 16);
  });

  group('a push', () {
    /// A relay asking [asks] bits of base, answering as tools/relay/server.py
    /// does: 428 with the base when the work is short or missing.
    (HttpSplitsRelay, List<Map<String, dynamic>>) relay(List<int> asks) {
      final seen = <Map<String, dynamic>>[];
      var call = 0;
      return (
        HttpSplitsRelay(
          origin: Uri.parse('https://relay.test'),
          post: (url, body) async {
            final push = jsonDecode(body) as Map<String, dynamic>;
            seen.add(push);
            final base = asks[call < asks.length ? call++ : asks.length - 1];
            final blobs = (push['blobs'] as List).cast<String>();
            final chars = blobs.fold<int>(0, (n, b) => n + b.length);
            final work = push['work'];
            if (work is String &&
                relayWorkHolds(
                  url.pathSegments.last,
                  blobs,
                  work,
                  relayWorkBits(chars, base: base),
                )) {
              return '{"ok":true}';
            }
            return jsonEncode({'error': 'not enough work', 'work': base});
          },
          get: (_) async => '{"blobs":[]}',
        ),
        seen,
      );
    }

    test('carries a stamp at the base', () async {
      final (r, seen) = relay([16]);
      await r.push(_channel, ['x']);
      expect(seen.single, {
        'blobs': ['x'],
        'work': '127719',
      });
    });

    test('is stamped once more at the base a refusal asks', () async {
      final (r, seen) = relay([18, 18]);
      await r.push(_channel, ['x']);
      expect(seen, hasLength(2));
      expect(
        relayWorkHolds(_channel, ['x'], seen.last['work'] as String, 18),
        isTrue,
      );
    });

    test('a second refusal stands', () async {
      final (r, seen) = relay([18, 40]);
      await expectLater(
        r.push(_channel, ['x']),
        throwsA(isA<SplitsRelayException>()),
      );
      expect(seen, hasLength(2));
    });

    test('an ask past 40 bits is not worked for', () async {
      final (r, seen) = relay([41]);
      await expectLater(
        r.push(_channel, ['x']),
        throwsA(isA<SplitsRelayException>()),
      );
      expect(seen, hasLength(1));
    });
  });
}
