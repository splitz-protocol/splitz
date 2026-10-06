/// §14.11: putting somebody on a bill before they join.
library;

import 'package:splitz_core/host.dart' as entries;
import 'package:splitz_core/splitz_core.dart' as protocol;
import 'package:splitz_host/splitz_host.dart';
import 'package:test/test.dart';

import 'support/fake_wallet.dart';

void main() {
  final ana = FakeHost(me: 'ana', payToAddress: 'u1ana');
  final ben = FakeHost(me: 'ben', payToAddress: 'u1ben');
  final log = [
    entries.createBill(
      host: ana,
      name: 'Trip',
      currency: 'USD',
      creatorKey: fakeKey('ana'),
    ),
    entries.joinBill(host: ana, name: 'Ana', payTo: 'u1ana'),
    entries.joinBill(host: ben, name: 'Ben', payTo: 'u1ben'),
  ];
  entries.FoldedBill fold(List<Map<String, dynamic>> l) =>
      entries.BillLog(ana, entries: l, billId: billIdOf(l)).fold();

  Matcher refusedWith(String code) =>
      throwsA(isA<protocol.SplitError>().having((e) => e.code, 'code', code));

  test('a new name is written as them, unsigned, and joins the bill', () {
    final entry = addPersonEntry(
      host: ana,
      folded: fold(log),
      id: 'jo',
      name: 'Jo',
    );
    expect(entry['author'], 'jo');
    expect(entry.containsKey('sig'), isFalse);
    final after = fold([...log, entry]);
    expect(after.bill.participant('jo')?.name, 'Jo');
    expect(after.setAside, isEmpty);
  });

  test('an id already on the bill is refused, not renamed', () {
    expect(
      () => addPersonEntry(host: ana, folded: fold(log), id: 'ben', name: 'B'),
      refusedWith(protocol.SplitCode.duplicateParticipant),
    );
  });

  test("this device's own id is refused", () {
    final alone = log.take(1).toList();
    expect(
      () =>
          addPersonEntry(host: ana, folded: fold(alone), id: 'ana', name: 'A'),
      refusedWith(protocol.SplitCode.duplicateParticipant),
    );
  });

  test('an empty id is refused', () {
    expect(
      () =>
          addPersonEntry(host: ana, folded: fold(log), id: '', name: 'Nobody'),
      refusedWith(protocol.SplitCode.billMissingEntryPayload),
    );
  });

  test('a second add under the id of somebody added by hand is refused', () {
    final jo = addPersonEntry(
      host: ana,
      folded: fold(log),
      id: 'jo',
      name: 'Jo',
    );
    final withJo = [...log, jo];
    expect(
      () => addPersonEntry(
        host: ana,
        folded: fold(withJo),
        id: 'jo',
        name: 'Joanna',
      ),
      refusedWith(protocol.SplitCode.duplicateParticipant),
    );
    expect(fold(withJo).bill.participant('jo')?.name, 'Jo');
  });

  test('two different people added by hand both join', () {
    final jo = addPersonEntry(
      host: ana,
      folded: fold(log),
      id: 'jo',
      name: 'Jo',
    );
    final withJo = [...log, jo];
    final sam = addPersonEntry(
      host: ana,
      folded: fold(withJo),
      id: 'sam',
      name: 'Jo',
    );
    final after = fold([...withJo, sam]);
    expect(after.setAside, isEmpty);
    expect(after.bill.participant('jo')?.name, 'Jo');
    expect(after.bill.participant('sam')?.name, 'Jo');
  });

  test('an id differing only in case is somebody else', () {
    final entry = addPersonEntry(
      host: ana,
      folded: fold(log),
      id: 'Ben',
      name: 'Ben',
    );
    final after = fold([...log, entry]);
    expect(after.bill.participant('ben')?.payTo, 'u1ben');
    expect(after.bill.participant('Ben'), isNotNull);
  });
}
