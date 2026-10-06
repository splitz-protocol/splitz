/// §10.9 and §14.9: a bill is paid only once its creator has closed it, and
/// any change to its expenses reopens it.
library;

import 'package:splitz_core/host.dart';
import 'package:splitz_core/splitz_core.dart' as splitz;
import 'package:test/test.dart';

import 'support/fake_host.dart';

/// Ana opened the bill; Ben paid a 30.00 dinner split between them, so Ana
/// owes Ben 15.00.
({BillLog log, FakeHost ana, FakeHost ben}) _dinner() {
  final ana = FakeHost(me: 'ana', payToAddress: 'u1ana');
  final ben = FakeHost(me: 'ben', payToAddress: 'u1ben');
  final log = BillLog(ana, entries: [
    createBill(
        host: ana, name: 'Trip', currency: 'USD', creatorKey: fakeKey('ana')),
    joinBill(host: ana, name: 'Ana', payTo: 'u1ana'),
    joinBill(host: ben, name: 'Ben', payTo: 'u1ben'),
    addExpense(
      host: ben,
      expenseId: 'x1',
      paidBy: 'ben',
      amount: 3000,
      split: const {
        'type': 'equal',
        'among': ['ana', 'ben'],
      },
    ),
    setRate(host: ana, currency: 'USD', minorUnitsPerZec: 100000),
  ]);
  ana.tick();
  ben.tick();
  return (log: log, ana: ana, ben: ben);
}

Map<String, dynamic> _taxi(FakeHost who) => addExpense(
      host: who,
      expenseId: 'taxi',
      paidBy: who.me,
      amount: 1200,
      split: const {
        'type': 'equal',
        'among': ['ana', 'ben'],
      },
    );

void main() {
  test('an open bill pays nothing, and sends nothing', () async {
    final d = _dinner();
    final folded = d.log.fold();
    expect(folded.closed, isFalse);
    expect(settleRefusal(folded), splitz.SplitCode.billNotClosed);
    final owed = obligationFor(d.ana, folded)!;
    var broadcasts = 0;
    final host = _Counting(d.ana, () => broadcasts++);
    final settled = await settle(host, d.log, owed);
    expect(settled.result, SendResult.failed);
    expect(settled.code, splitz.SplitCode.billNotClosed);
    expect(
      settled.detail,
      splitz.describeCode(splitz.SplitCode.billNotClosed),
    );
    expect(broadcasts, 0, reason: 'nothing reaches the wallet');
    expect(d.log.fold().bill.payments, isEmpty);
  });

  test('closed by its creator, it is paid', () async {
    final d = _dinner();
    expect(d.log.add([closeFor(d.ana, d.log.fold())]), isEmpty);
    final folded = d.log.fold();
    expect(folded.closed, isTrue);
    expect(settleRefusal(folded), isNull);
    final settled = await settle(d.ana, d.log, obligationFor(d.ana, folded)!);
    expect(settled.result, SendResult.sent);
    expect(d.log.fold().bill.payments.single.to, 'ben');
  });

  test('an expense after the close reopens it, and the payment waits', () {
    final d = _dinner();
    d.log.add([closeFor(d.ana, d.log.fold())]);
    expect(d.log.fold().closed, isTrue);
    // Ben writes the taxi on a device that had not seen the close.
    d.log.add([_taxi(d.ben)]);
    final folded = d.log.fold();
    expect(folded.closed, isFalse);
    expect(settleRefusal(folded), splitz.SplitCode.billNotClosed);
  });

  test(
      'closed, no expense may be written; reopened, it may, and a fresh '
      'close settles the new total', () async {
    final d = _dinner();
    d.log.add([closeFor(d.ana, d.log.fold())]);
    expect(expenseRefusal(d.log.fold()), splitz.SplitCode.billClosed);

    d.ana.tick();
    d.log.add([reopenFor(d.ana, d.log.fold())!]);
    final reopened = d.log.fold();
    expect(reopened.closed, isFalse);
    expect(expenseRefusal(reopened), isNull);

    d.log.add([_taxi(d.ben)]);
    d.ana.tick();
    d.log.add([closeFor(d.ana, d.log.fold())]);
    final folded = d.log.fold();
    expect(folded.closed, isTrue);
    final owed = obligationFor(d.ana, folded)!;
    expect(owed.settlements.single.amount, 2100, reason: '15.00 + 6.00');
    expect((await settle(d.ana, d.log, owed)).result, SendResult.sent);
  });

  test('only the creator closes or reopens', () {
    final d = _dinner();
    expect(
      () => closeFor(d.ben, d.log.fold()),
      throwsA(isA<splitz.SplitError>()
          .having((e) => e.code, 'code', splitz.SplitCode.unauthorizedEntry)),
    );
    // Written by hand anyway, every fold sets it aside.
    d.log.add([closeBill(host: d.ben, covers: d.log.fold().closedOver)]);
    final folded = d.log.fold();
    expect(folded.closed, isFalse);
    expect(folded.setAside.map((s) => s.code),
        contains(splitz.SplitCode.unauthorizedEntry));

    d.log.add([closeFor(d.ana, d.log.fold())]);
    expect(
      () => reopenFor(d.ben, d.log.fold()),
      throwsA(isA<splitz.SplitError>()
          .having((e) => e.code, 'code', splitz.SplitCode.unauthorizedEntry)),
    );
    expect(reopenFor(d.ana, _dinner().log.fold()), isNull,
        reason: 'an open bill has nothing to reopen');
  });

  group('the latest close decides', () {
    test('a reopen stays a reopen when the expenses return to an older close',
        () {
      final d = _dinner();
      d.log.add([closeFor(d.ana, d.log.fold())]);
      d.ben.tick();
      final taxi = _taxi(d.ben);
      d.log.add([taxi]);
      d.ana.tick();
      d.log.add([closeFor(d.ana, d.log.fold())]);
      d.ana.tick();
      d.log.add([reopenFor(d.ana, d.log.fold())!]);
      d.ben.tick();
      d.log.add([voidEntry(host: d.ben, targetId: taxi['id'] as String)]);
      final folded = d.log.fold();
      expect(folded.closedOver, isNot(isNull));
      expect(folded.closed, isFalse,
          reason: 'the first close covers these expenses, and was superseded');
      expect(settleRefusal(folded), splitz.SplitCode.billNotClosed);
    });

    test('an expense withdrawn after a later close leaves the bill open', () {
      final d = _dinner();
      d.log.add([closeFor(d.ana, d.log.fold())]);
      d.ben.tick();
      final taxi = _taxi(d.ben);
      d.log.add([taxi]);
      d.ana.tick();
      d.log.add([closeFor(d.ana, d.log.fold())]);
      d.ben.tick();
      d.log.add([voidEntry(host: d.ben, targetId: taxi['id'] as String)]);
      expect(d.log.fold().closed, isFalse);
    });

    test('two closes over one set of expenses take one reopen', () {
      final d = _dinner();
      final seen = d.log.fold();
      final first = closeFor(d.ana, seen);
      d.ana.tick();
      d.log.add([first, closeFor(d.ana, seen)]);
      final closed = d.log.fold();
      expect(closed.closed, isTrue);
      d.ana.tick();
      d.log.add([reopenFor(d.ana, closed)!]);
      final after = d.log.fold();
      expect(after.closed, isFalse);
      expect(expenseRefusal(after), isNull);
    });

    test('with no later close, withdrawing the new expense closes it again',
        () {
      final d = _dinner();
      d.log.add([closeFor(d.ana, d.log.fold())]);
      d.ben.tick();
      final taxi = _taxi(d.ben);
      d.log.add([taxi]);
      expect(d.log.fold().closed, isFalse);
      d.ben.tick();
      d.log.add([voidEntry(host: d.ben, targetId: taxi['id'] as String)]);
      expect(d.log.fold().closed, isTrue,
          reason: 'the expenses are again exactly what the creator closed');
    });

    test('a close somebody else wrote, or withdrew, reopens nothing', () {
      final d = _dinner();
      d.log.add([closeFor(d.ana, d.log.fold())]);
      d.ben.tick();
      final forged = closeBill(host: d.ben, covers: 'A' * 22);
      d.log.add([forged]);
      expect(d.log.fold().closed, isTrue);
      d.ben.tick();
      d.log.add([voidEntry(host: d.ben, targetId: forged['id'] as String)]);
      expect(d.log.fold().closed, isTrue);
    });
  });
}

/// [inner], counting every broadcast it is asked for.
class _Counting implements BillHost {
  _Counting(this.inner, this.onBroadcast);
  final FakeHost inner;
  final void Function() onBroadcast;
  @override
  String get me => inner.me;
  @override
  Clock get now => inner.now;
  @override
  Randomness get randomBytes => inner.randomBytes;
  @override
  SignEntry? get sign => inner.sign;
  @override
  VerifyEntry? get verify => inner.verify;
  @override
  ReadsAddress? get readsAddress => inner.readsAddress;
  @override
  Broadcast get broadcast => (uri) {
        onBroadcast();
        return inner.broadcast(uri);
      };
}
