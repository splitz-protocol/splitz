import 'package:splitz_host/splitz_host.dart';
import 'package:test/test.dart';

/// One table, pinned in both host packages: what a typed figure reads as.
const _cases = <(String, int, int?)>[
  ('12.34', 2, 1234),
  ('12,34', 2, 1234),
  ('0.29', 2, 29),
  (' 5 ', 2, 500),
  ('\t5\n', 2, 500),
  ('5.', 2, 500),
  ('.5', 2, 50),
  ('007', 2, 700),
  ('1,00', 2, 100),
  ('1,000', 3, null),
  ('1,000', 2, null),
  ('1.000', 3, 1000),
  ('.', 2, null),
  ('', 2, null),
  ('  ', 2, null),
  ('1.234', 2, null),
  ('-1', 2, null),
  ('+1', 2, null),
  ('1e3', 2, null),
  ('1.2.3', 2, null),
  ('\u{661}\u{662}', 2, null),
  ('\u{a0}5', 2, null),
  ('\u{ff15}', 2, null),
  ('92233720368547758.07', 2, 9223372036854775807),
  ('92233720368547758.08', 2, null),
  ('9223372036854775807', 0, 9223372036854775807),
  ('9223372036854775808', 0, null),
  ('99999999999999999999999', 0, null),
  ('7', 0, 7),
  ('7.', 0, 7),
  ('7.0', 0, null),
  ('1.5', 3, 1500),
  ('0', 18, 0),
  ('1', 18, 1000000000000000000),
  ('0', 19, null),
  ('0', 39, null),
];

void main() {
  test('a typed figure is read in integers or refused', () {
    for (final (text, exponent, want) in _cases) {
      expect(
        parseMinorUnits(text, exponent: exponent),
        want,
        reason: '"$text" at $exponent',
      );
    }
  });

  test(
    'an amount is read at its currency\'s exponent, and not in one with none',
    () {
      expect(parseAmountIn('12.34', 'EUR'), 1234);
      expect(parseAmountIn('1234', 'JPY'), 1234);
      expect(parseAmountIn('1.234', 'KWD'), 1234);
      expect(parseAmountIn('12.3', 'JPY'), isNull);
      expect(parseAmountIn('12', 'XAU'), isNull);
      expect(parseAmountIn('12', 'eur'), isNull);
    },
  );

  test('a refund reads with its sign, and only one sign', () {
    expect(parseSignedAmountIn('-30.00', 'EUR'), -3000);
    expect(parseSignedAmountIn('  -30.00', 'EUR'), -3000);
    expect(parseSignedAmountIn('30.00', 'EUR'), 3000);
    expect(parseSignedAmountIn('-0', 'EUR'), 0);
    for (final bad in [
      '--3',
      '- 3',
      '-',
      '+3',
      '3-',
      '-1,000',
      '-9223372036854775808',
    ]) {
      expect(parseSignedAmountIn(bad, 'EUR'), isNull, reason: bad);
    }
    // At the bound and one past it, in minor units.
    expect(
      parseSignedAmountIn('-92233720368547758.07', 'EUR'),
      -9223372036854775807,
    );
    expect(parseSignedAmountIn('-92233720368547758.08', 'EUR'), isNull);
    expect(parseSignedAmountIn('-1', 'XAU'), isNull);
  });
}
