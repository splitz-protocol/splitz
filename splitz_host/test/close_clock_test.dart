/// §10.9: a close or reopen the creator writes is dated after every close
/// and reopen in the log, so a second device whose clock trails the first
/// still closes, or reopens, the bill it reads.
library;

import 'dart:typed_data';

import 'package:splitz_core/host.dart' as splitz;
import 'package:test/test.dart';

class _Host extends splitz.BillHost {
  _Host(this._me, this._start);
  final String _me;
  final DateTime _start;
  int _n = 0;
  @override
  String get me => _me;
  @override
  splitz.Clock get now =>
      () => _start.add(Duration(seconds: _n++));
  @override
  splitz.Randomness get randomBytes =>
      (n) => Uint8List.fromList(
        List<int>.generate(n, (i) => i + _me.codeUnitAt(0) + _start.minute),
      );
  @override
  splitz.Broadcast get broadcast =>
      (uri) async => throw StateError('no');
}

void main() {
  final t = DateTime.utc(2026, 10, 28, 19);

  (List<Map<String, dynamic>>, splitz.FoldedBill Function()) bill() {
    final ana = _Host('ana', t);
    final ben = _Host('ben', t);
    final log = <Map<String, dynamic>>[];
    log.add(
      splitz.createBill(
        host: ana,
        name: 'Trip',
        currency: 'USD',
        creatorKey: 'A' * 43,
      ),
    );
    log.add(splitz.joinBill(host: ana, name: 'Ana', payTo: 'u1ana'));
    log.add(splitz.joinBill(host: ben, name: 'Ben', payTo: 'u1ben'));
    log.add(
      splitz.addExpense(
        host: ben,
        expenseId: 'x1',
        paidBy: 'ben',
        amount: 3000,
        split: {
          'type': 'equal',
          'among': ['ana', 'ben'],
        },
      ),
    );
    return (log, () => splitz.BillLog(ana, entries: log).fold());
  }

  for (final behind in [0, 2, 10, 600]) {
    test('a close from a device $behind minutes behind the reopen closes', () {
      final (log, fold) = bill();
      final phone = _Host('ana', t.add(const Duration(minutes: 10)));
      final tablet = _Host('ana', t.add(Duration(minutes: 10 - behind)));
      log.add(splitz.closeFor(phone, fold()));
      log.add(splitz.reopenFor(phone, fold())!);
      expect(fold().closed, isFalse);
      log.add(splitz.closeFor(tablet, fold()));
      expect(fold().closed, isTrue);
      expect(splitz.settleRefusal(fold()), isNull);
    });
  }

  test('a reopen from a device behind the close reopens', () {
    final (log, fold) = bill();
    final phone = _Host('ana', t.add(const Duration(minutes: 10)));
    final tablet = _Host('ana', t);
    log.add(splitz.closeFor(phone, fold()));
    log.add(splitz.reopenFor(tablet, fold())!);
    expect(fold().closed, isFalse);
    log.add(splitz.closeFor(tablet, fold()));
    expect(fold().closed, isTrue);
  });

  test('a device whose clock is ahead keeps its own time', () {
    final (log, fold) = bill();
    final phone = _Host('ana', t);
    final ahead = _Host('ana', t.add(const Duration(hours: 1)));
    log.add(splitz.closeFor(phone, fold()));
    final reopen = splitz.reopenFor(ahead, fold())!;
    expect(reopen['at'], '2026-10-28T20:00:00.000Z');
  });
}
