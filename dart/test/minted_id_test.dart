/// §10.3 step 5: every expense and payment id is minted by its entry's author.
///
/// An id is minted by an author when it is that author's participant id, `:`,
/// then anything, and an author whose id holds `:` mints nothing. An entry, or
/// an amendment, whose id its author did not mint is set aside with
/// `id_not_minted`.
import 'package:splitz_core/splitz_core.dart';
import 'package:test/test.dart';

String _at(int minute) =>
    '2026-10-28T19:${minute.toString().padLeft(2, '0')}:00.000Z';

Map<String, dynamic> _sealed(Map<String, dynamic> entry) =>
    {...entry, 'id': deriveEntryId(entry)};

final Map<String, dynamic> _create = () {
  final e = <String, dynamic>{
    'v': 1,
    'author': 'ana',
    'kind': 'createBill',
    'at': _at(0),
    'name': 'Dinner',
    'currency': 'EUR',
    'splitMode': 'equal',
    'creatorKey': 'A' * 43,
    'nonce': 'A' * 22,
  };
  return {...e, 'id': deriveBillId(e)};
}();

Map<String, dynamic> _join(String who, int minute) => _sealed({
      'v': 1,
      'author': who,
      'kind': 'joinBill',
      'at': _at(minute),
      'participant': {'id': who, 'name': who},
    });

Map<String, dynamic> _expense(String author, String id, int minute) => _sealed({
      'v': 1,
      'author': author,
      'kind': 'addExpense',
      'at': _at(minute),
      'expense': {
        'id': id,
        'paidBy': author,
        'amount': 9000,
        'at': _at(minute),
        'split': {
          'type': 'equal',
          'among': ['ana', 'ben'],
        },
      },
    });

Map<String, dynamic> _payment(
        String author, String from, String id, int amount, int minute) =>
    _sealed({
      'v': 1,
      'author': author,
      'kind': 'recordPayment',
      'at': _at(minute),
      'payment': {
        'id': id,
        'from': from,
        'to': 'ana',
        'amount': amount,
        'method': 'cash',
        'at': _at(minute),
      },
    });

Map<String, dynamic> _amendment(Map<String, dynamic> target, int amount) =>
    _sealed({
      'v': 1,
      'author': target['author'],
      'kind': 'amendEntry',
      'at': _at(30),
      'targetId': target['id'],
      'expense': {
        ...target['expense'] as Map<String, dynamic>,
        'amount': amount,
      },
    });

List<Map<String, dynamic>> _bill([List<String> others = const []]) => [
      _create,
      _join('ana', 1),
      _join('ben', 2),
      for (final (i, who) in others.indexed) _join(who, 3 + i),
    ];

FoldResult _fold(List<Map<String, dynamic>> entries) =>
    foldLog(entries, billId: _create['id'] as String);

List<String> _ids(FoldResult r, String member) => [
      for (final e in r.bill[member] as List) (e as Map)['id'] as String,
    ];

List<String> _codes(FoldResult r) => [for (final s in r.setAside) s.code];

void main() {
  test('an expense id its author did not mint is set aside', () {
    final r = _fold([..._bill(), _expense('ana', 'hotel', 10)]);
    expect(_ids(r, 'expenses'), isEmpty);
    expect(_codes(r), [SplitCode.idNotMinted]);
  });

  test('a payment id its author did not mint is set aside', () {
    final r = _fold([..._bill(), _payment('ben', 'ben', 't1', 4500, 10)]);
    expect(_ids(r, 'payments'), isEmpty);
    expect(_codes(r), [SplitCode.idNotMinted]);
  });

  test("a backdated copy of somebody else's id is set aside; theirs stands",
      () {
    final r = _fold([
      ..._bill(),
      _expense('ana', 'ana:hotel', 20),
      _expense('ben', 'ana:hotel', 8),
      _payment('ben', 'ben', 'ben:t1:ana', 4500, 20),
      _payment('ana', 'ben', 'ben:t1:ana', 1, 8),
    ]);
    expect(_ids(r, 'expenses'), ['ana:hotel']);
    expect(_ids(r, 'payments'), ['ben:t1:ana']);
    expect(((r.bill['payments'] as List).single as Map)['amount'], 4500);
    expect(_codes(r), [SplitCode.idNotMinted, SplitCode.idNotMinted]);
  });

  test("the author's own id is not minted; nothing after the colon is", () {
    final r = _fold(
        [..._bill(), _expense('ana', 'ana', 10), _expense('ana', 'ana:', 11)]);
    expect(_ids(r, 'expenses'), ['ana:']);
    expect(_codes(r), [SplitCode.idNotMinted]);
  });

  test('an id that only begins with the author id is not minted', () {
    final r = _fold([..._bill(), _expense('ana', 'anabel:x', 10)]);
    expect(_ids(r, 'expenses'), isEmpty);
    expect(_codes(r), [SplitCode.idNotMinted]);
  });

  test('an id holding a colon cannot join, so records nothing', () {
    final r = _fold([
      ..._bill(['ben:t1']),
      _payment('ben:t1', 'ben:t1', 'ben:t1:own', 1, 10),
    ]);
    expect(_ids(r, 'payments'), isEmpty);
    expect(_codes(r), contains(SplitCode.billBadParticipantId));
  });

  test('an amendment of an unminted expense is set aside with it', () {
    final target = _expense('ana', 'hotel', 10);
    final amendment = _amendment(target, 1);
    final r = _fold([..._bill(), target, amendment]);
    expect(_ids(r, 'expenses'), isEmpty);
    expect(
        r.setAside.map((s) => (s.id, s.code)),
        unorderedEquals([
          (amendment['id'], SplitCode.idNotMinted),
          (target['id'], SplitCode.idNotMinted),
        ]));
  });

  test('minted ids apply, and an amendment of one applies', () {
    final target = _expense('ana', 'ana:hotel', 10);
    final r = _fold([
      ..._bill(),
      target,
      _amendment(target, 1),
      _payment('ben', 'ben', 'ben:t1:ana', 4500, 12),
    ]);
    expect(_ids(r, 'expenses'), ['ana:hotel']);
    expect(((r.bill['expenses'] as List).single as Map)['amount'], 1);
    expect(_ids(r, 'payments'), ['ben:t1:ana']);
    expect(r.setAside, isEmpty);
    expect(ownsId('ana', 'ana:hotel'), isTrue);
    expect(ownsId('ana', 'ana'), isFalse);
    expect(ownsId('ben:t1', 'ben:t1:ana'), isFalse);
  });
}
