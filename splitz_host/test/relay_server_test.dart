@TestOn('vm')
library;

import 'dart:convert';
import 'dart:io';

import 'package:splitz_host/splitz_host.dart';
import 'package:test/test.dart';

/// Runs `tools/relay/server.py` and drives it through the real client.
///
/// A relay is the only piece of §11.3 nobody here writes twice: the wallet
/// speaks `HttpSplitsRelay` and a server answers it. Testing the client
/// against a server written in the same file would prove they agree with each
/// other, so this runs the server this repository ships, as a process, over a
/// socket.
Future<({Process process, int port})> _relay([
  List<String> extra = const [],
]) async {
  final port = 39300 + DateTime.now().microsecond % 2000;
  final process = await Process.start('python3', [
    '../tools/relay/server.py',
    '--port',
    '$port',
    ...extra,
  ]);
  final client = HttpClient();
  for (var i = 0; i < 100; i++) {
    try {
      final request = await client.getUrl(
        Uri.parse('http://127.0.0.1:$port/c/${'0' * 64}'),
      );
      await (await request.close()).drain<void>();
      return (process: process, port: port);
    } on Object {
      await Future<void>.delayed(const Duration(milliseconds: 50));
    }
  }
  process.kill();
  throw StateError('the relay did not come up on $port');
}

({JsonPost post, JsonGet get}) _io() {
  final client = HttpClient();
  return (
    post: (Uri url, String body) async {
      final request = await client.postUrl(url);
      request.headers.contentType = ContentType.json;
      request.write(body);
      final response = await request.close();
      return response.transform(utf8.decoder).join();
    },
    get: (Uri url) async {
      final request = await client.getUrl(url);
      final response = await request.close();
      return response.transform(utf8.decoder).join();
    },
  );
}

void main() {
  late Process process;
  late HttpSplitsRelay relay;

  setUpAll(() async {
    final up = await _relay();
    process = up.process;
    final io = _io();
    relay = HttpSplitsRelay(
      origin: Uri.parse('http://127.0.0.1:${up.port}'),
      post: io.post,
      get: io.get,
    );
  });

  tearDownAll(() => process.kill());

  final channel = 'a' * 64;

  test('what one device pushes, another fetches', () async {
    expect(await relay.fetch(channel), isEmpty);
    await relay.push(channel, ['blob-one', 'blob-two']);
    expect(await relay.fetch(channel), ['blob-one', 'blob-two']);
  });

  test('a push that repeats itself adds nothing', () async {
    final before = await relay.fetch(channel);
    await relay.push(channel, ['blob-one']);
    expect(
      await relay.fetch(channel),
      before,
      reason:
          'sync retries, and a relay that grew each time would '
          'punish a flaky connection',
    );
  });

  test('channels do not see each other', () async {
    expect(await relay.fetch('b' * 64), isEmpty);
  });

  test('the server keeps the blob bound the client keeps', () async {
    // The client refuses this before sending, so reach past it: a bound only
    // one side keeps is not a bound.
    final io = _io();
    final body = await io.post(
      Uri.parse('${relay.origin}/c/$channel'),
      jsonEncode({
        'blobs': ['x' * (HttpSplitsRelay.maxBlobChars + 1)],
      }),
    );
    expect(jsonDecode(body)['ok'], isNot(true));
    expect(await relay.fetch(channel), isNot(contains(startsWith('xxx'))));
  });

  test('a path that is not a channel is refused', () async {
    final io = _io();
    final body = await io.get(
      Uri.parse(
        'http://127.0.0.1:'
        '${relay.origin.port}/c/not-a-digest',
      ),
    );
    expect(jsonDecode(body)['blobs'], isNull);
  });

  group('bounded, for a relay strangers can reach', () {
    late Process small;
    late Uri origin;

    setUpAll(() async {
      final up = await _relay(['--max-body', '1000', '--max-held', '30']);
      small = up.process;
      origin = Uri.parse('http://127.0.0.1:${up.port}/c/${'c' * 64}');
    });

    tearDownAll(() => small.kill());

    Future<(int, String)> post(String body, {bool chunked = false}) async {
      final client = HttpClient();
      final request = await client.postUrl(origin);
      if (chunked) {
        request.headers.chunkedTransferEncoding = true;
      } else {
        request.contentLength = utf8.encode(body).length;
      }
      request.write(body);
      final response = await request.close();
      final text = await response.transform(utf8.decoder).join();
      client.close(force: true);
      return (response.statusCode, text);
    }

    String push(List<String> blobs) => jsonEncode({'blobs': blobs});

    test('a push under both bounds is stored', () async {
      expect((await post(push(['0123456789']))).$1, 200);
      expect((await post(push(['abcdefghij']))).$1, 200);
    });

    test(
      'a push that would cross what the relay holds is refused whole',
      () async {
        final (status, _) = await post(push(['ABCDEFGHIJK', 'z']));
        expect(status, 507);
        final (_, held) = await post(push(['0123456789']));
        expect(
          jsonDecode(held)['ok'],
          isTrue,
          reason: 'a repeat adds nothing, so it still fits',
        );
      },
    );

    test(
      'a body over the request bound is refused, however it is framed',
      () async {
        final big = push(['x' * 2000]);
        expect((await post(big)).$1, 413);
        expect((await post(big, chunked: true)).$1, 413);
      },
    );
  });
}
