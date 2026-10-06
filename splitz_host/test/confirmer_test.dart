/// §10.5, §14.11: who may say a payment arrived. The payee, and — for
/// somebody the creator added by hand, who holds no key — the creator,
/// writing as them.
library;

import 'package:splitz_core/host.dart' as entries;
import 'package:splitz_core/splitz_core.dart' as protocol;
import 'package:splitz_host/splitz_host.dart';
import 'package:test/test.dart';

import 'support/fake_wallet.dart';

class _Bill {
  _Bill() {
    write(
      'ana',
      (h) => entries.createBill(
        host: h,
        name: 'Trip',
        currency: 'USD',
        creatorKey: fakeKey('ana'),
      ),
    );
    write('ana', (h) => entries.joinBill(host: h, name: 'Ana', payTo: 'u1ana'));
    write('cai', (h) => entries.joinBill(host: h, name: 'Cai', payTo: 'u1cai'));
    // Added by the creator, written as him and unsigned, as a wallet does.
    write(
      'josh',
      (h) => entries.joinBill(host: h, name: 'Josh', payTo: 'u1josh'),
    );
  }

  final Map<String, FakeHost> _hosts = {};
  final List<Map<String, dynamic>> log = [];

  FakeHost host(String who) =>
      _hosts.putIfAbsent(who, () => FakeHost(me: who, payToAddress: 'u1$who'));

  Map<String, dynamic> write(
    String who,
    Map<String, dynamic> Function(FakeHost) entry,
  ) {
    final h = host(who)..tick(Duration(minutes: log.length + 1));
    final e = entry(h);
    log.add(e);
    return e;
  }

  entries.FoldedBill fold() =>
      entries.BillLog(host('ana'), entries: log, billId: billIdOf(log)).fold();
}

final _jo = protocol.participantId(fakeKey('jo'))!;

void main() {
  _Bill paid() {
    final b = _Bill();
    b.write(
      'ana',
      (h) => entries.addExpense(
        host: h,
        expenseId: 'x1',
        paidBy: 'josh',
        amount: 3000,
        split: {
          'type': 'equal',
          'among': ['ana', 'cai', 'josh'],
        },
      ),
    );
    b.write(
      'cai',
      (h) => entries.recordPayment(
        host: h,
        paymentId: 'tx1:josh',
        to: 'josh',
        amount: 1000,
        reference: 'aa' * 32,
      ),
    );
    // Josh then joins from a device of his own, under his own key and so
    // under another id.
    b.write(
      _jo,
      (h) => entries.joinBill(
        host: h,
        name: 'Josh',
        payTo: 'u1jo',
        identityKey: fakeKey('jo'),
      ),
    );
    return b;
  }

  test('the payee confirms their own payment', () {
    final b = _Bill();
    b.write(
      'ana',
      (h) => entries.recordPayment(
        host: h,
        paymentId: 'tx2:cai',
        to: 'cai',
        amount: 500,
        reference: 'bb' * 32,
      ),
    );
    final f = b.fold();
    final pay = f.bill.payments.single;
    expect(confirmerFor(f, pay, 'cai'), 'cai');
    expect(awaitingConfirmationFor(f, 'cai').map((p) => p.id), [pay.id]);
  });

  test('the creator confirms for somebody they added, as them', () {
    final b = paid();
    final f = b.fold();
    final pay = f.bill.payments.single;
    expect(confirmerFor(f, pay, 'ana'), 'josh');
    expect(awaitingConfirmationFor(f, 'ana').map((p) => p.id), [pay.id]);
    b.write(
      'josh',
      (h) => entries.confirmPayment(
        host: h,
        paymentId: pay.id,
        method: 'recipientConfirmed',
        record: f.paymentDigests[pay.id]!,
      ),
    );
    final after = b.fold();
    expect(after.setAside, isEmpty);
    expect(after.bill.confirmedPayments, [pay.id]);
    expect(protocol.netBalances(after.bill)['cai'], 0);
    expect(awaitingConfirmationFor(after, 'ana'), isEmpty);
  });

  test('nobody else confirms for somebody added', () {
    final f = paid().fold();
    final pay = f.bill.payments.single;
    expect(confirmerFor(f, pay, 'cai'), isNull);
    expect(confirmerFor(f, pay, _jo), isNull);
    expect(awaitingConfirmationFor(f, 'cai'), isEmpty);
  });

  test('the creator does not confirm a payment they made', () {
    final b = _Bill();
    b.write(
      'ana',
      (h) => entries.recordPayment(
        host: h,
        paymentId: 'tx4:josh',
        to: 'josh',
        amount: 700,
        method: 'cash',
      ),
    );
    final f = b.fold();
    final pay = f.bill.payments.single;
    expect(pay.from, 'ana');
    expect(confirmerFor(f, pay, 'ana'), isNull);
    expect(awaitingConfirmationFor(f, 'ana'), isEmpty);
    // Josh, on a device of his own under this id, still confirms it.
    expect(confirmerFor(f, pay, 'josh'), 'josh');
  });

  test('the creator does not confirm for somebody who joined with a key', () {
    final b = _Bill();
    b.write(
      _jo,
      (h) => entries.joinBill(
        host: h,
        name: 'Jo',
        payTo: 'u1jo',
        identityKey: fakeKey('jo'),
      ),
    );
    b.write(
      'cai',
      (h) => entries.recordPayment(
        host: h,
        paymentId: 'tx3:$_jo',
        to: _jo,
        amount: 500,
        reference: 'cc' * 32,
      ),
    );
    final f = b.fold();
    final pay = f.bill.payments.single;
    expect(confirmerFor(f, pay, 'ana'), isNull);
    expect(confirmerFor(f, pay, _jo), _jo);
  });
}
