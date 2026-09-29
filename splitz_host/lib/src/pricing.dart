/// What one ZEC costs, and where that figure comes from.
library;

import 'dart:convert';

import 'package:splitz_core/splitz_core.dart' as splitz;

import 'currencies.dart';
import 'relay.dart' show JsonGet;

/// A source of ZEC prices.
///
/// Injected, like everything else a wallet already has: a wallet showing
/// balances in a currency has a price feed, and a second one here would be a
/// second answer on one screen.
///
/// Specified in SPEC.md §15.6.
abstract interface class ZecPrices {
  /// Minor units of [currency] that one ZEC costs, or null when this source
  /// cannot price that currency.
  ///
  /// Minor units, not a decimal. §7 snapshots the figure onto the bill as an
  /// integer, so rounding it happens once, here, where the source's precision
  /// is known — rather than at every place that reads it.
  ///
  /// Null is an ordinary answer. A bill with no rate is an ordinary bill: there
  /// is no §12 code for unpriced, and nothing should invent a price to avoid
  /// showing that state.
  Future<int?> minorUnitsPerZec(String currency);
}

/// A price this build was told, rather than one it looked up.
///
/// For a demonstration, and for a test that needs the arithmetic to be
/// predictable. It is not a feed and does not pretend to be: what it returns
/// was true whenever somebody typed it.
class FixedZecPrices implements ZecPrices {
  const FixedZecPrices(this._prices);

  /// Currency code to minor units per ZEC.
  final Map<String, int> _prices;

  @override
  Future<int?> minorUnitsPerZec(String currency) async =>
      _prices[currency.toUpperCase()];
}

/// A source that prices nothing.
///
/// The honest default for a build with no feed wired in. Every bill is then
/// unpriced until somebody types a figure, and the screen says so instead of
/// showing a number nothing stands behind.
class NoZecPrices implements ZecPrices {
  const NoZecPrices();

  @override
  Future<int?> minorUnitsPerZec(String currency) async => null;
}

/// A price source that answered with something that is not a price.
///
/// Raised rather than answered as null: null means this source cannot price
/// the currency, and an answer that did not read is a different state — one
/// a person should hear about rather than see as an unpriced bill (§15).
class ZecPriceException implements Exception {
  const ZecPriceException(this.message);

  final String message;

  @override
  String toString() => 'ZecPriceException: $message';
}

/// The largest figure a price may be: 2^53 - 1, so it is exact wherever a
/// binding carries it as a double.
const int maxMinorUnitsPerZec = 9007199254740991;

/// Minor units of [currency] one ZEC costs, read from a CoinGecko
/// `/simple/price?ids=zcash&vs_currencies=…` answer.
///
/// The price is read as the shortest decimal that names it, and scaled by the
/// currency's ISO 4217 exponent in exact integers, rounding halves up — never
/// multiplied as a double, where `1.005 × 100` is `100.49999…`.
///
/// Null when the answer does not price [currency], when the register gives
/// [currency] no exponent (§2.1), and when the price is not positive, rounds
/// to nothing, or exceeds [maxMinorUnitsPerZec]. Throws [ZecPriceException]
/// when [body] is not such an answer at all.
int? priceFromCoinGecko(String body, String currency) {
  final Object? decoded;
  try {
    decoded = jsonDecode(body);
  } on FormatException {
    throw const ZecPriceException('The price answer is not JSON');
  }
  if (decoded is! Map<String, dynamic>) {
    throw const ZecPriceException('The price answer is not an object');
  }
  final zcash = decoded['zcash'];
  if (zcash is! Map<String, dynamic>) {
    throw const ZecPriceException('The price answer names no zcash price');
  }
  final exponent = currencyExponent(currency);
  if (exponent == null) return null;
  final raw = zcash[currency.toLowerCase()];
  if (raw == null) return null;
  if (raw is! num) {
    throw ZecPriceException('The $currency price is not a number');
  }
  if (raw is double && !raw.isFinite) return null;
  final scaled = _scaleExactly(raw.toString(), exponent);
  if (scaled == null ||
      scaled <= BigInt.zero ||
      scaled > BigInt.from(maxMinorUnitsPerZec)) {
    return null;
  }
  return scaled.toInt();
}

/// [decimal] × 10^[exponent], rounded half up, or null for a negative value.
///
/// [decimal] is how Dart writes a number: digits, an optional fraction, an
/// optional `e` exponent.
BigInt? _scaleExactly(String decimal, int exponent) {
  final match = RegExp(
    r'^(-?)([0-9]+)(?:\.([0-9]+))?(?:e([+-]?[0-9]+))?$',
  ).firstMatch(decimal);
  if (match == null) {
    throw ZecPriceException('Not a decimal: "$decimal"');
  }
  if (match.group(1)!.isNotEmpty) return null;
  final fraction = match.group(3) ?? '';
  final digits = BigInt.parse('${match.group(2)}$fraction');
  // value = digits × 10^shift, and the answer is value × 10^exponent.
  final shift = int.parse(match.group(4) ?? '0') - fraction.length + exponent;
  if (shift >= 0) return digits * BigInt.from(10).pow(shift);
  final divisor = BigInt.from(10).pow(-shift);
  final quotient = digits ~/ divisor;
  final remainder = digits - quotient * divisor;
  return remainder * BigInt.two >= divisor ? quotient + BigInt.one : quotient;
}

/// ZEC prices from CoinGecko's `/simple/price` (or a proxy speaking it).
///
/// [origin] is the API root the wallet chose, such as
/// `https://api.coingecko.com/api/v3`: a public host, or one the wallet runs
/// in front of it. Held to the provider's answers by
/// `test/coingecko_contract_test.dart`, and against the live service daily by
/// `tools/contracts/coingecko.py --check`.
///
/// A failed fetch raises: an unreachable feed is not an unpriced currency.
class CoinGeckoZecPrices implements ZecPrices {
  CoinGeckoZecPrices({required this.origin, required JsonGet get}) : _get = get;

  final Uri origin;
  final JsonGet _get;

  /// The request for [currency]'s price.
  Uri requestFor(String currency) => origin.replace(
    path: '${origin.path.replaceFirst(RegExp(r'/+$'), '')}/simple/price',
    queryParameters: {'ids': 'zcash', 'vs_currencies': currency.toLowerCase()},
  );

  @override
  Future<int?> minorUnitsPerZec(String currency) async {
    if (!splitz.isCurrency(currency) || currencyExponent(currency) == null) {
      return null;
    }
    return priceFromCoinGecko(await _get(requestFor(currency)), currency);
  }
}

/// Minor units of [currency] one ZEC costs, read from a Coinbase
/// `/v2/exchange-rates?currency=ZEC` answer.
///
/// The answer is `{"data": {"currency": "ZEC", "rates": {"USD": "1393.12",
/// …}}}`: every rate a decimal string, keyed by upper-case code. The string is
/// scaled by the currency's ISO 4217 exponent in exact integers, rounding
/// halves up, as [priceFromCoinGecko] does.
///
/// Null when the answer does not price [currency], when the register gives
/// [currency] no exponent (§2.1), and when the price is not positive, rounds
/// to nothing, or exceeds [maxMinorUnitsPerZec]. Throws [ZecPriceException]
/// when [body] is not such an answer for ZEC, or a rate is not a decimal
/// string.
int? priceFromCoinbase(String body, String currency) {
  final Object? decoded;
  try {
    decoded = jsonDecode(body);
  } on FormatException {
    throw const ZecPriceException('The price answer is not JSON');
  }
  final data = decoded is Map<String, dynamic> ? decoded['data'] : null;
  if (data is! Map<String, dynamic>) {
    throw const ZecPriceException('The price answer has no data object');
  }
  if (data['currency'] != 'ZEC') {
    throw const ZecPriceException('The price answer is not for ZEC');
  }
  final rates = data['rates'];
  if (rates is! Map<String, dynamic>) {
    throw const ZecPriceException('The price answer has no rates');
  }
  final exponent = currencyExponent(currency);
  if (exponent == null) return null;
  final raw = rates[currency.toUpperCase()];
  if (raw == null) return null;
  if (raw is! String || !RegExp(r'^[0-9]+(\.[0-9]+)?$').hasMatch(raw)) {
    throw ZecPriceException('The $currency rate is not a decimal string');
  }
  final scaled = _scaleExactly(raw, exponent);
  if (scaled == null ||
      scaled <= BigInt.zero ||
      scaled > BigInt.from(maxMinorUnitsPerZec)) {
    return null;
  }
  return scaled.toInt();
}

/// ZEC prices from Coinbase's `/v2/exchange-rates` (or a proxy speaking it).
///
/// [origin] is the host the wallet chose, such as `https://api.coinbase.com`.
/// One request prices every currency, since the base is ZEC. Held to the
/// provider's answers by `test/coinbase_contract_test.dart`, and against the
/// live service daily by `tools/contracts/coinbase.py --check`.
///
/// A failed fetch raises: an unreachable feed is not an unpriced currency.
class CoinbaseZecPrices implements ZecPrices {
  CoinbaseZecPrices({required this.origin, required JsonGet get}) : _get = get;

  final Uri origin;
  final JsonGet _get;

  /// The request for ZEC's rates against every currency.
  Uri get request => origin.replace(
    path: '${origin.path.replaceFirst(RegExp(r'/+$'), '')}/v2/exchange-rates',
    queryParameters: const {'currency': 'ZEC'},
  );

  @override
  Future<int?> minorUnitsPerZec(String currency) async {
    if (!splitz.isCurrency(currency) || currencyExponent(currency) == null) {
      return null;
    }
    return priceFromCoinbase(await _get(request), currency);
  }
}

/// The Binance pair [BinanceZecPrices] reads: ZEC against USDT, taken as US
/// dollars. Binance lists ZEC against stablecoins and crypto only, so no
/// other currency is priced.
const String binanceZecSymbol = 'ZECUSDT';

/// Minor units of USD one ZEC costs, read from a Binance
/// `/api/v3/ticker/price?symbol=ZECUSDT` answer: `{"symbol": "ZECUSDT",
/// "price": "1390.54000000"}`.
///
/// USDT is read as USD, so the figure is as good as that peg. Null for any
/// currency but USD, and when the price is not positive, rounds to nothing, or
/// exceeds [maxMinorUnitsPerZec]. Throws [ZecPriceException] when [body] is
/// not a ticker for [binanceZecSymbol], or its price is not a decimal string.
int? priceFromBinance(String body, String currency) {
  final Object? decoded;
  try {
    decoded = jsonDecode(body);
  } on FormatException {
    throw const ZecPriceException('The price answer is not JSON');
  }
  if (decoded is! Map<String, dynamic> ||
      decoded['symbol'] != binanceZecSymbol) {
    throw const ZecPriceException('The price answer is not the ZEC ticker');
  }
  final raw = decoded['price'];
  if (raw is! String || !RegExp(r'^[0-9]+(\.[0-9]+)?$').hasMatch(raw)) {
    throw const ZecPriceException('The ZEC price is not a decimal string');
  }
  if (currency != 'USD') return null;
  final scaled = _scaleExactly(raw, currencyExponent('USD')!);
  if (scaled == null ||
      scaled <= BigInt.zero ||
      scaled > BigInt.from(maxMinorUnitsPerZec)) {
    return null;
  }
  return scaled.toInt();
}

/// ZEC prices in USD from Binance's `/api/v3/ticker/price` (or a proxy
/// speaking it).
///
/// [origin] is the host the wallet chose, such as
/// `https://data-api.binance.vision`, Binance's market-data host. Any other
/// currency is null without a request. Held to the provider's answers by
/// `test/binance_contract_test.dart`, and against the live service daily by
/// `tools/contracts/binance.py --check`.
///
/// A failed fetch raises: an unreachable feed is not an unpriced currency.
class BinanceZecPrices implements ZecPrices {
  BinanceZecPrices({required this.origin, required JsonGet get}) : _get = get;

  final Uri origin;
  final JsonGet _get;

  /// The request for the ZEC ticker.
  Uri get request => origin.replace(
    path: '${origin.path.replaceFirst(RegExp(r'/+$'), '')}/api/v3/ticker/price',
    queryParameters: const {'symbol': binanceZecSymbol},
  );

  @override
  Future<int?> minorUnitsPerZec(String currency) async {
    if (currency != 'USD') return null;
    return priceFromBinance(await _get(request), currency);
  }
}

/// The first of [sources] that prices a currency.
///
/// Each is asked in order until one answers with a price. One that fails is
/// passed over for the next; only when every source failed does the last
/// failure raise, since an unreachable feed is not an unpriced currency. Null
/// when at least one answered and none priced it.
class FirstZecPrices implements ZecPrices {
  const FirstZecPrices(this.sources);

  final List<ZecPrices> sources;

  @override
  Future<int?> minorUnitsPerZec(String currency) async {
    Object? failure;
    StackTrace? trace;
    var answered = false;
    for (final source in sources) {
      try {
        final price = await source.minorUnitsPerZec(currency);
        if (price != null) return price;
        answered = true;
      } on Object catch (e, s) {
        failure = e;
        trace = s;
      }
    }
    if (!answered && failure != null) {
      Error.throwWithStackTrace(failure, trace!);
    }
    return null;
  }
}
