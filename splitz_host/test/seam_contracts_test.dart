/// The §15 seam checks, against every implementation this package ships and
/// against implementations that each break one rule.
@TestOn('vm')
library;

import 'dart:io';

import 'package:splitz_host/io.dart';
import 'package:splitz_host/splitz_host.dart';
import 'package:test/test.dart';

import 'http_relay_test.dart' show ioClient;
import 'support/process_port.dart';

/// A secret store that raises for a key it never held.
class _RaisesWhenAbsent extends InMemorySecretStore {
  @override
  Future<String?> read(String key) async =>
      await super.read(key) ?? (throw StateError('no such key'));
}

/// A store whose `keys` ignores the prefix it is given.
class _ListsEverything extends InMemoryBillStorage {
  @override
  Future<List<String>> keys(String prefix) => super.keys('');
}

/// A store that loses what it was given past a length.
class _Truncates extends InMemoryBillStorage {
  @override
  Future<void> write(String key, String value) =>
      super.write(key, value.length > 1000 ? value.substring(0, 1000) : value);
}

/// A relay that keeps every push, a repeat included.
class _Duplicates implements SplitsRelay {
  final _held = <String, List<String>>{};

  @override
  Future<void> push(String channel, List<String> blobs) async =>
      _held.putIfAbsent(channel, () => []).addAll(blobs);

  @override
  Future<List<String>> fetch(String channel) async => [...?_held[channel]];
}

/// A price source that invents a figure for anything.
class _Invents implements ZecPrices {
  @override
  Future<int?> minorUnitsPerZec(String currency) async => 0;
}

Iterable<String> _rules(List<SeamFinding> found) => found.map((f) => f.rule);

void main() {
  group('the implementations this package ships keep §15', () {
    test('InMemorySecretStore', () async {
      expect(
        await checkSecretStore(InMemorySecretStore(), runId: 'a'),
        isEmpty,
      );
    });

    test('InMemoryBillStorage', () async {
      expect(
        await checkBillStorage(InMemoryBillStorage(), runId: 'a'),
        isEmpty,
      );
    });

    test('FileBillStorage', () async {
      final dir = await Directory.systemTemp.createTemp('splitz-contract');
      addTearDown(() => dir.delete(recursive: true));
      expect(await checkBillStorage(FileBillStorage(dir), runId: 'a'), isEmpty);
    });

    test('InMemorySplitsRelay', () async {
      expect(
        await checkSplitsRelay(InMemorySplitsRelay(), runId: 'a'),
        isEmpty,
      );
    });

    test('HttpSplitsRelay against tools/relay/server.py', () async {
      final up = await startOnFreePort('../tools/relay/server.py', const []);
      addTearDown(up.process.kill);
      final io = ioClient();
      final relay = HttpSplitsRelay(
        origin: Uri.parse('http://127.0.0.1:${up.port}'),
        post: io.post,
        get: io.get,
      );
      expect(await checkSplitsRelay(relay, runId: 'a'), isEmpty);
    });

    test('FixedZecPrices and NoZecPrices', () async {
      expect(
        await checkZecPrices(const FixedZecPrices({'USD': 138819})),
        isEmpty,
      );
      expect(await checkZecPrices(const NoZecPrices()), isEmpty);
    });

    test('CoinGeckoZecPrices, over its captured answer', () async {
      final prices = CoinGeckoZecPrices(
        origin: Uri.parse('https://api.coingecko.com/api/v3'),
        get: (url) async => File(
          '../tools/contracts/fixtures/coingecko_price.json',
        ).readAsStringSync(),
      );
      expect(await checkZecPrices(prices, priced: 'EUR'), isEmpty);
    });
  });

  group('each rule names the implementation that breaks it', () {
    test('a secret store that raises for an absent key', () async {
      expect(
        _rules(await checkSecretStore(_RaisesWhenAbsent(), runId: 'a')),
        contains('a key never written reads as empty'),
      );
    });

    test('a store whose keys ignore the prefix', () async {
      expect(_rules(await checkBillStorage(_ListsEverything(), runId: 'a')), [
        'keys answers every key under the prefix, and only those',
      ]);
    });

    test('a store that loses a long value', () async {
      expect(
        _rules(await checkBillStorage(_Truncates(), runId: 'a')),
        containsAll(['a value written reads back whole']),
      );
    });

    test('a relay that keeps a repeated push', () async {
      expect(_rules(await checkSplitsRelay(_Duplicates(), runId: 'a')), [
        'pushing a blob again changes nothing',
      ]);
    });

    test('a price source that invents a figure', () async {
      expect(_rules(await checkZecPrices(_Invents())), [
        'a code nobody prices answers empty, not an error',
        'an answer is a positive whole number of minor units an IEEE-754 '
            'double holds exactly',
      ]);
    });
  });
}
