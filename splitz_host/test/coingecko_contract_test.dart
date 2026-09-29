/// The CoinGecko price source held to the provider's real answers.
///
/// `tools/contracts/coingecko_cases.json` pairs answers — one captured from
/// the live service, the rest built in its shape — with what they must read
/// as, computed by `tools/contracts/coingecko.py` in exact decimals.
@TestOn('vm')
library;

import 'dart:convert';
import 'dart:io';

import 'package:splitz_host/splitz_host.dart';
import 'package:test/test.dart';

import 'support/oneclick_contract.dart' show contractDir;

void main() {
  final doc =
      jsonDecode(File('$contractDir/coingecko_cases.json').readAsStringSync())
          as Map<String, dynamic>;
  final cases = (doc['cases'] as List).cast<Map<String, dynamic>>();

  test('the file declares its own count', () {
    expect(cases, hasLength(doc['count']));
  });

  for (final c in cases) {
    test('coingecko: ${c['name']}', () {
      final body = c['body'] as String;
      final currency = c['currency'] as String;
      if (c.containsKey('error')) {
        expect(
          () => priceFromCoinGecko(body, currency),
          throwsA(isA<ZecPriceException>()),
        );
      } else {
        expect(priceFromCoinGecko(body, currency), c['expect']);
      }
    });
  }

  group('the client', () {
    test(
      'asks for one currency, lower-cased, under the origin it was given',
      () async {
        Uri? asked;
        final prices = CoinGeckoZecPrices(
          origin: Uri.parse('https://api.coingecko.com/api/v3/'),
          get: (url) async {
            asked = url;
            return File(
              '$contractDir/fixtures/coingecko_price.json',
            ).readAsStringSync();
          },
        );
        expect(await prices.minorUnitsPerZec('EUR'), 122241);
        expect(
          asked.toString(),
          'https://api.coingecko.com/api/v3/simple/price'
          '?ids=zcash&vs_currencies=eur',
        );
      },
    );

    test('does not ask for a currency it cannot scale', () async {
      var asked = 0;
      final prices = CoinGeckoZecPrices(
        origin: Uri.parse('https://api.coingecko.com/api/v3'),
        get: (url) async {
          asked++;
          return '{"zcash":{}}';
        },
      );
      expect(await prices.minorUnitsPerZec('XAU'), isNull);
      expect(await prices.minorUnitsPerZec('usd'), isNull);
      expect(asked, 0);
    });

    test('a fetch that fails raises rather than reading as unpriced', () async {
      final prices = CoinGeckoZecPrices(
        origin: Uri.parse('https://api.coingecko.com/api/v3'),
        get: (url) async => throw const SocketException('unreachable'),
      );
      await expectLater(
        prices.minorUnitsPerZec('USD'),
        throwsA(isA<SocketException>()),
      );
    });
  });
}
