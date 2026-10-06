/// §6.3: what a settlement's covers do not explain, and the refusal when its
/// figures leave the signed 64-bit range.
library;

import 'package:splitz_core/splitz_core.dart';
import 'package:test/test.dart';

void main() {
  const max = 9223372036854775807;
  const min = -9223372036854775808;

  Matcher refusedWith(String code) =>
      throwsA(isA<SplitError>().having((e) => e.code, 'code', code));

  test('an honest settlement with no covers is unexplained whole', () {
    expect(const Settlement('ben', 'ana', 1500).unexplained, 1500);
  });

  test('covers that explain all of it leave nothing', () {
    const s = Settlement('ben', 'ana', 1500, covers: [
      DirectDebt('ben', 'ana', 1000),
      DirectDebt('ben', 'cai', 500)
    ]);
    expect(s.unexplained, 0);
  });

  test('covers summing past the range are refused, not wrapped', () {
    const s = Settlement('ben', 'ana', 1500,
        covers: [DirectDebt('ben', 'ana', max), DirectDebt('ben', 'cai', 1)]);
    expect(() => s.unexplained, refusedWith(SplitCode.amountOverflow));
  });

  test('an amount less its covers past the range is refused', () {
    const s =
        Settlement('ben', 'ana', min, covers: [DirectDebt('ben', 'ana', 1)]);
    expect(() => s.unexplained, refusedWith(SplitCode.amountOverflow));
  });
}
