/// ISO 4217 minor-unit exponents, the register SPEC.md §2.1 leaves to a reader.
///
/// Generated from ISO 4217 List One as published by its maintenance agency,
/// https://www.six-group.com/dam/download/financial-information/data-center/iso-currrency/lists/list-one.xml
/// (published 2026-09-17). A code the list gives no minor unit, such as
/// `XAU`, is absent: an amount in it has no scale at which a figure typed in
/// major units means anything, so §2.1 requires a reader to refuse it.
library;

/// Minor units per major unit, as a power of ten, by currency code.
const Map<String, int> iso4217Exponents = {
  'AED': 2,
  'AFN': 2,
  'ALL': 2,
  'AMD': 2,
  'AOA': 2,
  'ARS': 2,
  'AUD': 2,
  'AWG': 2,
  'AZN': 2,
  'BAM': 2,
  'BBD': 2,
  'BDT': 2,
  'BHD': 3,
  'BIF': 0,
  'BMD': 2,
  'BND': 2,
  'BOB': 2,
  'BOV': 2,
  'BRL': 2,
  'BSD': 2,
  'BTN': 2,
  'BWP': 2,
  'BYN': 2,
  'BZD': 2,
  'CAD': 2,
  'CDF': 2,
  'CHE': 2,
  'CHF': 2,
  'CHW': 2,
  'CLF': 4,
  'CLP': 0,
  'CNY': 2,
  'COP': 2,
  'COU': 2,
  'CRC': 2,
  'CUP': 2,
  'CVE': 2,
  'CZK': 2,
  'DJF': 0,
  'DKK': 2,
  'DOP': 2,
  'DZD': 2,
  'EGP': 2,
  'ERN': 2,
  'ETB': 2,
  'EUR': 2,
  'FJD': 2,
  'FKP': 2,
  'GBP': 2,
  'GEL': 2,
  'GHS': 2,
  'GIP': 2,
  'GMD': 2,
  'GNF': 0,
  'GTQ': 2,
  'GYD': 2,
  'HKD': 2,
  'HNL': 2,
  'HTG': 2,
  'HUF': 2,
  'IDR': 2,
  'ILS': 2,
  'INR': 2,
  'IQD': 3,
  'IRR': 2,
  'ISK': 0,
  'JMD': 2,
  'JOD': 3,
  'JPY': 0,
  'KES': 2,
  'KGS': 2,
  'KHR': 2,
  'KMF': 0,
  'KPW': 2,
  'KRW': 0,
  'KWD': 3,
  'KYD': 2,
  'KZT': 2,
  'LAK': 2,
  'LBP': 2,
  'LKR': 2,
  'LRD': 2,
  'LSL': 2,
  'LYD': 3,
  'MAD': 2,
  'MDL': 2,
  'MGA': 2,
  'MKD': 2,
  'MMK': 2,
  'MNT': 2,
  'MOP': 2,
  'MRU': 2,
  'MUR': 2,
  'MVR': 2,
  'MWK': 2,
  'MXN': 2,
  'MXV': 2,
  'MYR': 2,
  'MZN': 2,
  'NAD': 2,
  'NGN': 2,
  'NIO': 2,
  'NOK': 2,
  'NPR': 2,
  'NZD': 2,
  'OMR': 3,
  'PAB': 2,
  'PEN': 2,
  'PGK': 2,
  'PHP': 2,
  'PKR': 2,
  'PLN': 2,
  'PYG': 0,
  'QAR': 2,
  'RON': 2,
  'RSD': 2,
  'RUB': 2,
  'RWF': 0,
  'SAR': 2,
  'SBD': 2,
  'SCR': 2,
  'SDG': 2,
  'SEK': 2,
  'SGD': 2,
  'SHP': 2,
  'SLE': 2,
  'SOS': 2,
  'SRD': 2,
  'SSP': 2,
  'STN': 2,
  'SVC': 2,
  'SYP': 2,
  'SZL': 2,
  'THB': 2,
  'TJS': 2,
  'TMT': 2,
  'TND': 3,
  'TOP': 2,
  'TRY': 2,
  'TTD': 2,
  'TWD': 2,
  'TZS': 2,
  'UAH': 2,
  'UGX': 0,
  'USD': 2,
  'USN': 2,
  'UYI': 0,
  'UYU': 2,
  'UYW': 4,
  'UZS': 2,
  'VED': 2,
  'VES': 2,
  'VND': 0,
  'VUV': 0,
  'WST': 2,
  'XAD': 2,
  'XAF': 0,
  'XCD': 2,
  'XCG': 2,
  'XOF': 0,
  'XPF': 0,
  'YER': 2,
  'ZAR': 2,
  'ZMW': 2,
  'ZWG': 2,
};

/// The exponent [currency] is written with, or null when this register gives
/// it none — which is every code that is not an ISO 4217 currency with a minor
/// unit.
int? currencyExponent(String currency) => iso4217Exponents[currency];

/// The most decimals [parseMinorUnits] reads: a signed 64-bit amount holds 18
/// decimal digits in full. ISO 4217 exponents stop at 4.
const maxParseExponent = 18;

/// [text] a person typed, read as minor units at [exponent] decimals, or null
/// when it is not one.
///
/// Integer arithmetic throughout: the figure is split on its separator and
/// both halves read as whole numbers. Reading a double and multiplying rounds
/// — `0.29 * 100` is not 29 in binary floating point — and that rounding is
/// money. `.` and `,` both separate the fraction, but a `,` followed by
/// exactly three digits is refused: `1,000` is a thousand to one reader and
/// one to another, and a currency with three decimals makes both readings
/// well formed. Only ASCII space, tab, carriage return and line feed are
/// trimmed and only ASCII digits read, so every language reads one string
/// alike. A figure with no digit, more fractional digits than [exponent], or
/// a value past a signed 64-bit integer (§2.2) is refused rather than
/// rounded or wrapped, and so is an [exponent] outside 0..[maxParseExponent].
int? parseMinorUnits(String text, {required int exponent}) {
  if (exponent < 0 || exponent > maxParseExponent) return null;
  const space = {0x20, 0x09, 0x0d, 0x0a};
  var start = 0, end = text.length;
  while (start < end && space.contains(text.codeUnitAt(start))) {
    start++;
  }
  while (end > start && space.contains(text.codeUnitAt(end - 1))) {
    end--;
  }
  final typed = text.substring(start, end);
  if (RegExp(r',[0-9]{3}$').hasMatch(typed)) return null;
  final parts = typed.replaceAll(',', '.').split('.');
  if (parts.length > 2) return null;
  final whole = parts[0];
  final fraction = parts.length == 2 ? parts[1] : '';
  final digits = RegExp(r'^[0-9]*$');
  if (!digits.hasMatch(whole) || !digits.hasMatch(fraction)) return null;
  if (whole.isEmpty && fraction.isEmpty) return null;
  if (fraction.length > exponent) return null;
  final scaled = fraction.padRight(exponent, '0');
  final value =
      BigInt.parse(whole.isEmpty ? '0' : whole) *
          BigInt.from(10).pow(exponent) +
      BigInt.parse(scaled.isEmpty ? '0' : scaled);
  if (value > BigInt.parse('9223372036854775807')) return null;
  return value.toInt();
}

/// [text] read as an amount in [currency] (§2.1): [parseMinorUnits] at the
/// exponent this register gives it, or null when the register gives it none —
/// there is no scale at which a typed figure in it means anything.
int? parseAmountIn(String text, String currency) {
  final exponent = currencyExponent(currency);
  return exponent == null ? null : parseMinorUnits(text, exponent: exponent);
}
