/// Correcting and withdrawing an entry (§10.4, §10.8).
///
/// An amendment replaces its target wholesale. That is the whole hazard: a
/// payload built from the part being changed silently deletes everything it
/// leaves out, and the fold has no way to tell that from an intentional
/// removal.
library;

import 'package:test/test.dart';
import 'package:splitz_core/splitz_core.dart' as splitz;
import 'package:splitz_core/host.dart';

import 'support/fake_host.dart';

({BillLog log, FakeHost ana, Map<String, dynamic> expense}) billWithExpense() {
  final ana = FakeHost(me: 'ana', payToAddress: 'u1ana');
  final expense = addExpense(
    host: ana,
    expenseId: 'x1',
    paidBy: 'ana',
    amount: 9000,
    split: const {
      'type': 'equal',
      'among': ['ana', 'ben'],
    },
    description: 'dinner',
  );
  final log = BillLog(ana, entries: [
    createBill(
        host: ana, name: 'D', currency: 'USD', creatorKey: fakeKey('ana')),
    joinBill(host: ana, name: 'Ana', payTo: 'u1ana'),
    joinBill(host: FakeHost(me: 'ben'), name: 'Ben', payTo: 'u1ben'),
    expense,
  ]);
  return (log: log, ana: ana, expense: expense);
}

void main() {
  group('correcting an expense', () {
    test('an amendment replaces the entry it names', () {
      final b = billWithExpense();
      b.ana.tick();
      b.log.add([
        amendEntry(
          host: b.ana,
          targetId: b.expense['id'] as String,
          member: splitz.payloadForKind['addExpense']!,
          payload: <String, dynamic>{
            ...b.expense['expense'] as Map<String, dynamic>,
            'amount': 6000,
          },
        )
      ]);

      final folded = b.log.fold();
      expect(folded.setAside, isEmpty);
      // One expense, not two: an amendment corrects rather than appends.
      expect(folded.bill.expenses.length, 1);
      expect(folded.bill.expenses.single.amount, 6000);
      expect(folded.bill.expenses.single.description, 'dinner');
    });

    test('a payload that leaves a field out deletes it', () {
      // The reason the doc comment says to build from the current entry: an
      // amendment is a replacement, and the fold cannot tell an omission from
      // an intentional removal.
      final b = billWithExpense();
      b.ana.tick();
      b.log.add([
        amendEntry(
          host: b.ana,
          targetId: b.expense['id'] as String,
          member: 'expense',
          payload: <String, dynamic>{
            'id': 'ana:x1',
            'paidBy': 'ana',
            'amount': 6000,
            'currency': 'USD',
            'at': '2026-10-28T19:30:00.000Z',
            'split': const {
              'type': 'equal',
              'among': ['ana', 'ben'],
            },
          },
        )
      ]);

      final folded = b.log.fold();
      expect(folded.setAside, isEmpty);
      expect(folded.bill.expenses.single.description, isEmpty,
          reason: 'the description was not carried over and is gone');
    });

    test('the mapping comes from the protocol, not from a guess', () {
      // A member name wrong by one letter is `amend_kind_mismatch`, so the
      // caller reads the fold's own mapping.
      expect(splitz.payloadForKind['addExpense'], 'expense');
      expect(splitz.payloadForKind['joinBill'], 'participant');
      expect(splitz.payloadForKind['recordPayment'], 'payment');
    });
  });

  group('what an amendment refuses', () {
    test('somebody amending an entry they did not write', () {
      final b = billWithExpense();
      final ben = FakeHost(me: 'ben')..tick();
      b.log.add([
        amendEntry(
          host: ben,
          targetId: b.expense['id'] as String,
          member: 'expense',
          payload: <String, dynamic>{
            ...b.expense['expense'] as Map<String, dynamic>,
            'amount': 1,
          },
        )
      ]);

      final folded = b.log.fold();
      expect(folded.setAside.map((s) => s.code),
          contains(splitz.SplitCode.unauthorizedEntry));
      // And the expense is untouched: Ben cannot rewrite Ana's figure.
      expect(folded.bill.expenses.single.amount, 9000);
    });

    test('a payload of the wrong kind', () {
      final b = billWithExpense();
      b.ana.tick();
      b.log.add([
        amendEntry(
          host: b.ana,
          targetId: b.expense['id'] as String,
          // A participant where an expense belongs. Refused rather than
          // applied, because an amendment replaces wholesale and this one
          // would delete the expense.
          member: 'participant',
          payload: <String, dynamic>{'id': 'ana', 'name': 'Ana'},
        )
      ]);

      final folded = b.log.fold();
      expect(folded.setAside.map((s) => s.code),
          contains(splitz.SplitCode.amendKindMismatch));
      expect(folded.bill.expenses.single.amount, 9000);
    });

    test('an amendment naming an entry the log does not hold', () {
      final b = billWithExpense();
      b.ana.tick();
      b.log.add([
        amendEntry(
          host: b.ana,
          targetId: 'an-entry-nobody-has',
          member: 'expense',
          payload: <String, dynamic>{
            ...b.expense['expense'] as Map<String, dynamic>,
          },
        )
      ]);
      expect(b.log.fold().setAside.map((s) => s.code),
          contains(splitz.SplitCode.unknownEntry));
    });
  });

  group('withdrawing an expense', () {
    test('a void takes it off the bill and leaves it in the log', () {
      final b = billWithExpense();
      b.ana.tick();
      b.log.add([voidEntry(host: b.ana, targetId: b.expense['id'] as String)]);

      final folded = b.log.fold();
      expect(folded.bill.expenses, isEmpty);
      // Still in the log: removing it would leave a reader unable to see it
      // was ever written.
      expect(folded.withdrawn, contains(b.expense['id']));
      expect(b.log.entries.map((e) => e['id']), contains(b.expense['id']));
    });

    test('somebody withdrawing an expense they did not write', () {
      final b = billWithExpense();
      final ben = FakeHost(me: 'ben')..tick();
      b.log.add([voidEntry(host: ben, targetId: b.expense['id'] as String)]);

      final folded = b.log.fold();
      // §10.8: an expense is its author's. Ben is on the bill and still
      // cannot take Ana's expense off it.
      expect(folded.bill.expenses.single.amount, 9000);
      expect(folded.setAside.map((s) => s.code),
          contains(splitz.SplitCode.unauthorizedEntry));
    });
  });
}
