// Taking somebody off a bill by restating expenses (§10.8): the plan reads
// what the verified fold holds in force, and what it writes is folded as one
// expense per restated entry however many devices wrote it.
import 'package:splitz_core/host.dart' as splitz;
import 'package:splitz_core/splitz_core.dart' as protocol;
import 'package:splitz_host/splitz_host.dart';
import 'package:test/test.dart';

import 'support/fake_wallet.dart';

List<int> seedFor(String who) =>
    List<int>.generate(SplitsSigner.seedBytes, (i) => who.codeUnitAt(0) + i);

final signer = SplitsSigner();

class Table {
  Table._(
    this.log,
    this.bill,
    this.anaWallet,
    this.ana,
    this.benId,
    this.ben,
    this.benWallet,
    this.forger,
  );

  final List<Map<String, dynamic>> log;
  final String bill;
  final FakeWallet anaWallet;
  final WalletBillHost ana;
  final String benId;
  final WalletBillHost ben;
  final FakeWallet benWallet;

  /// Writes as ana, signed with ben's key: every verifying reader refuses it
  /// at ingress.
  final WalletBillHost forger;

  static Future<Table> open() async {
    final anaSeed = seedFor('ana');
    final benSeed = seedFor('ben');
    final anaKey = await signer.publicKeyFromSeed(anaSeed);
    final benKey = await signer.publicKeyFromSeed(benSeed);
    final anaWallet = FakeWallet();
    final ana = WalletBillHost(anaWallet, sign: signer.signerFor(anaSeed));
    final benId = splitz.participantId(benKey)!;
    final benWallet = FakeWallet(id: benId, payTo: 'u1ben');
    final ben = WalletBillHost(benWallet, sign: signer.signerFor(benSeed));
    final create = splitz.createBill(
      host: ana,
      name: 'Dinner',
      currency: 'EUR',
      creatorKey: anaKey,
    );
    final bill = create['id'] as String;
    final log = <Map<String, dynamic>>[
      await splitz.signEntry(host: ana, entry: create, billId: bill),
    ];
    anaWallet.tick();
    log.add(
      await splitz.signEntry(
        host: ana,
        entry: splitz.joinBill(
          host: ana,
          name: 'Ana',
          payTo: 'u1ana',
          identityKey: anaKey,
        ),
        billId: bill,
      ),
    );
    benWallet
      ..tick()
      ..tick();
    log.add(
      await splitz.signEntry(
        host: ben,
        entry: splitz.joinBill(
          host: ben,
          name: 'Ben',
          payTo: 'u1ben',
          identityKey: benKey,
        ),
        billId: bill,
      ),
    );
    final calWallet = FakeWallet(id: 'cal', payTo: 'u1cal');
    for (var i = 0; i < 3; i++) {
      calWallet.tick();
    }
    log.add(
      splitz.joinBill(
        host: WalletBillHost(calWallet),
        name: 'Cal',
        payTo: 'u1cal',
      ),
    );
    final forgerWallet = FakeWallet(id: 'ana', payTo: 'u1ana');
    for (var i = 0; i < 20; i++) {
      forgerWallet.tick();
    }
    return Table._(
      log,
      bill,
      anaWallet,
      ana,
      benId,
      ben,
      benWallet,
      WalletBillHost(forgerWallet, sign: signer.signerFor(benSeed)),
    );
  }

  Future<Map<String, dynamic>> expense(
    WalletBillHost host,
    String id,
    int amount,
    Map<String, dynamic> split, {
    String paidBy = 'ana',
  }) async {
    anaWallet.tick(const Duration(minutes: 1));
    benWallet.tick(const Duration(minutes: 1));
    return splitz.signEntry(
      host: host,
      entry: splitz.addExpense(
        host: host,
        expenseId: id,
        paidBy: paidBy,
        amount: amount,
        split: split,
        description: id,
      ),
      billId: bill,
    );
  }

  Future<splitz.FoldedBill> fold(List<Map<String, dynamic>> entries) =>
      foldVerified(anaWallet, entries, billId: bill, signer: signer);

  /// What [host], as [me], writes to take cal off, read from [entries].
  Future<(RemovalPlan, List<Map<String, dynamic>>)> remove(
    List<Map<String, dynamic>> entries,
    WalletBillHost host,
    String me,
  ) async {
    final folded = await fold(entries);
    final plan = planRemoval(
      folded: folded,
      creatorId: folded.creatorId,
      log: entries,
      id: 'cal',
      me: me,
    );
    if (!plan.complete) return (plan, <Map<String, dynamic>>[]);
    anaWallet.tick(const Duration(hours: 1));
    benWallet.tick(const Duration(hours: 1));
    return (
      plan,
      [
        for (final e in removalEntries(host: host, plan: plan))
          await splitz.signEntry(host: host, entry: e, billId: bill),
      ],
    );
  }
}

int total(splitz.FoldedBill f) =>
    f.bill.expenses.fold(0, (sum, e) => sum + e.amount);

void main() {
  test(
    'an honest removal plans complete and takes them off in one write',
    () async {
      final t = await Table.open();
      final e1 = await t.expense(t.ana, 'e1', 900, {
        'type': 'equal',
        'among': ['ana', t.benId, 'cal'],
      });
      final log = [...t.log, e1];
      final (plan, written) = await t.remove(log, t.ana, 'ana');
      expect(plan.complete, isTrue);
      final after = await t.fold([...log, ...written]);
      expect(after.bill.participant('cal'), isNull);
      expect(after.setAside, isEmpty);
      expect(total(after), 900);
    },
  );

  test(
    'an expense refused at ingress does not hold the removal back',
    () async {
      final t = await Table.open();
      final e1 = await t.expense(t.ana, 'e1', 900, {
        'type': 'equal',
        'among': ['ana', t.benId],
      });
      final forged = await t.expense(t.forger, 'f1', 300, {
        'type': 'equal',
        'among': ['ana', 'cal'],
      });
      final log = [...t.log, e1, forged];
      final (plan, written) = await t.remove(log, t.ana, 'ana');
      expect(plan.blockers, isEmpty);
      final after = await t.fold([...log, ...written]);
      expect(after.bill.participant('cal'), isNull);
      expect(after.setAside.map((s) => s.code), [
        protocol.SplitCode.unauthorizedEntry,
      ]);
    },
  );

  test('a forged amendment does not stand in for the applied one', () async {
    final t = await Table.open();
    final e2 = await t.expense(t.ana, 'e2', 600, {
      'type': 'equal',
      'among': ['ana', t.benId],
    });
    Map<String, dynamic> payload(List<String> among) => {
      'id': 'ana:e2',
      'paidBy': 'ana',
      'amount': 600,
      'at': e2['expense']['at'],
      'description': 'e2',
      'split': {'type': 'equal', 'among': among},
    };
    t.anaWallet.tick();
    final a1 = await splitz.signEntry(
      host: t.ana,
      entry: splitz.amendEntry(
        host: t.ana,
        targetId: e2['id'] as String,
        member: 'expense',
        payload: payload(['ana', t.benId, 'cal']),
      ),
      billId: t.bill,
    );
    final forged = await splitz.signEntry(
      host: t.forger,
      entry: splitz.amendEntry(
        host: t.forger,
        targetId: e2['id'] as String,
        member: 'expense',
        payload: payload(['ana', t.benId]),
      ),
      billId: t.bill,
    );
    final log = [...t.log, e2, a1, forged];
    final (plan, written) = await t.remove(log, t.ana, 'ana');
    expect(plan.complete, isTrue);
    expect(plan.edits.single.basis, a1['id']);
    expect(plan.shareChanges, {'ana': 100, t.benId: 100, 'cal': -200});
    final after = await t.fold([...log, ...written]);
    expect(after.bill.participant('cal'), isNull);
    expect(total(after), 600);
  });

  test('a member the split type does not read is taken out too', () async {
    final t = await Table.open();
    final junk = await t.expense(t.ana, 'e3', 400, {
      'type': 'equal',
      'among': ['ana', t.benId],
      'amounts': {'cal': 1},
    });
    final log = [...t.log, junk];
    final (plan, written) = await t.remove(log, t.ana, 'ana');
    expect(plan.complete, isTrue);
    expect(
      (plan.edits.single.split['amounts'] as Map).containsKey('cal'),
      isFalse,
    );
    final after = await t.fold([...log, ...written]);
    expect(after.bill.participant('cal'), isNull);
    expect(after.setAside, isEmpty);
  });

  test(
    'the creator and the author restating one expense at once leave one',
    () async {
      final t = await Table.open();
      // Ben's expense: the creator and its author may both restate it.
      final e1 = await t.expense(t.ben, 'e1', 3000, {
        'type': 'equal',
        'among': ['ana', t.benId, 'cal'],
      }, paidBy: t.benId);
      final log = [...t.log, e1];
      final (_, byAna) = await t.remove(log, t.ana, 'ana');
      // Ben may not take cal off (§10.8), so his plan writes nothing; he
      // restates his own expense without cal meanwhile.
      final (benPlan, nothing) = await t.remove(log, t.ben, t.benId);
      expect(benPlan.complete, isFalse);
      expect(benPlan.mayWithdrawJoins, isFalse);
      expect(nothing, isEmpty);
      final byBen = [
        await splitz.signEntry(
          host: t.ben,
          entry: splitz.restateExpense(
            host: t.ben,
            targetId: e1['id'] as String,
            basis: null,
            expenseId: 'e1-again',
            paidBy: t.benId,
            amount: 3000,
            split: {
              'type': 'equal',
              'among': ['ana', t.benId],
            },
          ),
          billId: t.bill,
        ),
      ];
      final after = await t.fold([...log, ...byAna, ...byBen]);
      expect(after.bill.expenses, hasLength(1));
      expect(total(after), 3000);
      expect(after.bill.participant('cal'), isNull);
      expect(after.setAside.map((s) => s.code), [
        protocol.SplitCode.restatementSuperseded,
      ]);
    },
  );

  test(
    'one creator on two devices restating at once leaves one expense',
    () async {
      final t = await Table.open();
      final e1 = await t.expense(t.ana, 'e1', 3000, {
        'type': 'equal',
        'among': ['ana', t.benId, 'cal'],
      });
      final log = [...t.log, e1];
      final tabletWallet = FakeWallet();
      for (var i = 0; i < 300; i++) {
        tabletWallet.tick();
      }
      final tablet = WalletBillHost(
        tabletWallet,
        sign: signer.signerFor(seedFor('ana')),
      );
      final (_, phone) = await t.remove(log, t.ana, 'ana');
      final (_, pad) = await t.remove(log, tablet, 'ana');
      final after = await t.fold([...log, ...phone, ...pad]);
      expect(after.bill.expenses, hasLength(1));
      expect(total(after), 3000);
      expect(after.bill.participant('cal'), isNull);
    },
  );

  test('a correction written meanwhile is kept, and the person stays until '
      'the removal is planned again', () async {
    final t = await Table.open();
    final e1 = await t.expense(t.ben, 'e1', 4000, {
      'type': 'equal',
      'among': ['ana', t.benId, 'cal'],
    }, paidBy: t.benId);
    final log = [...t.log, e1];
    final (_, removal) = await t.remove(log, t.ana, 'ana');
    t.benWallet.tick(const Duration(hours: 2));
    final corrected = await splitz.signEntry(
      host: t.ben,
      entry: splitz.amendEntry(
        host: t.ben,
        targetId: e1['id'] as String,
        member: 'expense',
        payload: {
          ...Map<String, dynamic>.from(e1['expense'] as Map),
          'amount': 5000,
        },
      ),
      billId: t.bill,
    );
    final merged = [...log, ...removal, corrected];
    final after = await t.fold(merged);
    expect(total(after), 5000);
    expect(after.bill.participant('cal'), isNotNull);
    expect(after.setAside.map((s) => s.code).toSet(), {
      protocol.SplitCode.restatementStale,
      protocol.SplitCode.participantStillNamed,
    });
    final (plan, again) = await t.remove(merged, t.ana, 'ana');
    expect(plan.complete, isTrue);
    final done = await t.fold([...merged, ...again]);
    expect(total(done), 5000);
    expect(done.bill.participant('cal'), isNull);
  });

  test('entries for a plan that is not complete are refused', () async {
    final t = await Table.open();
    final paidByCal = await t.expense(t.ana, 'e1', 900, {
      'type': 'equal',
      'among': ['ana', 'cal'],
    }, paidBy: 'cal');
    final log = [...t.log, paidByCal];
    final folded = await t.fold(log);
    final plan = planRemoval(
      folded: folded,
      creatorId: folded.creatorId,
      log: log,
      id: 'cal',
      me: 'ana',
    );
    expect(plan.complete, isFalse);
    expect(
      () => removalEntries(host: t.ana, plan: plan),
      throwsA(
        isA<protocol.SplitError>().having(
          (e) => e.code,
          'code',
          protocol.SplitCode.participantStillNamed,
        ),
      ),
    );
  });

  test(
    'a removal by somebody who may not withdraw the join is refused',
    () async {
      final t = await Table.open();
      final e1 = await t.expense(t.ben, 'e1', 900, {
        'type': 'equal',
        'among': ['ana', t.benId, 'cal'],
      }, paidBy: t.benId);
      final log = [...t.log, e1];
      final folded = await t.fold(log);
      final plan = planRemoval(
        folded: folded,
        creatorId: folded.creatorId,
        log: log,
        id: 'cal',
        me: t.benId,
      );
      expect(plan.blockers, isEmpty);
      expect(plan.complete, isFalse);
      expect(
        () => removalEntries(host: t.ben, plan: plan),
        throwsA(
          isA<protocol.SplitError>().having(
            (e) => e.code,
            'code',
            protocol.SplitCode.unauthorizedEntry,
          ),
        ),
      );
      // The person themselves may leave.
      final own = planRemoval(
        folded: folded,
        creatorId: folded.creatorId,
        log: log,
        id: 'cal',
        me: 'cal',
      );
      expect(own.mayWithdrawJoins, isTrue);
    },
  );

  group('after a removal restates somebody else\'s expense', () {
    Future<(Table, Map<String, dynamic>, String, List<Map<String, dynamic>>)>
    restated() async {
      final t = await Table.open();
      final e2 = await t.expense(t.ben, 'e2', 900, {
        'type': 'equal',
        'among': ['ana', t.benId, 'cal'],
      }, paidBy: t.benId);
      final log = [...t.log, e2];
      final (plan, written) = await t.remove(log, t.ana, 'ana');
      expect(plan.complete, isTrue);
      final restatement =
          written.firstWhere((e) => e['kind'] == 'addExpense')['id'] as String;
      return (t, e2, restatement, [...log, ...written]);
    }

    Future<Map<String, dynamic>> amend(
      Table t,
      Map<String, dynamic> target,
      Map<String, dynamic> payload,
    ) async {
      t.benWallet.tick(const Duration(hours: 2));
      return splitz.signEntry(
        host: t.ben,
        entry: splitz.amendEntry(
          host: t.ben,
          targetId: target['id'] as String,
          member: 'expense',
          payload: payload,
        ),
        billId: t.bill,
      );
    }

    test('its author corrects it, and cal stays off (§10.8)', () async {
      final (t, _, restatement, log) = await restated();
      final written = log.firstWhere((e) => e['id'] == restatement);
      final fix = await amend(t, written, {
        ...(written['expense'] as Map).cast<String, dynamic>(),
        'amount': 950,
      });
      final after = await t.fold([...log, fix]);
      expect(after.setAside, isEmpty);
      expect(total(after), 950);
      expect(after.bill.participant('cal'), isNull);
      final held = splitz.BillLog(t.ben, entries: log, billId: t.bill);
      expect(held.refusalOf(fix), isNull);
    });

    test(
      'amending the replaced original is refused before it is written',
      () async {
        final (t, e2, _, log) = await restated();
        final back = await amend(
          t,
          e2,
          (e2['expense'] as Map).cast<String, dynamic>(),
        );
        final held = splitz.BillLog(t.ben, entries: log, billId: t.bill);
        expect(held.refusalOf(back), protocol.SplitCode.unauthorizedEntry);
        // The fold alone would admit it and put cal back: what the host's
        // refusal stands against.
        final admitted = await t.fold([...log, back]);
        expect(admitted.bill.participant('cal'), isNotNull);
      },
    );

    test(
      'the creator takes a restated expense off, and cal stays off',
      () async {
        final (t, e2, restatement, log) = await restated();
        final folded = await t.fold(log);
        final target = expenseWithdrawalTarget(
          folded: folded,
          log: log,
          expenseId: folded.bill.expenses.single.id,
          me: 'ana',
        )!;
        t.anaWallet.tick(const Duration(hours: 2));
        final off = await splitz.signEntry(
          host: t.ana,
          entry: splitz.voidEntry(host: t.ana, targetId: target),
          billId: t.bill,
        );
        final after = await t.fold([...log, off]);
        expect(after.bill.expenses, isEmpty);
        expect(after.bill.participant('cal'), isNull);
        // Control: withdrawing the restatement instead puts cal back.
        t.anaWallet.tick(const Duration(hours: 2));
        final back = await splitz.signEntry(
          host: t.ana,
          entry: splitz.voidEntry(host: t.ana, targetId: restatement),
          billId: t.bill,
        );
        final undone = await t.fold([...log, back]);
        expect(undone.bill.participant('cal'), isNotNull);
        expect(undone.bill.expenses.single.amount, 900);
      },
    );

    test("the creator's own restated expense comes off the same way", () async {
      final t = await Table.open();
      final e1 = await t.expense(t.ana, 'e1', 600, {
        'type': 'equal',
        'among': ['ana', 'cal'],
      }, paidBy: 'ana');
      final log = [...t.log, e1];
      final (plan, written) = await t.remove(log, t.ana, 'ana');
      expect(plan.complete, isTrue);
      final all = [...log, ...written];
      final folded = await t.fold(all);
      final target = expenseWithdrawalTarget(
        folded: folded,
        log: all,
        expenseId: folded.bill.expenses.single.id,
        me: 'ana',
      );
      expect(target, e1['id']);
      t.anaWallet.tick(const Duration(hours: 2));
      final off = await splitz.signEntry(
        host: t.ana,
        entry: splitz.voidEntry(host: t.ana, targetId: target!),
        billId: t.bill,
      );
      final after = await t.fold([...all, off]);
      expect(after.bill.expenses, isEmpty);
      expect(after.bill.participant('cal'), isNull);
    });

    test('an expense nobody restated is withdrawn by its own entry', () async {
      final t = await Table.open();
      final e1 = await t.expense(t.ben, 'e1', 300, {
        'type': 'equal',
        'among': ['ana', t.benId],
      }, paidBy: t.benId);
      final log = [...t.log, e1];
      final folded = await t.fold(log);
      for (final me in ['ana', t.benId]) {
        expect(
          expenseWithdrawalTarget(
            folded: folded,
            log: log,
            expenseId: folded.bill.expenses.single.id,
            me: me,
          ),
          e1['id'],
        );
      }
    });

    test('its author withdraws it by their own first entry', () async {
      final (t, e2, restatement, log) = await restated();
      final folded = await t.fold(log);
      final restatedId = folded.bill.expenses.single.id;
      expect(
        expenseWithdrawalTarget(
          folded: folded,
          log: log,
          expenseId: restatedId,
          me: t.benId,
        ),
        e2['id'],
      );
      // The creator, who may withdraw any expense, withdraws the first entry
      // too: withdrawing the restatement would put cal back.
      expect(
        expenseWithdrawalTarget(
          folded: folded,
          log: log,
          expenseId: restatedId,
          me: 'ana',
        ),
        e2['id'],
      );
      // Somebody who may withdraw neither is given the entry in force.
      expect(
        expenseWithdrawalTarget(
          folded: folded,
          log: log,
          expenseId: restatedId,
          me: 'cal',
        ),
        restatement,
      );
      t.benWallet.tick(const Duration(hours: 2));
      final off = await splitz.signEntry(
        host: t.ben,
        entry: splitz.voidEntry(host: t.ben, targetId: e2['id'] as String),
        billId: t.bill,
      );
      final after = await t.fold([...log, off]);
      expect(after.bill.expenses, isEmpty);
      expect(after.bill.participant('cal'), isNull);
    });
  });
}
