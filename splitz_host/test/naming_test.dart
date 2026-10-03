/// What a participant is called on screen (§9.1).
library;

import 'dart:convert';

import 'package:splitz_core/splitz_core.dart' as splitz;
import 'package:splitz_host/splitz_host.dart';
import 'package:test/test.dart';

/// One table, pinned in both host packages.
const _skeletons =
    r'''[["Ana", "ana"], ["ANA", "ana"], ["\u0410na", "ana"], ["\u0430n\u0430", "ana"], ["Ana\u200b", "ana"], ["A\u0301na", "ana"], ["  Ana   Ben ", "ana ben"], ["\uff22en", "ben"], ["\ud835\udc01en", "ben"], ["\ud835\udfcf\ud835\udfd0", "12"], ["\u0392\u03b5\u03bd", "\u03b2ev"], ["\u03a3\u0399\u03a3\u03a5\u03a6\u039f\u03a3", "\u03c3i\u03c3u\u03c6o\u03c3"], ["\u0130stanbul", "istanbul"], ["\u01c0ucy", "lucy"], ["\u0391\u039d\u0391", "ava"], ["\u041e\u043b\u0435\u0433", "o\u043be\u0433"], ["stra\u00dfe", "stra\u00dfe"], ["\u01c5", "\u01c6"], ["Ana\u3000Ben", "ana ben"], ["\ufeffAna", "ana"], ["Ana\udb40\udc41", "ana"]]''';

splitz.Bill _bill(List<(String, String)> people) => splitz.Bill(
  id: 'b',
  name: 'Dinner',
  currency: 'EUR',
  participants: [
    for (final (id, name) in people) splitz.Participant(id: id, name: name),
  ],
);

void main() {
  test('a name folds to what a reader sees', () {
    for (final pair in jsonDecode(_skeletons) as List) {
      final [name as String, skeleton as String] = pair as List;
      expect(nameSkeleton(name), skeleton, reason: name);
    }
  });

  test('only a colliding name is qualified, the organiser by role', () {
    final bill = _bill([
      ('ana-1111111111aaaaaaaa', 'Ana'),
      ('ana-2222222222bbbbbbbb', 'Аna'),
      ('ben', 'Ben'),
    ]);
    expect(bill.displayNameOf('ben'), 'Ben');
    expect(
      bill.displayNameOf(
        'ana-1111111111aaaaaaaa',
        creatorId: 'ana-1111111111aaaaaaaa',
      ),
      'Ana (organiser)',
    );
    expect(bill.displayNameOf('ana-2222222222bbbbbbbb'), 'Аna (…bbbbbbbb)');
    expect(bill.sharedNames, splitz.sortedUtf8(['Ana', 'Аna']));
    expect(bill.displayNameOf('nobody-12345678'), '…5678');
  });

  test('an id copying another\'s last eight is shown whole', () {
    final bill = _bill([('xx-aaaaaaaa', 'Ana'), ('yy-aaaaaaaa', 'Ana')]);
    expect(bill.displayNameOf('yy-aaaaaaaa'), 'Ana (yy-aaaaaaaa)');
  });

  test('a letter written precomposed or decomposed is one letter', () {
    // U+00C1 and A + U+0301 render alike; both fold to a.
    expect(nameSkeleton('\u00C1na'), 'ana');
    expect(nameSkeleton('A\u0301na'), 'ana');
    expect(nameSkeleton('Ren\u00E9e'), nameSkeleton('Renee'));
    expect(nameSkeleton('\u01FAs'), 'as'); // the last in U+00C0..U+0233 used
    // Letters with no decomposition stay themselves.
    expect(nameSkeleton('Stra\u00DFe'), 'stra\u00DFe');
    expect(nameSkeleton('\u00E6\u0111\u00F8'), '\u00E6\u0111\u00F8');
    expect(nameSkeleton('\u0234'), '\u0234');
    final bill = _bill([
      ('xx-aaaaaaaa', '\u00C1na'),
      ('yy-bbbbbbbb', 'A\u0301na'),
    ]);
    expect(bill.sharedNames, hasLength(2));
  });
}
