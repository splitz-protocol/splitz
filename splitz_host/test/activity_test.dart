/// The log read as a history.
///
/// A folded bill says what is true now. These assert the three things only the
/// log shows: an entry somebody withdrew, an entry the fold refused, and a
/// payment claimed but not confirmed.
library;

import 'package:splitz_core/splitz_core.dart' show deriveEntryId;
import 'package:splitz_core/host.dart' as entries;
import 'package:splitz_host/splitz_host.dart';
import 'package:test/test.dart';

import 'support/fake_wallet.dart';

/// Folds [log] and reads it as a history.
List<BillEvent> historyOf(entries.BillLog log) {
  final folded = log.fold();
  return activityOf(
    log.entries,
    folded.bill,
    setAside: folded.setAside,
    withdrawn: folded.withdrawn,
  );
}

void main() {
  group('an entry the fold set aside for a member of the wrong type', () {
    // §10.1 checks the ids at ingress and leaves the rest to the fold, so an
    // entry naming a string amount reaches the store. The fold sets it aside;
    // the history must read it as well, or one peer's entry takes down every
    // bill a wallet lists.
    for (final (member, value) in <(String, Object)>[
      ('amount', '9000'),
      ('description', 7),
    ]) {
      test('expense.$member ${value.runtimeType}', () {
        final ana = FakeHost(me: 'ana', payToAddress: 'u1ana');
        final create = entries.createBill(
          host: ana,
          name: 'Dinner',
          currency: 'USD',
          creatorKey: fakeKey('ana'),
        );
        ana.tick();
        final join = entries.joinBill(host: ana, name: 'Ana', payTo: 'u1ana');
        ana.tick();
        final expense = entries.addExpense(
          host: ana,
          expenseId: 'x1',
          paidBy: 'ana',
          amount: 9000,
          split: const {
            'type': 'equal',
            'among': ['ana'],
          },
        );
        // Typed as a decoded entry is, so the reader meets the member.
        final bad = <String, dynamic>{
          ...expense,
          'expense': <String, dynamic>{
            ...expense['expense'] as Map<String, dynamic>,
            member: value,
          },
        };
        bad['id'] = deriveEntryId(bad);
        final log = entries.BillLog(ana, entries: [create, join]);
        log.add([bad]);
        expect(log.fold().setAside.map((s) => s.id), contains(bad['id']));
        expect(() => historyOf(log), returnsNormally);
        expect(historyOf(log), hasLength(3));
      });
    }
  });

  test('two copies of one entry are one line', () {
    final ana = FakeHost(me: 'ana', payToAddress: 'u1ana');
    final create = entries.createBill(
      host: ana,
      name: 'Dinner',
      currency: 'USD',
      creatorKey: fakeKey('ana'),
    );
    ana.tick();
    final join = entries.joinBill(host: ana, name: 'Ana', payTo: 'u1ana');
    ana.tick();
    final expense = entries.addExpense(
      host: ana,
      expenseId: 'x1',
      paidBy: 'ana',
      amount: 9000,
      split: const {
        'type': 'equal',
        'among': ['ana'],
      },
    );
    // §10.2 keeps both signed copies of one id; §9.5 makes them agree.
    final log = entries.BillLog(
      ana,
      entries: [
        create,
        {...join, 'sig': 'AAAA'},
        {...join, 'sig': 'BBBB'},
        {...expense, 'sig': 'AAAA'},
        {...expense, 'sig': 'BBBB'},
      ],
    );
    final history = historyOf(log);
    expect(history.map((e) => e.kind), [
      BillEventKind.expenseAdded,
      BillEventKind.joined,
      BillEventKind.opened,
    ]);
  });

  group('what a history shows', () {
    test('every entry becomes a line, newest first', () {
      final ana = FakeHost(me: 'ana', payToAddress: 'u1ana');
      final create = entries.createBill(
        host: ana,
        name: 'Dinner',
        currency: 'USD',
        creatorKey: fakeKey('ana'),
      );
      // §10.2 orders by instant and breaks a tie by entry id, so two entries
      // written at one instant are ordered by a digest rather than by which
      // was written first. The clock moves so the history has a clock order
      // to show.
      ana.tick();
      final log = entries.BillLog(
        ana,
        entries: [
          create,
          entries.joinBill(host: ana, name: 'Ana', payTo: 'u1ana'),
        ],
      );
      ana.tick();
      log.add([
        entries.addExpense(
          host: ana,
          expenseId: 'x1',
          paidBy: 'ana',
          amount: 9000,
          split: const {
            'type': 'equal',
            'among': ['ana'],
          },
          description: 'dinner',
        ),
      ]);

      final history = historyOf(log);
      expect(history.first.kind, BillEventKind.expenseAdded);
      expect(history.first.amountMinorUnits, 9000);
      expect(history.first.description, 'dinner');
      expect(history.last.kind, BillEventKind.opened);
      expect(history.every((e) => e.applied), isTrue);
    });

    test('a second join carrying an address is an address change', () {
      // §13: a payer MUST be shown this before settling to it. A second
      // "joined" line would bury the one amendment that moves money.
      final ana = FakeHost(me: 'ana', payToAddress: 'u1ana');
      final log = entries.BillLog(
        ana,
        entries: [
          entries.createBill(
            host: ana,
            name: 'D',
            currency: 'USD',
            creatorKey: fakeKey('ana'),
          ),
          entries.joinBill(host: ana, name: 'Ana', payTo: 'u1ana'),
        ],
      );
      ana.tick();
      log.add([entries.joinBill(host: ana, name: 'Ana', payTo: 'u1elsewhere')]);

      final history = historyOf(log);
      expect(history.first.kind, BillEventKind.addressChanged);
      expect(history.where((e) => e.kind == BillEventKind.joined).length, 1);
    });

    test('a join with no address is not an address change', () {
      final ana = FakeHost(me: 'ana');
      final log = entries.BillLog(
        ana,
        entries: [
          entries.createBill(
            host: ana,
            name: 'D',
            currency: 'USD',
            creatorKey: fakeKey('ana'),
          ),
          entries.joinBill(host: ana, name: 'Ana'),
        ],
      );
      ana.tick();
      log.add([entries.joinBill(host: ana, name: 'Ana Again')]);

      expect(
        historyOf(log).where((e) => e.kind == BillEventKind.addressChanged),
        isEmpty,
      );
    });

    test('a priced bill says what it was priced at', () {
      final ana = FakeHost(me: 'ana');
      final log = entries.BillLog(
        ana,
        entries: [
          entries.createBill(
            host: ana,
            name: 'D',
            currency: 'USD',
            creatorKey: fakeKey('ana'),
          ),
          entries.setRate(
            host: ana,
            currency: 'USD',
            minorUnitsPerZec: 51234,
            source: 'a named feed',
          ),
        ],
      );
      final priced = historyOf(
        log,
      ).firstWhere((e) => e.kind == BillEventKind.priced);
      expect(priced.amountMinorUnits, 51234);
      expect(priced.description, 'a named feed');
    });
  });

  group('a payment is a claim until it is confirmed', () {
    ({entries.BillLog log, FakeHost ana}) paid({required String method}) {
      final ana = FakeHost(me: 'ana', payToAddress: 'u1ana');
      final log = entries.BillLog(
        ana,
        entries: [
          entries.createBill(
            host: ana,
            name: 'D',
            currency: 'USD',
            creatorKey: fakeKey('ana'),
          ),
          entries.joinBill(host: ana, name: 'Ana', payTo: 'u1ana'),
          entries.joinBill(
            host: FakeHost(me: 'ben'),
            name: 'Ben',
            payTo: 'u1ben',
          ),
          entries.addExpense(
            host: FakeHost(me: 'ben'),
            expenseId: 'x1',
            paidBy: 'ben',
            amount: 2000,
            split: const {
              'type': 'equal',
              'among': ['ana', 'ben'],
            },
          ),
        ],
      );
      ana.tick();
      log.add([
        entries.recordPayment(
          host: ana,
          paymentId: method == 'swap' ? 'near-intent-7f3a' : 'p1',
          to: 'ben',
          amount: 1000,
          method: method,
          reference: method == 'swap' ? 'near-intent-7f3a' : null,
        ),
      ]);
      return (log: log, ana: ana);
    }

    test('an unconfirmed payment says so', () {
      final b = paid(method: 'cash');
      final event = historyOf(
        b.log,
      ).firstWhere((e) => e.kind == BillEventKind.paymentRecorded);
      expect(event.method, 'cash');
      expect(event.amountMinorUnits, 1000);
      expect(event.subject, 'ben');
      // Presenting this as settled tells a payer a debt is discharged that
      // the payee has never agreed was paid.
      expect(event.confirmed, isFalse);
    });

    test("the payee's confirmation flips it, and is its own line", () {
      final b = paid(method: 'cash');
      // Ben's own clock, moved past the payment: the confirmation is a later
      // entry and §10.2 orders the log by what each author wrote, not by when
      // this device merged it.
      final ben = FakeHost(me: 'ben')..tick(const Duration(minutes: 2));
      b.log.add([
        entries.confirmPayment(
          host: ben,
          paymentId: 'ana:p1',
          method: 'recipientConfirmed',
          record: b.log.fold().paymentDigests['ana:p1']!,
        ),
      ]);

      final history = historyOf(b.log);
      expect(
        history
            .firstWhere((e) => e.kind == BillEventKind.paymentRecorded)
            .confirmed,
        isTrue,
      );
      expect(history.first.kind, BillEventKind.paymentConfirmed);
      expect(history.first.method, 'recipientConfirmed');
    });

    test('a swap carries its reference, which is not a txid', () {
      final b = paid(method: 'swap');
      final event = historyOf(
        b.log,
      ).firstWhere((e) => e.kind == BillEventKind.paymentRecorded);
      expect(event.method, 'swap');
      expect(event.reference, 'near-intent-7f3a');
    });

    test('only the payee is offered the confirmation', () {
      // A payer who could confirm their own payment would settle a debt by
      // asserting twice that they paid it.
      final b = paid(method: 'cash');
      final bill = b.log.fold().bill;
      expect(awaitingConfirmationBy(bill, 'ben').map((p) => p.id), ['ana:p1']);
      expect(awaitingConfirmationBy(bill, 'ana'), isEmpty);
    });

    test('a confirmed payment leaves the waiting list', () {
      final b = paid(method: 'cash');
      final ben = FakeHost(me: 'ben')..tick(const Duration(minutes: 2));
      b.log.add([
        entries.confirmPayment(
          host: ben,
          paymentId: 'ana:p1',
          method: 'recipientConfirmed',
          record: b.log.fold().paymentDigests['ana:p1']!,
        ),
      ]);
      expect(awaitingConfirmationBy(b.log.fold().bill, 'ben'), isEmpty);
    });
  });

  group('what a correction or a withdrawal acts on', () {
    test('each names its target entry', () {
      final ana = FakeHost(me: 'ana', payToAddress: 'u1ana');
      final log = entries.BillLog(
        ana,
        entries: [
          entries.createBill(
            host: ana,
            name: 'D',
            currency: 'USD',
            creatorKey: fakeKey('ana'),
          ),
          entries.joinBill(host: ana, name: 'Ana', payTo: 'u1ana'),
        ],
      );
      ana.tick();
      final taxi = entries.addExpense(
        host: ana,
        expenseId: 'x1',
        paidBy: 'ana',
        amount: 3000,
        split: const {
          'type': 'equal',
          'among': ['ana'],
        },
        description: 'Taxi',
      );
      log.add([taxi]);
      ana.tick();
      final amend = entries.amendEntry(
        host: ana,
        targetId: taxi['id'] as String,
        member: 'expense',
        payload: {...taxi['expense'] as Map<String, dynamic>, 'amount': 2500},
      );
      log.add([amend]);
      ana.tick();
      log.add([entries.voidEntry(host: ana, targetId: taxi['id'] as String)]);

      final history = historyOf(log);
      final withdrawal = history.firstWhere(
        (e) => e.kind == BillEventKind.entryWithdrawn,
      );
      expect(withdrawal.subject, taxi['id']);
      final correction = history.firstWhere(
        (e) => e.kind == BillEventKind.expenseAmended,
      );
      expect(correction.subject, taxi['id']);
      // The line a withdrawal names is still read, and marked withdrawn.
      final added = history.firstWhere((e) => e.entryId == taxi['id']);
      expect((added.description, added.withdrawn), ('Taxi', true));
    });
  });

  group('what the fold would not apply', () {
    test('a refused entry stays in the history, with its code', () {
      // An entry that vanished silently is indistinguishable from one that
      // was never sent.
      final ana = FakeHost(me: 'ana', payToAddress: 'u1ana');
      final log = entries.BillLog(
        ana,
        entries: [
          entries.createBill(
            host: ana,
            name: 'D',
            currency: 'USD',
            creatorKey: fakeKey('ana'),
          ),
          entries.joinBill(host: ana, name: 'Ana', payTo: 'u1ana'),
        ],
      );
      ana.tick();
      log.add([
        entries.recordPayment(
          host: ana,
          paymentId: 'ana:p1',
          to: 'ana',
          amount: 100,
          method: 'cash',
        ),
      ]);

      final refused = historyOf(
        log,
      ).firstWhere((e) => e.kind == BillEventKind.paymentRecorded);
      expect(refused.refusedCode, 'self_payment');
      expect(refused.applied, isFalse);
    });
  });
}
