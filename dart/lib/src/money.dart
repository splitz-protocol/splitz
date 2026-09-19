/// Exact integer money (SPEC.md §2).
///
/// An amount is a signed integer count of the smallest indivisible unit of one
/// currency. No amount is a floating point value at any point in its life.
library;

import 'errors.dart';

/// The largest amount this protocol admits, and the largest a signed 64-bit
/// integer holds (SPEC.md §2.2).
///
/// Stated as a constant rather than taken from the host's widest type so that
/// every implementation accepts and refuses the same inputs. On the Dart VM an
/// `int` is exactly this wide; under `dart2js` it is an IEEE-754 double, which
/// is why [tools/web-target] asserts this library does not compile to
/// JavaScript.
const int maxAmount = 9223372036854775807;

/// The most negative signed 64-bit integer. It has no positive counterpart, so
/// taking its magnitude is the identity (SPEC.md §3, step 3).
const int minAmount = -9223372036854775808;

/// Whether [code] is an ISO 4217 alpha-3 code in upper case (SPEC.md §2.1).
///
/// Lower case is refused rather than folded: `usd` and `USD` are one currency
/// to a person and two to the byte order of §2.3.
bool isCurrency(Object? code) {
  if (code is! String || code.length != 3) return false;
  for (var i = 0; i < 3; i++) {
    final c = code.codeUnitAt(i);
    if (c < 0x41 || c > 0x5A) return false;
  }
  return true;
}

/// Refuses [code] with `bill_bad_currency` unless it is a currency.
void checkCurrency(Object? code) {
  if (!isCurrency(code)) {
    raise(SplitCode.billBadCurrency,
        'A currency is three upper-case letters, got ${_show(code)}');
  }
}

/// [a] + [b], refused with [code] rather than wrapped (SPEC.md §2.2).
int checkedAdd(int a, int b, [String code = SplitCode.amountOverflow]) {
  final sum = a + b;
  // Wrapping shows up as a sign the operands cannot produce.
  if ((a > 0 && b > 0 && sum < 0) || (a < 0 && b < 0 && sum >= 0)) {
    raise(code, 'Adding $a and $b overflows a 64-bit integer');
  }
  return sum;
}

/// [a] × [b], refused with [code] rather than wrapped (SPEC.md §2.2).
int checkedMultiply(int a, int b, [String code = SplitCode.amountOverflow]) {
  if (a == 0 || b == 0) return 0;
  final product = a * b;
  if (product ~/ b != a ||
      (a == -1 && b == minAmount) ||
      (b == -1 && a == minAmount)) {
    raise(code, 'Multiplying $a by $b overflows a 64-bit integer');
  }
  return product;
}

/// The sum of [values], refused with [code] rather than wrapped.
int checkedSum(Iterable<int> values, [String code = SplitCode.amountOverflow]) {
  var total = 0;
  for (final v in values) {
    total = checkedAdd(total, v, code);
  }
  return total;
}

String _show(Object? value) => value is String ? '"$value"' : '$value';
