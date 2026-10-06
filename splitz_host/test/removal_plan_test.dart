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

  group('whole or not at all', () {
    test('only shared expenses name them: the plan takes them off', () {
      final (b, _) = _taxi();
      expect(b.plan('ben').complete, isTrue);
    });

    test('something they paid for keeps them on: not complete, though the '
        'shared ones are still listed', () {
      final (b, _) = _taxi();
      b.expense(
        'ben',
        'hotel',
        'ben',
        2000,
        _equal(['ana', 'ben']),
        description: 'Hotel',
      );
      final plan = b.plan('ben');
      expect(plan.edits, hasLength(1));
      expect(plan.blockers.single.block, RemovalBlock.paidFor);
      expect(plan.complete, isFalse);
    });

    test('somebody on nothing is complete with nothing to write', () {
      final b = _Bill()..join('ben');
      final plan = b.plan('ben');
      expect(plan.namesThem, isFalse);
      expect(plan.complete, isTrue);
      expect(plan.shareChanges, isEmpty);
    });
  });

  group('what a removal moves', () {
    test('an even split: their share, shared by the rest', () {
      // 30.00 among three is 10.00 each; among two, 15.00.
      final (b, _) = _taxi();
      expect(b.plan('ben').shareChanges, {
        'ana': 500,
        'ben': -1000,
        'cai': 500,
      });
    });

    test('a split with a remainder takes exactly their share and sums to '
        'zero', () {
      // 10.00 among three is 3.34, 3.33, 3.33 (§3, the extra cent to the
      // first id); among two, 5.00 each.
      final b = _Bill()
        ..join('ben')
        ..join('cai');
      b.expense('ana', 'cab', 'ana', 1000, _equal(['ana', 'ben', 'cai']));
      final changes = b.plan('ben').shareChanges;
      expect(changes, {'ana': 166, 'ben': -333, 'cai': 167});
      expect(changes.values.fold<int>(0, (a, v) => a + v), 0);
    });

    test('shares: the rest take it in proportion', () {
      // 40.00 in shares 2:1:1 is 20.00, 10.00, 10.00; without Ben, 2:1 is
      // 26.67 and 13.33.
      final b = _Bill()
        ..join('ben')
        ..join('cai');
      b.expense('ana', 'villa', 'ana', 4000, {
        'type': 'shares',
        'shareCounts': {'ana': 2, 'ben': 1, 'cai': 1},
      });
      expect(b.plan('ben').shareChanges, {
        'ana': 667,
        'ben': -1000,
        'cai': 333,
      });
    });

    test('several expenses add up, per person', () {
      final (b, _) = _taxi();
      b.expense('ana', 'cab', 'ana', 1000, _equal(['ana', 'ben', 'cai']));
      // 500 + 166 for Ana, 500 + 167 for Cai, 1000 + 333 off Ben.
      expect(b.plan('ben').shareChanges, {
        'ana': 666,
        'ben': -1333,
        'cai': 667,
      });
    });

    test('what is not restated moves nothing', () {
      // Ben paid for the hotel: it stays as it is, and only the taxi moves.
      final (b, _) = _taxi();
      b.expense('ben', 'hotel', 'ben', 2000, _equal(['ana', 'ben']));
      expect(b.plan('ben').shareChanges, {
        'ana': 500,
        'ben': -1000,
        'cai': 500,
      });
    });

    test('a running total past §2.2 is refused, not wrapped', () {
      // Two halves of the largest amount still fit; three do not.
      RemovalEdit edit(int n) => RemovalEdit(
        entryId: 'e$n',
        seen: protocol.Expense(
          id: 'x$n',
          description: '',
          paidBy: 'x',
          amount: protocol.maxAmount,
          currency: 'USD',
          at: '2026-10-05T00:00:00Z',
          split: _equal(['x', 'y']),
        ),
        author: 'x',
        split: _equal(['x']),
      );
      RemovalPlan of(int n) => RemovalPlan(
        edits: [for (var i = 0; i < n; i++) edit(i)],
        blockers: [],
      );
      expect(of(2).shareChanges, {
        'x': 9223372036854775806,
        'y': -9223372036854775806,
      });
      expect(
        () => of(3).shareChanges,
        throwsA(
          isA<protocol.SplitError>().having(
            (e) => e.code,
            'code',
            'amount_overflow',
          ),
        ),
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

  group('merging somebody added before they joined', () {
    /// Ana added 'josh' herself; Jo joined from his own device. Josh paid a
    /// 30.00 dinner split by Ana, Josh and Cai, and shares a 20.00 taxi Ana
    /// paid with Ana.
    _Bill joshAndJo() {
      final b = _Bill()
        ..join('cai')
        ..join('jo');
      b.write('josh', (h) => entries.joinBill(host: h, name: 'Josh'));
      b.expense('ana', 'dinner', 'josh', 3000, _equal(['ana', 'cai', 'josh']));
      b.expense('ana', 'taxi', 'ana', 2000, _equal(['ana', 'josh']));
      return b;
    }

    RemovalPlan merge(_Bill b, {String me = 'ana', String from = 'josh'}) =>
        planMerge(
          folded: b.fold(),
          creatorId: 'ana',
          log: b.log,
          from: from,
          into: 'jo',
          me: me,
        );

    test('every expense names Jo in his place, as payer too, and he comes '
        'off the bill in one write the fold applies', () {
      final b = joshAndJo();
      final before = protocol.netBalances(b.fold().bill);
      final plan = merge(b);
      expect(plan.complete, isTrue);
      expect(plan.edits.map((e) => e.paidBy), ['jo', null]);
      for (final e in removalEntries(host: b.host('ana'), plan: plan)) {
        b.write('ana', (_) => e);
      }
      final folded = b.fold();
      expect(folded.setAside, isEmpty);
      expect(folded.bill.participant('josh'), isNull);
      final after = protocol.netBalances(folded.bill);
      // Jo holds what Josh held; nobody else moves.
      expect(after['jo'], before['josh']! + before['jo']!);
      expect(after['ana'], before['ana']);
      expect(after['cai'], before['cai']);
    });

    test('figures add onto his, so every other figure stays', () {
      expect(
        splitMerged(
          {
            'type': 'exact',
            'amounts': {'ana': 500, 'josh': 300, 'jo': 200},
          },
          'josh',
          'jo',
        ),
        {
          'type': 'exact',
          'amounts': {'ana': 500, 'jo': 500},
        },
      );
      expect(
        splitMerged(
          {
            'type': 'shares',
            'shareCounts': {'ana': 1, 'josh': 2},
          },
          'josh',
          'jo',
        ),
        {
          'type': 'shares',
          'shareCounts': {'ana': 1, 'jo': 2},
        },
      );
      expect(
        () => splitMerged(
          {
            'type': 'exact',
            'amounts': {'josh': 0x7fffffffffffffff, 'jo': 1},
          },
          'josh',
          'jo',
        ),
        throwsA(
          isA<protocol.SplitError>().having(
            (e) => e.code,
            'code',
            protocol.SplitCode.amountOverflow,
          ),
        ),
      );
    });

    test('a list already naming both is by hand: one place for two names '
        'moves everybody else', () {
      expect(splitMerged(_equal(['jo', 'josh']), 'josh', 'jo'), isNull);
      final b = joshAndJo()
        ..expense('ana', 'boat', 'ana', 900, _equal(['ana', 'jo', 'josh']));
      final plan = merge(b);
      expect(plan.complete, isFalse);
      expect(plan.blockers.single.block, RemovalBlock.splitByHand);
    });

    test('a payment to him holds the merge back, as it holds a removal', () {
      final b = joshAndJo();
      b.write(
        'cai',
        (h) => entries.recordPayment(
          host: h,
          paymentId: 'p1',
          to: 'josh',
          amount: 1000,
          method: 'cash',
        ),
      );
      expect(merge(b).blockers.single.block, RemovalBlock.payment);
    });

    test('only the creator merges', () {
      final plan = merge(joshAndJo(), me: 'cai');
      expect(plan.mayWithdrawJoins, isFalse);
      expect(plan.complete, isFalse);
      expect(
        () => removalEntries(host: joshAndJo().host('cai'), plan: plan),
        throwsA(isA<protocol.SplitError>()),
      );
    });

    test('somebody who joined with a key of their own is never merged, and '
        'neither is nobody, or one person into themselves', () {
      final b = joshAndJo();
      final key = fakeKey('kim');
      final kim = entries.participantId(key)!;
      b.write(
        kim,
        (h) => entries.joinBill(host: h, name: 'Kim', identityKey: key),
      );
      expect(b.fold().bill.participant(kim)?.identityKey, key);
      Matcher refused(String code) => throwsA(
        isA<protocol.SplitError>().having((e) => e.code, 'code', code),
      );
      expect(
        () => merge(b, from: kim),
        refused(protocol.SplitCode.unauthorizedEntry),
      );
      expect(
        () => merge(b, from: 'nobody'),
        refused(protocol.SplitCode.unknownParticipant),
      );
      expect(
        () => merge(b, from: 'jo'),
        refused(protocol.SplitCode.unknownParticipant),
      );
      // The honest one still goes through beside them.
      expect(merge(b).complete, isTrue);
    });
  });

  group('a merge that moves a third person is by hand', () {
    // Josh, added by Ana, merges into Bo. §3 gives leftover units by id and
    // by largest remainder, so moving Josh's name or figure onto Bo can carry
    // a unit across Cai or Ana.
    final cases = <String, (Map<String, dynamic>, int, bool)>{
      'equal, no leftover crosses anybody': (
        _equal(['ana', 'cai', 'josh']),
        100,
        true,
      ),
      'equal, a leftover unit crosses cai': (
        _equal(['ana', 'cai', 'josh']),
        200,
        false,
      ),
      'percentage, the largest remainder moves': (
        {
          'type': 'percentage',
          'basisPoints': {'ana': 3334, 'bo': 3333, 'josh': 3333},
        },
        100,
        false,
      ),
      'shares, a leftover unit moves': (
        {
          'type': 'shares',
          'shareCounts': {'ana': 1, 'cai': 1, 'josh': 1},
        },
        200,
        false,
      ),
      'exact figures add and move nobody': (
        {
          'type': 'exact',
          'amounts': {'ana': 67, 'cai': 67, 'josh': 66},
        },
        200,
        true,
      ),
    };
    for (final MapEntry(key: name, value: (split, amount, whole))
        in cases.entries) {
      test(name, () {
        final b = _Bill()
          ..join('bo')
          ..join('cai');
        b.write('josh', (h) => entries.joinBill(host: h, name: 'Josh'));
        b.expense('ana', 'dinner', 'ana', amount, split);
        final before = protocol.netBalances(b.fold().bill);
        final plan = planMerge(
          folded: b.fold(),
          creatorId: 'ana',
          log: b.log,
          from: 'josh',
          into: 'bo',
          me: 'ana',
        );
        if (!whole) {
          expect(plan.complete, isFalse);
          expect(plan.blockers.single.block, RemovalBlock.splitByHand);
          return;
        }
        expect(plan.complete, isTrue);
        for (final e in removalEntries(host: b.host('ana'), plan: plan)) {
          b.write('ana', (_) => e);
        }
        final folded = b.fold();
        expect(folded.setAside, isEmpty);
        final after = protocol.netBalances(folded.bill);
        expect(after['bo'], before['josh']! + before['bo']!);
        expect(after['ana'], before['ana']);
        expect(after['cai'] ?? 0, before['cai'] ?? 0);
      });
    }
  });

  group('a closed bill', () {
    Matcher refusedClosed() => throwsA(
      isA<protocol.SplitError>().having(
        (e) => e.code,
        'code',
        protocol.SplitCode.billClosed,
      ),
    );

    /// Ana's taxi, shared with Ben and Cai, closed for settling by Ana.
    _Bill closedTaxi() {
      final b = _Bill()
        ..join('ben')
        ..join('cai')
        ..join('dee');
      b.expense('ana', 'taxi', 'ana', 3000, _equal(['ana', 'ben', 'cai']));
      b.write('ana', (h) => entries.closeFor(h, b.fold()));
      expect(b.fold().closed, isTrue);
      return b;
    }

    test('refuses a removal that restates an expense', () {
      expect(() => closedTaxi().plan('ben'), refusedClosed());
    });

    test('refuses a merge that restates an expense', () {
      final b = closedTaxi();
      b.write('josh', (h) => entries.joinBill(host: h, name: 'Josh'));
      b.expense('ana', 'cab', 'ana', 900, _equal(['ana', 'josh']));
      b.write('ana', (h) => entries.closeFor(h, b.fold()));
      expect(
        () => planMerge(
          folded: b.fold(),
          creatorId: 'ana',
          log: b.log,
          from: 'josh',
          into: 'cai',
          me: 'ana',
        ),
        refusedClosed(),
      );
    });

    test('still takes off somebody on no expense', () {
      final plan = closedTaxi().plan('dee');
      expect(plan.edits, isEmpty);
      expect(plan.complete, isTrue);
    });

    test('returns a plan a payment holds back, with that blocker', () {
      final b = closedTaxi();
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
      final plan = b.plan('ben');
      expect(plan.blockers.map((x) => x.block), [RemovalBlock.payment]);
      expect(plan.complete, isFalse);
    });

    test('plans the removal again once reopened', () {
      final b = closedTaxi();
      b.write('ana', (h) => entries.reopenFor(h, b.fold())!);
      expect(b.plan('ben').edits, hasLength(1));
    });
  });

  test('the protocol refuses the removal the plan says is held back', () {
    // The fold's own §10.8 check agrees with the plan when somebody else's
    // entry names them: the removal is set aside with participant_still_named.
    final b = _Bill()..join('ben');
    b.expense('ana', 'hotel', 'ben', 2000, _equal(['ana', 'ben']));
    expect(b.plan('ben').namesThem, isTrue);
    final removal = b.withdraw('ana', b.joins['ben']!);
    final aside = b.fold().setAside.where((s) => s.id == removal['id']);
    expect(aside.single.code, protocol.SplitCode.participantStillNamed);
  });

  test('their own entry holds the plan back but not the fold', () {
    // §10.8: an entry the person wrote never holds their removal back, so
    // one written after it cannot undo it. A removal written past the plan
    // sets their own expense aside, as the creator withdrawing it would.
    final b = _Bill()..join('ben');
    b.expense('ben', 'hotel', 'ben', 2000, _equal(['ana', 'ben']));
    expect(b.plan('ben').namesThem, isTrue);
    final removal = b.withdraw('ana', b.joins['ben']!);
    final folded = b.fold();
    expect(folded.setAside.where((s) => s.id == removal['id']), isEmpty);
    expect(folded.bill.participant('ben'), isNull);
    expect(folded.bill.expenses, isEmpty);
    expect(
      folded.setAside.map((s) => s.code),
      contains(protocol.SplitCode.unknownParticipant),
    );
  });

  group('the history of a removal', () {
    List<BillEvent> history(_Bill b) {
      final f = b.fold();
      return activityOf(
        b.log,
        f.bill,
        setAside: f.setAside,
        withdrawn: f.withdrawn,
      );
    }

    void carryOut(_Bill b, RemovalPlan plan) {
      for (final e in removalEntries(host: b.host('ana'), plan: plan)) {
        b.write('ana', (_) => e);
      }
    }

    test('a merge reads as a correction moving their part to whom they '
        'are merged into', () {
      final b = _Bill()..join('bo');
      b.write('josh', (h) => entries.joinBill(host: h, name: 'Josh'));
      final dinner = b.expense(
        'bo',
        'dinner',
        'josh',
        3000,
        _equal(['ana', 'josh']),
        description: 'Dinner',
      );
      carryOut(
        b,
        planMerge(
          folded: b.fold(),
          creatorId: 'ana',
          log: b.log,
          from: 'josh',
          into: 'bo',
          me: 'ana',
        ),
      );
      final restated = history(
        b,
      ).firstWhere((e) => e.kind == BillEventKind.expenseAmended);
      expect(restated.subject, dinner['id']);
      expect(restated.author, 'ana');
      expect(restated.amountMinorUnits, 3000);
      expect(restated.description, 'Dinner');
      expect(restated.takenOff, 'josh');
      expect(restated.movedTo, 'bo');
      expect(
        history(b).where((e) => e.kind == BillEventKind.expenseAdded),
        hasLength(1),
        reason: 'a restatement is not a second expense',
      );
    });

    test('a removal spreading their share names nobody it moved to', () {
      final b = _Bill()
        ..join('ben')
        ..join('cai');
      b.expense('ana', 'taxi', 'ana', 3000, _equal(['ana', 'ben', 'cai']));
      carryOut(b, b.plan('ben'));
      final restated = history(
        b,
      ).firstWhere((e) => e.kind == BillEventKind.expenseAmended);
      expect(restated.takenOff, 'ben');
      expect(restated.movedTo, isNull);
    });

    test('an author correcting their own expense says the new amount, and '
        'takes nobody off', () {
      final b = _Bill()..join('ben');
      final taxi = b.expense(
        'ana',
        'taxi',
        'ana',
        3000,
        _equal(['ana', 'ben']),
      );
      b.write(
        'ana',
        (h) => entries.amendEntry(
          host: h,
          targetId: taxi['id'] as String,
          member: 'expense',
          payload: {
            ...(taxi['expense'] as Map<String, dynamic>),
            'amount': 2500,
          },
        ),
      );
      final amended = history(b).first;
      expect(amended.kind, BillEventKind.expenseAmended);
      expect(amended.amountMinorUnits, 2500);
      expect(amended.takenOff, isNull);
      expect(amended.movedTo, isNull);
    });
  });

  group('who may correct an expense', () {
    test('its author, and after a merge, the creator and its author', () {
      final b = _Bill()..join('bo');
      b.write('josh', (h) => entries.joinBill(host: h, name: 'Josh'));
      b.expense('bo', 'dinner', 'josh', 3000, _equal(['ana', 'josh']));
      expect(
        expenseCorrectors(
          folded: b.fold(),
          log: b.log,
          expenseId: b.fold().bill.expenses.single.id,
        ),
        ['bo'],
      );
      final plan = planMerge(
        folded: b.fold(),
        creatorId: 'ana',
        log: b.log,
        from: 'josh',
        into: 'bo',
        me: 'ana',
      );
      for (final e in removalEntries(host: b.host('ana'), plan: plan)) {
        b.write('ana', (_) => e);
      }
      final restated = b.fold().bill.expenses.single.id;
      final who = expenseCorrectors(
        folded: b.fold(),
        log: b.log,
        expenseId: restated,
      );
      expect(who, ['ana', 'bo']);

      // Each one named is one the fold admits a correction from.
      final entryId = b.fold().expenseEntries[restated]!;
      final current = b.log.firstWhere((e) => e['id'] == entryId);
      b.write(
        'bo',
        (h) => entries.amendEntry(
          host: h,
          targetId: entryId,
          member: 'expense',
          payload: {
            ...(current['expense'] as Map<String, dynamic>),
            'amount': 2800,
          },
        ),
      );
      expect(b.fold().setAside, isEmpty);
      expect(b.fold().bill.expenses.single.amount, 2800);
    });

    test('somebody else is not named, and an unknown expense names nobody', () {
      final b = _Bill()..join('ben');
      b.expense('ben', 'taxi', 'ben', 3000, _equal(['ana', 'ben']));
      final who = expenseCorrectors(
        folded: b.fold(),
        log: b.log,
        expenseId: b.fold().bill.expenses.single.id,
      );
      expect(who, ['ben']);
      expect(
        expenseCorrectors(folded: b.fold(), log: b.log, expenseId: 'nope'),
        isEmpty,
      );
    });
  });
}
