/// Fiat to zatoshi and back (SPEC.md §7).
///
/// The rate is part of the bill's shared state, snapshotted rather than looked
/// up per device: six people applying six live rates to one dinner compute six
/// different amounts and the bill never closes.
library;

import 'errors.dart';
import 'money.dart';

/// One ZEC, in zatoshi.
const int zatoshiPerZec = 100000000;

/// The largest amount §7.1 can convert, being `maxAmount ~/ zatoshiPerZec`.
///
/// Stated so every implementation refuses the same inputs rather than each
/// taking its own widest type's limit.
const int maxConvertibleMinorUnits = 92233720368;

/// How the last zatoshi is decided when the division leaves a remainder.
enum RateRounding {
  /// Settlement amounts default to this. A debt rounded down leaves dust
  /// behind and the bill never quite closes.
  up,
  down,
  nearest,
}

/// The price of one ZEC, in a currency's minor units.
class ExchangeRate {
  const ExchangeRate({
    required this.currency,
    required this.minorUnitsPerZec,
    required this.at,
    this.source,
  });

  final String currency;
  final int minorUnitsPerZec;
  final String at;
  final String? source;
}

/// Converts [minorUnits] to zatoshi at [rate].
///
/// [amountCurrency], when given, is the currency the amount states; it must be
/// the one the rate prices.
int fiatToZatoshi(
  int minorUnits,
  ExchangeRate rate, {
  String? amountCurrency,
  RateRounding rounding = RateRounding.up,
}) {
  checkCurrency(rate.currency);
  if (amountCurrency != null) {
    checkCurrency(amountCurrency);
    if (amountCurrency != rate.currency) {
      raise(SplitCode.rateCurrencyMismatch,
          'A rate in ${rate.currency} does not price $amountCurrency');
    }
  }
  if (rate.minorUnitsPerZec <= 0) {
    raise(SplitCode.rateNotPositive,
        'A ZEC costs more than nothing, got ${rate.minorUnitsPerZec}');
  }
  if (minorUnits < 0) {
    raise(SplitCode.negativeAmount, 'An amount of $minorUnits is negative');
  }
  if (minorUnits > maxConvertibleMinorUnits) {
    raise(SplitCode.rateAmountTooLarge,
        'Converting $minorUnits overflows a 64-bit integer');
  }

  // The multiplication precedes the division so a small amount at a high ZEC
  // price does not collapse to zero.
  final numerator = minorUnits * zatoshiPerZec;
  final quotient = numerator ~/ rate.minorUnitsPerZec;
  final remainder = numerator % rate.minorUnitsPerZec;
  if (remainder == 0) return quotient;

  switch (rounding) {
    case RateRounding.up:
      return quotient + 1;
    case RateRounding.down:
      return quotient;
    case RateRounding.nearest:
      // Written as a comparison rather than remainder * 2, which wraps at
      // 2^62 and then rounds the wrong way. Both sides are bounded by the
      // rate, so neither can exceed what it already contains.
      return remainder >= rate.minorUnitsPerZec - remainder
          ? quotient + 1
          : quotient;
  }
}

/// Converts [zatoshi] to minor units at [rate], rounding halves up.
///
/// For display only — a label under a number, not a number anyone settles
/// against. Its result must never reach a settlement amount.
int zatoshiToFiat(int zatoshi, ExchangeRate rate) {
  checkCurrency(rate.currency);
  if (rate.minorUnitsPerZec <= 0) {
    raise(SplitCode.rateNotPositive,
        'A ZEC costs more than nothing, got ${rate.minorUnitsPerZec}');
  }
  if (zatoshi < 0) {
    raise(SplitCode.negativeAmount, 'A count of $zatoshi zatoshi is negative');
  }
  final product = checkedMultiply(zatoshi, rate.minorUnitsPerZec);
  final quotient = product ~/ zatoshiPerZec;
  final remainder = product % zatoshiPerZec;
  return remainder * 2 >= zatoshiPerZec ? quotient + 1 : quotient;
}
