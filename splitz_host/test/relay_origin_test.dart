/// Any relay at `SPLITZ_RELAY_ORIGIN`, held to what `tools/relay/server.py`
/// answers and to §15.5.
///
/// Skipped when the variable is unset. `tools/relay/cloudflare/test.sh` runs
/// it against the Python relay and against the Worker, so the hosted relay is
/// held to the same answers as the one every other lane uses.
@TestOn('vm')
library;

import 'dart:convert';
import 'dart:io';

import 'package:splitz_core/splitz_core.dart' show channelFor;
import 'package:splitz_host/splitz_host.dart';
import 'package:test/test.dart';

import 'http_relay_test.dart' show ioClient;

void main() {
  final origin = Platform.environment['SPLITZ_RELAY_ORIGIN'];
  final skip = origin == null ? 'set SPLITZ_RELAY_ORIGIN to a relay' : null;
  // Unique per run: a hosted relay keeps what earlier runs pushed.
  final run = DateTime.now().microsecondsSinceEpoch.toString();

  HttpSplitsRelay relay() {
    final io = ioClient();
    return HttpSplitsRelay(
      origin: Uri.parse(origin!),
      post: io.post,
      get: io.get,
    );
  }

  Future<(int, Map<String, dynamic>)> raw(
    String method,
    String path, [
    String? body,
  ]) async {
    final client = HttpClient();
    try {
      final request = await client.openUrl(method, Uri.parse('$origin$path'));
      if (body != null) {
        request.headers.contentType = ContentType.json;
        request.write(body);
      }
      final response = await request.close();
      final text = await response.transform(utf8.decoder).join();
      return (response.statusCode, jsonDecode(text) as Map<String, dynamic>);
    } finally {
      client.close(force: true);
    }
  }

  test('keeps §15.5', () async {
    expect(await checkSplitsRelay(relay(), runId: run), isEmpty);
  }, skip: skip);

  test('answers a push and a fetch as server.py does', () async {
    final channel = channelForRun(run, 'answers');
    final work = await relayWorkStamp(channel, ['a', 'b'], relayWorkBase);
    final (pushed, ok) = await raw(
      'POST',
      '/c/$channel',
      jsonEncode({
        'blobs': ['a', 'b'],
        'work': work,
      }),
    );
    expect(pushed, 200);
    expect(ok, {'ok': true});
    final (fetched, held) = await raw('GET', '/c/$channel');
    expect(fetched, 200);
    expect(held, {
      'blobs': ['a', 'b'],
    });
  }, skip: skip);

  test('answers a page from where it is asked to start', () async {
    final channel = channelForRun(run, 'from');
    final work = await relayWorkStamp(channel, ['a', 'b'], relayWorkBase);
    await raw(
      'POST',
      '/c/$channel',
      jsonEncode({
        'blobs': ['a', 'b'],
        'work': work,
      }),
    );
    expect((await raw('GET', '/c/$channel?from=1')).$2, {
      'blobs': ['b'],
    });
    expect((await raw('GET', '/c/$channel?from=9')).$2, {'blobs': <String>[]});
    for (final from in ['x', '-1', '', '1&from=2', '1' * 21]) {
      final (status, refused) = await raw('GET', '/c/$channel?from=$from');
      expect(status, 400, reason: 'from=$from');
      expect(refused, {'error': 'not a page'}, reason: 'from=$from');
    }
  }, skip: skip);

  // Set by tools/relay/cloudflare/test.sh, which starts both local relays
  // with 16-byte pages; a relay of unknown page size skips it.
  final pageBytes = Platform.environment['SPLITZ_RELAY_PAGE_BYTES'];
  test(
    'breaks a channel into 16-byte pages and a fetch reads them all',
    () async {
      final channel = channelForRun(run, 'pages');
      const blobs = ['aaaa', 'bbbbbbbbbb', 'ccccc', 'ddddd', 'e'];
      final work = await relayWorkStamp(channel, blobs, relayWorkBase);
      await raw(
        'POST',
        '/c/$channel',
        jsonEncode({'blobs': blobs, 'work': work}),
      );
      // Each blob costs its characters plus 3: 7, 13, 8, 8 and 4. The third
      // page is exactly 16.
      expect((await raw('GET', '/c/$channel?from=0')).$2, {
        'blobs': ['aaaa'],
        'next': 1,
      });
      expect((await raw('GET', '/c/$channel?from=1')).$2, {
        'blobs': ['bbbbbbbbbb'],
        'next': 2,
      });
      expect((await raw('GET', '/c/$channel?from=2')).$2, {
        'blobs': ['ccccc', 'ddddd'],
        'next': 4,
      });
      expect((await raw('GET', '/c/$channel?from=4')).$2, {
        'blobs': ['e'],
      });
      // Without `from`, every blob in one answer.
      expect((await raw('GET', '/c/$channel')).$2, {'blobs': blobs});
      expect(await relay().fetch(channel), blobs);
    },
    skip: skip ?? (pageBytes == '16' ? null : 'needs 16-byte pages'),
  );

  test('refuses what server.py refuses, with the same status', () async {
    final channel = channelForRun(run, 'refusals');
    expect((await raw('GET', '/c/not-a-digest')).$1, 404);
    expect((await raw('POST', '/c/$channel', '{"blobs": 7}')).$1, 400);
    expect((await raw('POST', '/c/$channel', 'not json')).$1, 400);
    // §15.5: a push with no work, or a stamp that is not a nonce.
    final (status, asked) = await raw('POST', '/c/$channel', '{"blobs":["x"]}');
    expect(status, 428);
    expect(asked, {'error': 'a push carries work', 'work': 16});
    expect(
      (await raw('POST', '/c/$channel', '{"blobs":["x"],"work":"1a"}')).$1,
      400,
    );
    final over = 'x' * (64 * 1024 + 1);
    expect(
      (await raw(
        'POST',
        '/c/$channel',
        jsonEncode({
          'blobs': [over],
        }),
      )).$1,
      413,
    );
    // A refused push stores nothing.
    expect((await raw('GET', '/c/$channel')).$2, {'blobs': <String>[]});
  }, skip: skip);
}

/// A channel no bill has, for [run] and [name].
String channelForRun(String run, String name) =>
    channelFor('splitz-relay-origin-$run-$name');
