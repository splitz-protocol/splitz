/// The Coinbase and Binance price sources held to the providers' real
/// answers, and the order [FirstZecPrices] asks them in.
///
/// `tools/contracts/coinbase_cases.json` and `binance_cases.json` pair
/// answers — one captured from each live service, the rest built in its
/// shape — with what they must read as, computed by `tools/contracts/
/// coinbase.py` and `binance.py` in exact decimals.
@TestOn('vm')
library;

import 'dart:convert';
import 'dart:io';

import 'package:splitz_host/splitz_host.dart';
import 'package:test/test.dart';

import 'support/oneclick_contract.dart' show contractDir;

void _cases(String file, int? Function(String, String) read) {
  final doc =
      jsonDecode(File('$contractDir/$file').readAsStringSync())
          as Map<String, dynamic>;
  final cases = (doc['cases'] as List).cast<Map<String, dynamic>>();

  test('$file declares its own count', () {
    expect(cases, hasLength(doc['count']));
  });

  for (final c in cases) {
    test('$file: ${c['name']}', () {
      final body = c['body'] as String;
      final currency = c['currency'] as String;
      if (c.containsKey('error')) {
        expect(() => read(body, currency), throwsA(isA<ZecPriceException>()));
      } else {
        expect(read(body, currency), c['expect']);
      }
    });
  }
}

/// A source answering [price], or failing when [fails].
class _Source implements ZecPrices {
  _Source(this.price, {this.fails = false});

  final int? price;
  final bool fails;
  int asked = 0;

  @override
  Future<int?> minorUnitsPerZec(String currency) async {
    asked++;
    if (fails) throw const ZecPriceException('unreachable');
    return price;
  }
}

void main() {
  _cases('coinbase_cases.json', priceFromCoinbase);
  _cases('binance_cases.json', priceFromBinance);

  group('the Coinbase client', () {
    test('asks once for ZEC against everything, under its origin', () async {
      final asked = <Uri>[];
      final prices = CoinbaseZecPrices(
        origin: Uri.parse('https://api.coinbase.com/'),
        get: (url) async {
          asked.add(url);
          return File(
            '$contractDir/fixtures/coinbase_rates.json',
          ).readAsStringSync();
        },
      );
      expect(await prices.minorUnitsPerZec('KES'), isPositive);
      expect(asked.map((u) => u.toString()), [
        'https://api.coinbase.com/v2/exchange-rates?currency=ZEC',
      ]);
    });

    test('does not ask for a currency it cannot scale', () async {
      var asked = 0;
      final prices = CoinbaseZecPrices(
        origin: Uri.parse('https://api.coinbase.com'),
        get: (url) async {
          asked++;
          return '{}';
        },
      );
      expect(await prices.minorUnitsPerZec('XAU'), isNull);
      expect(await prices.minorUnitsPerZec('usd'), isNull);
      expect(asked, 0);
    });
  });

  group('the Binance client', () {
    test('asks for the ZEC ticker, and prices USD only', () async {
      final asked = <Uri>[];
      final prices = BinanceZecPrices(
        origin: Uri.parse('https://data-api.binance.vision/'),
        get: (url) async {
          asked.add(url);
          return File(
            '$contractDir/fixtures/binance_ticker.json',
          ).readAsStringSync();
        },
      );
      expect(await prices.minorUnitsPerZec('USD'), isPositive);
      expect(await prices.minorUnitsPerZec('EUR'), isNull);
      expect(asked.map((u) => u.toString()), [
        'https://data-api.binance.vision/api/v3/ticker/price?symbol=ZECUSDT',
      ]);
    });

    test('a fetch that fails raises rather than reading as unpriced', () async {
      final prices = BinanceZecPrices(
        origin: Uri.parse('https://data-api.binance.vision'),
        get: (url) async => throw const SocketException('unreachable'),
      );
      await expectLater(
        prices.minorUnitsPerZec('USD'),
        throwsA(isA<SocketException>()),
      );
    });
  });

  group('the first source that prices it', () {
    test('a source that fails is passed over for the next', () async {
      final down = _Source(null, fails: true);
      final up = _Source(139028);
      expect(await FirstZecPrices([down, up]).minorUnitsPerZec('USD'), 139028);
      expect((down.asked, up.asked), (1, 1));
    });

    test('an earlier price stops the asking', () async {
      final first = _Source(100);
      final second = _Source(200);
      expect(
        await FirstZecPrices([first, second]).minorUnitsPerZec('USD'),
        100,
      );
      expect(second.asked, 0);
    });

    test('unpriced by some and failed by the rest is unpriced', () async {
      expect(
        await FirstZecPrices([
          _Source(null),
          _Source(null, fails: true),
        ]).minorUnitsPerZec('KES'),
        isNull,
      );
    });

    test('every source failing raises', () async {
      await expectLater(
        FirstZecPrices([
          _Source(null, fails: true),
          _Source(null, fails: true),
        ]).minorUnitsPerZec('USD'),
        throwsA(isA<ZecPriceException>()),
      );
    });
  });
}
