/// §15.5's pages, from the client's side: a fetch reads every page, and an
/// answer that does not follow on from the one before is refused.
library;

import 'dart:convert';

import 'package:splitz_host/splitz_host.dart';
import 'package:test/test.dart';

final _origin = Uri.parse('https://relay.example');
const _channel =
    '0000000000000000000000000000000000000000000000000000000000000000';

/// A relay whose answers [answer] writes, recording each URL it was asked.
(HttpSplitsRelay, List<Uri>) _relay(String Function(Uri url) answer) {
  final asked = <Uri>[];
  final relay = HttpSplitsRelay(
    origin: _origin,
    post: (url, body) async => '{"ok":true}',
    get: (url) async {
      asked.add(url);
      return answer(url);
    },
  );
  return (relay, asked);
}

int _from(Uri url) => int.parse(url.queryParameters['from']!);

/// §15.5's page of [blobs] from [start], at [pageBytes].
Map<String, Object> _page(List<String> blobs, int start, int pageBytes) {
  final taken = <String>[];
  var size = 0;
  var end = start;
  while (end < blobs.length) {
    final cost = blobs[end].length + 3;
    if (taken.isNotEmpty && size + cost > pageBytes) break;
    taken.add(blobs[end]);
    size += cost;
    end++;
  }
  return {'blobs': taken, if (end < blobs.length) 'next': end};
}

void main() {
  test('a page is addressed by its first blob', () {
    expect(
      HttpSplitsRelay.pageUrl(_origin, _channel, 127).toString(),
      'https://relay.example/c/$_channel?from=127',
    );
    expect(
      HttpSplitsRelay.pageUrl(
        Uri.parse('https://relay.example/base'),
        _channel,
        0,
      ).toString(),
      'https://relay.example/base/c/$_channel?from=0',
    );
    expect(
      () => HttpSplitsRelay.pageUrl(
        Uri.parse('https://relay.example?x=1'),
        _channel,
        0,
      ),
      throwsA(
        isA<SplitsRelayException>().having(
          (e) => e.isTransient,
          'transient',
          false,
        ),
      ),
    );
  });

  test('a fetch reads every page, in order', () async {
    final blobs = [for (var i = 0; i < 9; i++) 'blob$i' * (i + 1)];
    final (relay, asked) = _relay(
      (url) => jsonEncode(_page(blobs, _from(url), 40)),
    );
    expect(await relay.fetch(_channel), blobs);
    expect(asked.length, greaterThan(2));
    expect(_from(asked.first), 0);
  });

  test('a relay that ignores the query is read in one answer', () async {
    final (relay, asked) = _relay(
      (url) => jsonEncode({
        'blobs': ['a', 'b', 'c'],
      }),
    );
    expect(await relay.fetch(_channel), ['a', 'b', 'c']);
    expect(asked, hasLength(1));
  });

  group('an answer that does not follow on is refused', () {
    for (final (name, answer) in [
      (
        'next skips blobs',
        {
          'blobs': ['a'],
          'next': 5,
        },
      ),
      (
        'next repeats the page',
        {
          'blobs': ['a'],
          'next': 0,
        },
      ),
      ('next on an empty page', {'blobs': <String>[], 'next': 0}),
      (
        'next that is not a number',
        {
          'blobs': ['a'],
          'next': '1',
        },
      ),
    ]) {
      test(name, () {
        expect(
          () => HttpSplitsRelay.fetchPage(jsonEncode(answer), 0),
          throwsA(
            isA<SplitsRelayException>().having(
              (e) => e.isTransient,
              'transient',
              true,
            ),
          ),
        );
      });
    }
  });

  test('the last page, and a page past the end, carry no next', () {
    final last = HttpSplitsRelay.fetchPage('{"blobs":["c"]}', 2);
    expect(last.blobs, ['c']);
    expect(last.next, isNull);
    final past = HttpSplitsRelay.fetchPage('{"blobs":[]}', 9);
    expect(past.blobs, isEmpty);
    expect(past.next, isNull);
  });

  test('a page that follows on is read, and a non-string is dropped', () {
    final page = HttpSplitsRelay.fetchPage(
      '{"blobs":["a",7,"b"],"next":13}',
      10,
    );
    expect(page.blobs, ['a', 'b']);
    expect(page.next, 13);
  });

  test('a relay that never stops answering pages is refused', () async {
    final (relay, asked) = _relay(
      (url) => jsonEncode({
        'blobs': ['x${_from(url)}'],
        'next': _from(url) + 1,
      }),
    );
    await expectLater(
      relay.fetch(_channel),
      throwsA(isA<SplitsRelayException>()),
    );
    expect(asked, hasLength(HttpSplitsRelay.maxPages));
  });

  test('a channel of the most pages a relay holds is read whole', () async {
    final (relay, asked) = _relay(
      (url) => jsonEncode({
        'blobs': ['x${_from(url)}'],
        if (_from(url) + 1 < HttpSplitsRelay.maxPages) 'next': _from(url) + 1,
      }),
    );
    expect(await relay.fetch(_channel), hasLength(HttpSplitsRelay.maxPages));
    expect(asked, hasLength(HttpSplitsRelay.maxPages));
  });
}
