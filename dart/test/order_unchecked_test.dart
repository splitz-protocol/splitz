// §10.2 orders entries a caller has not checked, as every reader orders them:
// a member that is missing or not a string sorts as empty.
import 'package:splitz_core/splitz_core.dart';
import 'package:test/test.dart';

void main() {
  test('an entry with no string `at`, `author` or `id` still orders', () {
    final ordered = orderEntries([
      {'id': 'b', 'at': '2026-10-28T19:00:00.000Z', 'author': 'ana'},
      {'id': 'a'},
      {'at': 5},
    ]);
    expect([for (final e in ordered) e['id']], [null, 'a', 'b']);
  });

  test('and deltaFor answers for it rather than throwing', () {
    expect(
        deltaFor([
          {'id': 'x'}
        ], {}),
        isA<DeltaSquare>());
    expect(
        deltaFor([
          {'at': 5}
        ], {}),
        isA<DeltaSquare>());
    expect(
        deltaFor([
          {'id': 'x'}
        ], {
          'x'
        }),
        isA<NothingMissing>());
  });
}
