/// What one ZEC costs, and where that figure comes from.
library;

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
