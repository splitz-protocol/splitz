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
    final (pushed, ok) = await raw(
      'POST',
      '/c/$channel',
      '{"blobs":["a","b"]}',
    );
    expect(pushed, 200);
    expect(ok, {'ok': true});
    final (fetched, held) = await raw('GET', '/c/$channel');
    expect(fetched, 200);
    expect(held, {
      'blobs': ['a', 'b'],
    });
  }, skip: skip);

  test('refuses what server.py refuses, with the same status', () async {
    final channel = channelForRun(run, 'refusals');
    expect((await raw('GET', '/c/not-a-digest')).$1, 404);
    expect((await raw('POST', '/c/$channel', '{"blobs": 7}')).$1, 400);
    expect((await raw('POST', '/c/$channel', 'not json')).$1, 400);
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
