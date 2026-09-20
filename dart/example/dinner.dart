/// A dinner for three, from expenses to one payer's payment request.
///
/// Run with `dart run example/dinner.dart` from the `dart` directory.
library;

import 'package:splitz_core/splitz_core.dart';

void main() {
  const at = '2026-10-28T19:30:00.000Z';

  final bill = Bill(
    id: 'weekend',
    name: 'Zcon7 dinner',
    currency: 'MXN',
    participants: const [
      Participant(id: 'ana', name: 'Ana', payTo: 'u1ana000000000000000000'),
      Participant(id: 'ben', name: 'Ben', payTo: 'u1ben000000000000000000'),
      Participant(id: 'cai', name: 'Cai', payTo: 'u1cai000000000000000000'),
    ],
    expenses: const [
      // Ana covered dinner, split evenly.
      Expense(
        id: 'e1',
        description: 'dinner',
        paidBy: 'ana',
        amount: 480000, // 4800.00 MXN, in minor units
        currency: 'MXN',
        at: at,
        split: {
          'type': 'equal',
          'among': ['ana', 'ben', 'cai'],
        },
      ),
      // Ben covered the taxi, and only he and Cai rode in it.
      Expense(
        id: 'e2',
        description: 'taxi',
        paidBy: 'ben',
        amount: 30000,
        currency: 'MXN',
        at: at,
        split: {
          'type': 'equal',
          'among': ['ben', 'cai'],
        },
      ),
    ],
    // The rate is snapshotted into the bill, not looked up per device: six
    // people applying six live rates to one dinner compute six different
    // amounts and the bill never closes.
    rate: ExchangeRate(
      currency: 'MXN',
      minorUnitsPerZec: 950000,
      at: at,
    ),
  );

  final net = netBalances(bill);
  print('Net positions');
  for (final id in sortedUtf8(net.keys)) {
    print('  ${id.padRight(4)} ${_money(net[id]!)}');
  }

  print('\nDebts as they arose');
  for (final d in directDebts(bill)) {
    print('  ${d.from} owes ${d.to} ${_money(d.amount)}');
  }

  final plan = settleBill(bill);
  print('\nSettlement: ${plan.paymentCount} payments'
      '${plan.isOptimal ? '' : ' (not proven minimal)'}');
  for (final s in plan.settlements) {
    print('  ${s.from} pays ${s.to} ${_money(s.amount)}');
    // Netting reroutes payments (§6.3). A settlement carries the debts it
    // discharges so a wallet can explain one rather than ask for trust.
    if (s.isRerouted) {
      for (final c in s.covers) {
        print('       ${_money(c.amount).padLeft(9)} of what '
            '${s.from} owes ${c.to}');
      }
    }
  }

  // One payer's whole obligation becomes one transaction, one output per
  // recipient.
  final payer = plan.settlements.first.from;
  final owed = [
    for (final s in plan.settlements)
      if (s.from == payer) s
  ];
  final payments = <Zip321Payment>[];
  for (final s in owed) {
    final address = bill.participant(s.to)?.payableAddress;
    if (address == null) {
      // A recipient the request cannot carry is reported, never dropped: a
      // dropped output settles less than the plan says it does.
      print('\n  cannot pay ${s.to}: no published address');
      continue;
    }
    payments.add(Zip321Payment(
      address: address,
      zatoshi: fiatToZatoshi(s.amount, bill.rate!),
      fiat: FiatPrice(bill.currency, s.amount),
      label: bill.participant(s.to)?.name,
    ));
  }

  print('\n$payer pays in ONE transaction:');
  for (var i = 0; i < owed.length; i++) {
    print('  ${_money(owed[i].amount)} to ${owed[i].to}'
        '  (${payments[i].zatoshi} zatoshi)');
  }
  print('\n${renderUri(payments, includeFiat: true)}');
}

/// Minor units as major units. MXN has an ISO 4217 exponent of 2; the protocol
/// does not carry the exponent, so a caller supplies it (SPEC.md §2.1).
String _money(int minorUnits) {
  final sign = minorUnits < 0 ? '-' : '';
  final m = minorUnits.abs();
  return '$sign\$${m ~/ 100}.${(m % 100).toString().padLeft(2, '0')}';
}
