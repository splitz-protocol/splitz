// Properties the corpus cannot state.
//
// A vector fixes one input against one output. These hold over many inputs,
// and they are the ones two devices disagreeing about would show a person
// different money for the same history.

import 'dart:convert';
import 'dart:io';
import 'dart:math';

import 'package:splitz/splitz.dart';
import 'package:test/test.dart';

List<Map<String, dynamic>> _logOf(String caseName) {
  final doc = jsonDecode(File('../vectors/delta.json').readAsStringSync())
      as Map<String, dynamic>;
  final c = (doc['cases'] as List)
      .cast<Map<String, dynamic>>()
      .firstWhere((c) => c['name'] == caseName);
  return (c['log'] as List).cast<Map<String, dynamic>>();
}

void main() {
  group('deltaFor (§14.5)', () {
    test('the square is a function of the entry set, not of arrival order',
        () {
      // Two devices holding one history, having received it in different
      // orders, must produce the same square. §10.2 orders the log; a delta
      // that skipped that would hand a peer two different URIs for one set.
      final entries = _logOf('a_peer_who_has_nothing_gets_every_entry');
      final base = deltaFor(entries, const {}) as DeltaSquare;
      final rng = Random(7);
      for (var i = 0; i < 50; i++) {
        final shuffled = [...entries]..shuffle(rng);
        final got = deltaFor(shuffled, const {});
        expect((got as DeltaSquare).uri, base.uri,
            reason: 'shuffle $i produced a different square');
        expect(got.entryCount, base.entryCount);
      }

      // A probe where every input gives one answer is broken rather than
      // conclusive: a different entry set must differ.
      final fewer =
          deltaFor(entries.sublist(0, entries.length - 1), const {});
      expect((fewer as DeltaSquare).uri, isNot(base.uri));
    });

    test('what a peer already holds is never sent again', () {
      final entries = _logOf('a_peer_who_has_nothing_gets_every_entry');
      final ids = [for (final e in entries) e['id'] as String];
      for (var take = 0; take <= ids.length; take++) {
        final held = ids.take(take).toSet();
        final got = deltaFor(entries, held);
        final expected = ids.length - take;
        expect(got.entryCount, expected,
            reason: 'holding $take of ${ids.length}');
        if (expected == 0) {
          expect(got, isA<NothingMissing>());
        } else {
          final body = utf8.decode(base64Url.decode(base64Url.normalize(
              (got as DeltaSquare).uri.substring(deltaPrefix.length))));
          final log = (jsonDecode(body) as Map)['log'] as List;
          final sent = {for (final e in log) (e as Map)['id'] as String};
          expect(sent.intersection(held), isEmpty,
              reason: 'the delta re-sent an entry the peer already holds');
        }
      }
    });
  });
}
