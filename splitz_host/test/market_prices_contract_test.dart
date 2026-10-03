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
        'https://data-api.binance.vision/api/v3/ticker/price?symbol=ZECUSDC',
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

  group('two markets held to each other', () {
    Future<int?> agreeing(_Source a, _Source b, [String currency = 'USD']) =>
        AgreeingZecPrices(a, b).minorUnitsPerZec(currency);

    test('two markets that agree give the second one\'s figure', () async {
      const prices = AgreeingZecPrices(
        FixedZecPrices({'USD': 138819}),
        FixedZecPrices({'USD': 138905, 'EUR': 122241}),
      );
      expect(await prices.minorUnitsPerZec('USD'), 138905);
      expect(await prices.minorUnitsPerZec('EUR'), 122241);
      expect(await prices.minorUnitsPerZec('GBP'), isNull);
    });

    test('two markets that disagree give no price', () async {
      // (152701 - 138819) x 10000 = 138820000 > 138819 x 200 = 27763800:
      // 1000 bp apart, past the 200 allowed.
      expect(await agreeing(_Source(138819), _Source(152701)), isNull);
      expect(await agreeing(_Source(152701), _Source(138819)), isNull);
    });

    test('the tolerance is inclusive, in basis points of the lower', () async {
      // 10000 x 1.02 = 10200: exactly 200 bp, then one past it.
      expect(await agreeing(_Source(10000), _Source(10200)), 10200);
      expect(await agreeing(_Source(10200), _Source(10000)), 10000);
      expect(await agreeing(_Source(10000), _Source(10201)), isNull);
      expect(await agreeing(_Source(10201), _Source(10000)), isNull);
    });

    test('a tolerance the caller chose is the one applied', () async {
      final a = _Source(10000);
      final b = _Source(10201);
      expect(
        await AgreeingZecPrices(a, b, toleranceBp: 201).minorUnitsPerZec('USD'),
        10201,
      );
      expect(
        await AgreeingZecPrices(a, b, toleranceBp: 0).minorUnitsPerZec('USD'),
        isNull,
      );
      expect(
        await AgreeingZecPrices(
          _Source(7),
          _Source(7),
          toleranceBp: 0,
        ).minorUnitsPerZec('USD'),
        7,
      );
    });

    test('figures at the 2^53 - 1 bound do not overflow', () async {
      const top = 9007199254740991;
      expect(await agreeing(_Source(top), _Source(top)), top);
      expect(await agreeing(_Source(top ~/ 2), _Source(top)), isNull);
    });

    test('one market answering stands alone', () async {
      expect(await agreeing(_Source(null), _Source(138905)), 138905);
      expect(await agreeing(_Source(138819), _Source(null)), 138819);
      expect(
        await agreeing(_Source(null, fails: true), _Source(138905)),
        138905,
      );
      expect(
        await agreeing(_Source(138819), _Source(null, fails: true)),
        138819,
      );
    });

    test('a figure that is not positive is not an answer', () async {
      expect(await agreeing(_Source(0), _Source(138905)), 138905);
      expect(await agreeing(_Source(-5), _Source(138905)), 138905);
      expect(await agreeing(_Source(138819), _Source(0)), 138819);
      expect(await agreeing(_Source(0), _Source(0)), isNull);
    });

    test('markets that cannot be reached leave the bill unpriced', () async {
      expect(
        await agreeing(
          _Source(null, fails: true),
          _Source(null, fails: true),
          'EUR',
        ),
        isNull,
      );
    });

    test('both markets are asked every time', () async {
      final a = _Source(100);
      final b = _Source(200);
      await agreeing(a, b);
      expect((a.asked, b.asked), (1, 1));
    });

    test('agreedPrice refuses a negative tolerance', () {
      expect(() => agreedPrice(1, 1, -1), throwsArgumentError);
    });

    test('keeps §15.6', () async {
      const prices = AgreeingZecPrices(
        FixedZecPrices({'USD': 138819}),
        FixedZecPrices({'EUR': 122241}),
      );
      expect(await checkZecPrices(prices), isEmpty);
      expect(await checkZecPrices(prices, priced: 'EUR'), isEmpty);
    });
  });

  test('a rate five percent or more from the live price is warned about', () {
    for (final (rate, live, off) in [
      (105, 100, 5),
      (104, 100, 4),
      (95, 100, -5),
      (96, 100, -4),
      (1, 3, -66),
    ]) {
      expect(ratePercentOff(rate, live), off, reason: '$rate $live');
    }
    expect(ratePercentOff(100, 0), isNull);
    expect(ratePercentOff(9223372036854775807, 1), isNull);
    expect(rateFarFromLive(105, 100) && rateFarFromLive(95, 100), isTrue);
    expect(rateFarFromLive(104, 100) || rateFarFromLive(96, 100), isFalse);
    expect(rateFarFromLive(100, 0), isFalse);
  });
}
