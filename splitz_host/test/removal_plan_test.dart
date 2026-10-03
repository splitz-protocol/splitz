/// What taking somebody off a bill needs first (§10.8).
///
/// Mirrored case for case by `rust/splitz-host/tests/removal_plan.rs`.
library;

import 'package:splitz_core/host.dart' as entries;
import 'package:splitz_core/splitz_core.dart' as protocol;
import 'package:splitz_host/splitz_host.dart';
import 'package:test/test.dart';

import 'support/fake_wallet.dart';

/// One bill's writers on one clock, and the log they write.
class _Bill {
  _Bill() {
    write('ana', (h) {
      return entries.createBill(
        host: h,
        name: 'Trip',
        currency: 'USD',
        creatorKey: fakeKey('ana'),
      );
    });
    join('ana');
  }

  final Map<String, FakeHost> _hosts = {};
  final Map<String, int> _clock = {};
  final List<Map<String, dynamic>> log = [];
  final Map<String, String> joins = {};

  FakeHost host(String who) =>
      _hosts.putIfAbsent(who, () => FakeHost(me: who, payToAddress: 'u1$who'));

  /// [who] writes one entry, a minute after the last.
  Map<String, dynamic> write(
    String who,
    Map<String, dynamic> Function(FakeHost) entry,
  ) {
    // Every entry a minute after the last, whoever writes it, so §10.2's
    // order is the order written.
    final h = host(who);
    h.tick(Duration(minutes: log.length - (_clock[who] ?? 0)));
    _clock[who] = log.length;
    final e = entry(h);
    log.add(e);
    return e;
  }

  void join(String who) => joins[who] =
      write(
            who,
            (h) => entries.joinBill(host: h, name: who, payTo: 'u1$who'),
          )['id']
          as String;

  Map<String, dynamic> expense(
    String who,
    String expenseId,
    String paidBy,
    int amount,
    Map<String, dynamic> split, {
    String? description,
  }) => write(
    who,
    (h) => entries.addExpense(
      host: h,
      expenseId: expenseId,
      paidBy: paidBy,
      amount: amount,
      split: split,
      description: description,
    ),
  );

  Map<String, dynamic> amend(
    String who,
    Map<String, dynamic> target,
    Map<String, dynamic> split,
  ) => write(
    who,
    (h) => entries.amendEntry(
      host: h,
      targetId: target['id'] as String,
      member: 'expense',
      payload: {...(target['expense'] as Map<String, dynamic>), 'split': split},
    ),
  );

  Map<String, dynamic> withdraw(String who, String targetId) =>
      write(who, (h) => entries.voidEntry(host: h, targetId: targetId));

  entries.BillLog get _held =>
      entries.BillLog(host('ana'), entries: log, billId: billIdOf(log));

  entries.FoldedBill fold() => _held.fold();

  /// [id]'s removal as [me] plans it, over the log in §10.2's order.
  RemovalPlan plan(String id, {String me = 'ana'}) => planRemoval(
    folded: fold(),
    creatorId: 'ana',
    log: _held.entries,
    id: id,
    me: me,
  );

  /// Writes [plan] as [me] would: each expense again under its new split,
  /// and the one it replaces withdrawn.
  void restate(RemovalPlan plan, {String me = 'ana'}) {
    var n = 0;
    for (final edit in plan.edits) {
      write(
        me,
        (h) => entries.addExpense(
          host: h,
          expenseId: 'restated-${n++}',
          paidBy: edit.seen.paidBy,
          amount: edit.seen.amount,
          split: edit.split,
          description: edit.seen.description.isEmpty
              ? null
              : edit.seen.description,
        ),
      );
      withdraw(me, edit.entryId);
    }
  }
}

Map<String, dynamic> _equal(List<String> among) => {
  'type': 'equal',
  'among': among,
};

/// A taxi of 30.00 Ana paid, split by Ana, Ben and Cai.
(_Bill, Map<String, dynamic>) _taxi() {
  final b = _Bill()
    ..join('ben')
    ..join('cai');
  final taxi = b.expense(
    'ana',
    'taxi',
    'ana',
    3000,
    _equal(['ana', 'ben', 'cai']),
    description: 'Taxi',
  );
  return (b, taxi);
}

/// A boat of 40.00 Cai wrote and paid, shared by Ana, Ben and Cai.
(_Bill, Map<String, dynamic>) _boat() {
  final b = _Bill()
    ..join('ben')
    ..join('cai');
  final boat = b.expense(
    'cai',
    'boat',
    'cai',
    4000,
    _equal(['ana', 'ben', 'cai']),
    description: 'Boat',
  );
  return (b, boat);
}

void main() {
  group('a split without somebody', () {
    test('equal and shares drop them; the rest share it', () {
      expect(splitWithout(_equal(['ana', 'ben', 'cai']), 'ben'), {
        'type': 'equal',
        'among': ['ana', 'cai'],
      });
      expect(
        splitWithout({
          'type': 'shares',
          'shareCounts': {'ana': 1, 'ben': 2},
        }, 'ben'),
        {
          'type': 'shares',
          'shareCounts': {'ana': 1},
        },
      );
    });

    test('nobody left, or a figure that must still add up, is by hand', () {
      expect(splitWithout(_equal(['ben']), 'ben'), isNull);
      expect(
        splitWithout({
          'type': 'exact',
          'amounts': {'ana': 500, 'ben': 500},
        }, 'ben'),
        isNull,
      );
      expect(
        splitWithout({
          'type': 'percentage',
          'basisPoints': {'ana': 5000, 'ben': 5000},
        }, 'ben'),
        isNull,
      );
    });

    test('itemized drops them per item; an item only they had is by hand', () {
      Map<String, dynamic> itemized(List<String> sharedBy) => {
        'type': 'itemized',
        'extraMinorUnits': 0,
        'items': [
          {'description': 'pizza', 'minorUnits': 1000, 'sharedBy': sharedBy},
        ],
      };
      expect(
        (splitWithout(itemized(['ana', 'ben']), 'ben')!['items'] as List)
            .single['sharedBy'],
        ['ana'],
      );
      expect(splitWithout(itemized(['ben']), 'ben'), isNull);
    });

    test('zero shares left is by hand, some left is not', () {
      expect(
        splitWithout({
          'type': 'shares',
          'shareCounts': {'ana': 0, 'ben': 2, 'cai': 0},
        }, 'ben'),
        isNull,
      );
      expect(
        splitWithout({
          'type': 'shares',
          'shareCounts': {'ana': 0, 'ben': 2, 'cai': 1},
        }, 'ben'),
        {
          'type': 'shares',
          'shareCounts': {'ana': 0, 'cai': 1},
        },
      );
    });
  });

  group('the plan', () {
    test('off an expense this device wrote, then off the bill', () {
      final (b, taxi) = _taxi();
      final plan = b.plan('ben');
      expect(plan.blockers, isEmpty);
      expect(plan.edits.single.entryId, taxi['id']);
      expect(plan.edits.single.split, _equal(['ana', 'cai']));

      b
        ..restate(plan)
        ..withdraw('ana', b.joins['ben']!);
      final folded = b.fold();
      expect(folded.setAside, isEmpty);
      expect(folded.bill.participant('ben'), isNull);
      final now = folded.bill.expenses.single;
      expect((now.paidBy, now.amount, now.description), ('ana', 3000, 'Taxi'));
      expect(now.split['among'], ['ana', 'cai']);
      expect(b.plan('ben').namesThem, isFalse);
    });

    test('what they paid for is said, not done', () {
      final b = _Bill()..join('ben');
      final hotel = b.expense(
        'ben',
        'hotel',
        'ben',
        2000,
        _equal(['ana', 'ben']),
        description: 'Hotel',
      );
      final plan = b.plan('ben');
      expect(plan.edits, isEmpty);
      final blocker = plan.blockers.single;
      expect(blocker.block, RemovalBlock.paidFor);
      expect(blocker.entryId, hotel['id']);
      expect(blocker.description, 'Hotel');
    });

    test('the creator or the author restates; anybody else is told whose', () {
      final (b, boat) = _boat();
      b.join('dee');
      expect(b.plan('ben', me: 'cai').edits, isNotEmpty);
      final other = b.plan('ben', me: 'dee');
      expect(other.edits, isEmpty);
      final blocker = other.blockers.single;
      expect(blocker.block, RemovalBlock.addedByAnother);
      expect((blocker.description, blocker.author), ('Boat', 'cai'));

      final plan = b.plan('ben');
      expect(plan.blockers, isEmpty);
      expect(plan.edits.single.entryId, boat['id']);
      expect(plan.edits.single.author, 'cai');
      b
        ..restate(plan)
        ..withdraw('ana', b.joins['ben']!);
      final folded = b.fold();
      expect(folded.setAside, isEmpty);
      expect(folded.bill.participant('ben'), isNull);
      final now = folded.bill.expenses.single;
      expect((now.paidBy, now.amount, now.description), ('cai', 4000, 'Boat'));
      expect(now.split['among'], ['ana', 'cai']);
    });

    test('an amendment adding them names them too', () {
      final (b, boat) = _boat();
      b
        ..join('dee')
        ..amend('cai', boat, _equal(['ana', 'ben', 'cai', 'dee']));
      final plan = b.plan('dee');
      expect(plan.namesThem, isTrue);
      expect(plan.edits.single.seen.description, 'Boat');
      expect(plan.edits.single.split, _equal(['ana', 'ben', 'cai']));
    });

    test('named only by the entry an amendment corrects: offered as it '
        'reads now, and they come off', () {
      final (b, taxi) = _taxi();
      b.amend('ana', taxi, _equal(['ana', 'cai']));
      final plan = b.plan('ben');
      expect(plan.namesThem, isTrue);
      expect(plan.edits.single.split, _equal(['ana', 'cai']));
      b
        ..restate(plan)
        ..withdraw('ana', b.joins['ben']!);
      final folded = b.fold();
      expect(folded.setAside, isEmpty);
      expect(folded.bill.participant('ben'), isNull);
      expect(folded.bill.expenses.single.amount, 3000);
    });

    test('somebody on nothing is on nothing', () {
      final (b, _) = _taxi();
      b.join('dee');
      final plan = b.plan('dee');
      expect(plan.namesThem, isFalse);
    });

    test('two copies of one expense are one restatement', () {
      final (b, taxi) = _taxi();
      b.log.add({...taxi, 'sig': 'BBBB'});
      final plan = b.plan('ben');
      expect(plan.edits, hasLength(1));
      b.restate(plan);
      final folded = b.fold();
      expect(folded.bill.expenses, hasLength(1));
      expect(folded.bill.expenses.single.amount, 3000);
    });

    for (final (name, onlyNaming) in [
      ('an earlier amendment does not stand in for a withdrawn one', false),
      ('a withdrawn amendment naming them does not count', true),
    ]) {
      test(name, () {
        final b = _Bill()
          ..join('cai')
          ..join('dee');
        final taxi = b.expense(
          'ana',
          'taxi',
          'ana',
          3000,
          _equal(['ana', 'cai']),
          description: 'Taxi',
        );
        Map<String, dynamic>? last;
        for (final among in [
          ['ana', 'cai', 'dee'],
          if (!onlyNaming) ['ana', 'cai'],
        ]) {
          last = b.amend('ana', taxi, _equal(among));
        }
        b.withdraw('ana', last!['id'] as String);
        expect(b.plan('dee').namesThem, isFalse);
        b.withdraw('ana', b.joins['dee']!);
        final folded = b.fold();
        expect(folded.setAside, isEmpty);
        expect(folded.bill.participant('dee'), isNull);
      });
    }

    test('shares where only they held one are by hand', () {
      final b = _Bill()..join('ben');
      final ticket = b.expense('ana', 'ticket', 'ana', 3000, {
        'type': 'shares',
        'shareCounts': {'ben': 1, 'ana': 0},
      }, description: 'Ben’s ticket');
      final plan = b.plan('ben');
      expect(plan.edits, isEmpty);
      final blocker = plan.blockers.single;
      expect(blocker.block, RemovalBlock.splitByHand);
      expect(
        (blocker.entryId, blocker.description),
        (ticket['id'], 'Ben’s ticket'),
      );
    });

    test('an expense the fold sets aside still names them', () {
      final b = _Bill()..join('ben');
      final wrong = b.expense('ana', 'wrong', 'ana', 3000, {
        'type': 'exact',
        'amounts': {'ana': 1000, 'ben': 1000},
      }, description: 'Dinner');
      expect(b.fold().setAside.map((s) => s.id), contains(wrong['id']));
      final blocker = b.plan('ben').blockers.single;
      expect(blocker.block, RemovalBlock.unapplied);
      expect((blocker.entryId, blocker.description), (wrong['id'], 'Dinner'));
    });

    test('a payment from them, to them, and a confirmation by them', () {
      final b = _Bill()
        ..join('ben')
        ..join('cai');
      b.expense('ana', 'taxi', 'ana', 3000, _equal(['ana', 'ben', 'cai']));
      final paid = b.write(
        'ben',
        (h) => entries.recordPayment(
          host: h,
          paymentId: 'p1',
          to: 'ana',
          amount: 1000,
          method: 'cash',
        ),
      );
      final received = b.write(
        'cai',
        (h) => entries.recordPayment(
          host: h,
          paymentId: 'p2',
          to: 'ben',
          amount: 1,
          method: 'cash',
        ),
      );
      final confirmed = b.write(
        'ben',
        (h) => entries.confirmPayment(
          host: h,
          paymentId:
              (received['payment'] as Map<String, dynamic>)['id'] as String,
          method: 'cash',
          record:
              b.fold().paymentDigests[(received['payment']
                  as Map<String, dynamic>)['id']]!,
        ),
      );
      final blockers = b.plan('ben').blockers;
      expect(
        [for (final x in blockers) (x.block, x.entryId, x.fromThem)],
        [
          (RemovalBlock.payment, paid['id'], true),
          (RemovalBlock.payment, received['id'], false),
          (RemovalBlock.confirmation, confirmed['id'], false),
        ],
      );
    });
  });

  group('their joins', () {
    test('every join still stating them is listed; one left keeps them on', () {
      final (b, _) = _taxi();
      b.join('dee');
      final first = b.joins['dee']!;
      // Changing how Dee is paid restates her record in a second join.
      b.join('dee');
      final second = b.joins['dee']!;
      expect(second, isNot(first));

      final plan = b.plan('dee');
      expect(plan.namesThem, isFalse);
      expect(plan.joins, [first, second]);

      // One withdrawn, one standing: she is still on the bill, and the plan
      // lists only the one left.
      b.withdraw('ana', first);
      expect(b.fold().bill.participants.map((p) => p.id), contains('dee'));
      expect(b.plan('dee').joins, [second]);

      b.withdraw('ana', second);
      expect(
        b.fold().bill.participants.map((p) => p.id),
        isNot(contains('dee')),
      );
      expect(b.plan('dee').joins, isEmpty);
    });

    test('a plan whose joins changed no longer stands', () {
      final (b, _) = _taxi();
      b.join('dee');
      final before = b.plan('dee');
      b.join('dee');
      expect(before.sameAs(b.plan('dee')), isFalse);
      expect(b.plan('dee').sameAs(b.plan('dee')), isTrue);
    });
  });

  group('the same plan', () {
    test('read twice is the same', () {
      final (b, _) = _taxi();
      expect(b.plan('ben').sameAs(b.plan('ben')), isTrue);
    });

    test('once written, is not the plan any more', () {
      final (b, _) = _taxi();
      final plan = b.plan('ben');
      b.restate(plan);
      expect(b.plan('ben').namesThem, isFalse);
      expect(b.plan('ben').sameAs(plan), isFalse);
    });

    test('with a correction synced in, is not the plan any more', () {
      final (b, boat) = _boat();
      final plan = b.plan('ben');
      b
        ..join('dee')
        ..amend('cai', boat, _equal(['ana', 'ben', 'cai', 'dee']));
      final now = b.plan('ben');
      expect(now.sameAs(plan), isFalse);
      expect(now.edits.single.split, _equal(['ana', 'cai', 'dee']));
    });

    test('held back by something else, is not the plan any more', () {
      final (b, _) = _taxi();
      final plan = b.plan('ben');
      b.write(
        'ben',
        (h) => entries.recordPayment(
          host: h,
          paymentId: 'p1',
          to: 'ana',
          amount: 1000,
          method: 'cash',
        ),
      );
      expect(b.plan('ben').sameAs(plan), isFalse);
    });
  });

  test('the protocol refuses the removal the plan says is held back', () {
    // The fold's own §10.8 check agrees with the plan: a removal planned as
    // blocked is set aside with participant_still_named.
    final b = _Bill()..join('ben');
    b.expense('ben', 'hotel', 'ben', 2000, _equal(['ana', 'ben']));
    expect(b.plan('ben').namesThem, isTrue);
    final removal = b.withdraw('ana', b.joins['ben']!);
    final aside = b.fold().setAside.where((s) => s.id == removal['id']);
    expect(aside.single.code, protocol.SplitCode.participantStillNamed);
  });
}
