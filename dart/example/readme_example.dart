// A bill from nothing to a payment request, compiled and runnable.
//
// This is the shortest path through the package: open a bill, add the people
// on it, put an expense on it, price it, and ask what this device owes. If it
// stops building, the package's own walkthrough is broken.
//
// ignore_for_file: avoid_print

import 'dart:typed_data';

import 'package:splitz_core/host.dart';

/// Stands in for the wallet. A real one signs, holds a secure random source
/// and sends; this one is enough to show the shape.
class MyWallet extends BillHost {
  MyWallet({required this.me, required this.payToAddress});

  @override
  final String me;

  final String? payToAddress;

  @override
  Clock get now => DateTime.now;

  @override
  Randomness get randomBytes => (n) => Uint8List(n);

  @override
  Broadcast get broadcast =>
      (uri) async => const Sent.pending(detail: 'created, not broadcast');
}

void showWhoCannotBePaid(List<Object?> who) => print('cannot pay: $who');
void showWhatIsStillPending(List<Awaiting> a) =>
    print('pending: ${a.map((x) => '${x.to} paid ${x.paid} of ${x.owed}')}');
void showPaid(String txid) => print('paid: $txid');
void showCheckBeforeRetrying(String? detail) => print('pending: $detail');
void showFailed(String? detail) => print('failed: $detail');

Future<void> main() async {
  final ana = MyWallet(me: 'ana', payToAddress: 'u1ana');
  final ben = MyWallet(me: 'ben', payToAddress: 'u1ben');
  final anaPublicKey = 'k' * 43;

  final log = BillLog(ana);
  log.add([
    createBill(
        host: ana, name: 'Dinner', currency: 'EUR', creatorKey: anaPublicKey),
  ]);
  log.add([joinBill(host: ana, name: 'Ana', payTo: 'u1ana')]);
  log.add([joinBill(host: ben, name: 'Ben', payTo: 'u1ben')]);
  log.add([
    addExpense(
      host: ana,
      expenseId: 'x1',
      paidBy: 'ana',
      amount: 9000,
      split: const {
        'type': 'equal',
        'among': ['ana', 'ben'],
      },
    ),
  ]);
  log.add([setRate(host: ana, currency: 'EUR', minorUnitsPerZec: 51234)]);

  final folded = log.fold();
  final owed = obligationFor(ben, folded);

  if (owed != null && owed.uri != null) {
    if (!owed.isComplete) showWhoCannotBePaid(owed.unpayable);
    if (owed.awaiting.isNotEmpty) showWhatIsStillPending(owed.awaiting);

    final settled = await settle(ben, log, owed);
    switch (settled.result) {
      case SendResult.sent:
        showPaid(settled.txid!);
      case SendResult.pending:
        showCheckBeforeRetrying(settled.detail);
      case SendResult.failed:
        showFailed(settled.detail);
    }
  }

  print('entries: ${log.entries.length}, set aside: ${folded.setAside.length}');
  print('ben owes: ${owed?.settlements.map((s) => '${s.to}=${s.amount}')}');
}
