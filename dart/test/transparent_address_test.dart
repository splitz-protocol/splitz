/// §9.1's transparent payout, against every case `vectors/address.json` holds.
library;

import 'dart:convert';
import 'dart:io';

import 'package:splitz_core/splitz_core.dart';
import 'package:test/test.dart';

/// A Unified Address with a P2PKH receiver and one of a typecode §8.6 does
/// not know: refused by §8.6, paid transparently by a wallet that reads it.
const _transparentOnlyUnified =
    'u1e67lmu7s8etun46sslprau5u5z7v2ffd6p6wwp9xxxga86m9skaslmkptgyxlnewgxvym66jke59fp0np8rhmy0lrrympgy2hm33h0tjkxmg2mc4dcks57s9wm';

/// The vectors §8.6 refuses whose text is still a Unified Address string:
/// a valid BIP 350 checksum, computed apart from this library, over data
/// whose padding bits are zero.
const _refusedUnified = {
  'an_orchard_receiver_one_byte_short',
  'a_p2pkh_receiver_one_byte_long',
  'unified_with_only_p2pkh',
  'unified_with_only_p2sh',
  'unified_with_only_an_unknown_typecode',
  'unified_with_p2pkh_and_an_unknown_typecode',
  'unified_with_p2pkh_and_p2sh',
  'unified_with_a_duplicate_typecode',
  'unified_with_receivers_out_of_order',
  'unified_with_a_must_understand_typecode',
  'unified_with_the_last_must_understand_typecode',
  'unified_with_a_typecode_past_the_limit',
  'unified_with_a_non_canonical_typecode',
  'unified_with_a_non_canonical_length',
  'unified_with_a_length_past_the_end',
  'unified_with_a_trailing_byte',
  'unified_with_testnet_padding',
  'unified_with_empty_padding',
  'unified_shorter_than_f4jumble_accepts',
  'unified_with_no_items',
  'unified_with_nothing_to_unjumble',
};

void main() {
  test(
      'every vector: transparent when §8.6 decodes it so, or refuses a '
      'Unified Address string', () {
    final doc = jsonDecode(File('../vectors/address.json').readAsStringSync());
    final counts = <bool, int>{};
    for (final c in (doc['cases'] as List).cast<Map<String, dynamic>>()) {
      final answer = c['expect'] as Map<String, dynamic>?;
      final transparent = answer == null
          ? _refusedUnified.contains(c['name'])
          : {'p2pkh', 'p2sh', 'tex'}.contains(answer['kind']);
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

  test('a Unified Address §8.6 refuses is not presented as private', () {
    expect(
      () => parseAddress(_transparentOnlyUnified),
      throwsA(isA<SplitError>()),
    );
    expect(isTransparentAddress(_transparentOnlyUnified), isTrue);
  });

  test('no address at all, and one that fails its checksum, is not', () {
    expect(isTransparentAddress(''), isFalse);
    expect(isTransparentAddress('not an address'), isFalse);
    final last = _transparentOnlyUnified.length - 1;
    final flipped = _transparentOnlyUnified.replaceRange(
      last,
      null,
      _transparentOnlyUnified[last] == 'm' ? 'n' : 'm',
    );
    expect(isTransparentAddress(flipped), isFalse);
  });
}
