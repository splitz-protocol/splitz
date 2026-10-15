/// §9.1's transparent payout, against every case `vectors/address.json` holds.
library;

import 'dart:convert';
import 'dart:io';

import 'package:splitz_core/splitz_core.dart';
import 'package:test/test.dart';

void main() {
  test('an address is transparent exactly when §8.6 decodes it as one', () {
    final doc = jsonDecode(File('../vectors/address.json').readAsStringSync());
    final counts = <bool, int>{};
    for (final c in (doc['cases'] as List).cast<Map<String, dynamic>>()) {
      final answer = c['expect'] as Map<String, dynamic>?;
      final transparent =
          answer != null && {'p2pkh', 'p2sh', 'tex'}.contains(answer['kind']);
      expect(
        isTransparentAddress(c['address'] as String),
        transparent,
        reason: c['name'] as String,
      );
      counts[transparent] = (counts[transparent] ?? 0) + 1;
    }
    expect(counts[true], greaterThan(0));
    expect(counts[false], greaterThan(0));
  });
}
