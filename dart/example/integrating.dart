// The protocol-level walkthrough INTEGRATING.md's "Dart" section shows,
// compiled and run by tools/examples/run.sh. `tools/docs/blocks.py` fails when
// the document and this file drift.
//
// ignore_for_file: avoid_print
// docs:begin
import 'package:splitz_core/splitz_core.dart';

// A bill: Ben and Cai each paid 30.00 split with Ana, so Ana owes each of
// them 15.00, and it carries the rate §7 snapshotted onto it. A wallet has
// this from a fold.
final bill = decodeBill({
  'v': 1,
  'id': 'b1',
  'currency': 'EUR',
  'participants': [
    {'id': 'ana', 'name': 'Ana', 'payTo': 'u1ana0000000000000000000'},
    {'id': 'ben', 'name': 'Ben', 'payTo': 'u1ben0000000000000000000'},
    {'id': 'cai', 'name': 'Cai', 'payTo': 'u1cai0000000000000000000'},
  ],
  'expenses': [
    {
      'id': 'x1',
      'paidBy': 'ben',
      'amount': 3000,
      'at': '2026-10-28T19:30:00.000Z',
      'split': {
        'type': 'equal',
        'among': ['ana', 'ben'],
      },
    },
    {
      'id': 'x2',
      'paidBy': 'cai',
      'amount': 3000,
      'at': '2026-10-28T19:31:00.000Z',
      'split': {
        'type': 'equal',
        'among': ['ana', 'cai'],
      },
    },
  ],
  'rate': {
    'currency': 'EUR',
    'minorUnitsPerZec': 300000,
    'at': '2026-10-28T19:32:00.000Z',
  },
});

const me = 'ana';

void main() {
  final plan = settleBill(bill); // fewest payments
  // A bill with no `setRate` entry is an ordinary bill, so this is a branch
  // and not a `!`. There is no refusal code for "unpriced": it is not an
  // error, and there is nothing for a `SplitError` handler to catch.
  final rate = bill.rate;
  if (rate == null) return;

  final payments = <Zip321Payment>[];
  final paidTo = <Settlement>[];
  for (final settlement in plan.settlements) {
    if (settlement.from != me) continue;

    final address = bill.participant(settlement.to)?.payableAddress;
    if (address == null) {
      // Reported, never dropped: a dropped output settles less than the plan
      // says it does, and the payer cannot tell.
      print('cannot pay ${settlement.to} here');
      continue;
    }

    payments.add(
      Zip321Payment(
        address: address,
        zatoshi: fiatToZatoshi(settlement.amount, rate),
        fiat: FiatPrice(bill.currency, settlement.amount),
        label: bill.participant(settlement.to)?.name,
      ),
    );
    paidTo.add(settlement);
  }

  final uri = renderUri(payments); // one transaction
  print('request: $uri');
  const txid = 'tx-from-the-wallet'; // what the wallet's broadcast returned

  // A bill closes because a payment is confirmed, not because one was sent.
  // One transaction paying two people is two records, and §10.5 requires each
  // to carry its own id: under one id the second is set aside and its payee
  // asked to be paid again. The transaction goes in `reference`, and each
  // record states what it sent in ZEC and the rate it was priced at, so the
  // payee confirms against a figure they can compare with what arrived.
  //
  // The protocol import exports no entry builder: at this level the wallet
  // assembles the map and derives its §9.5 id with `deriveEntryId`.
  // `package:splitz_core/host.dart` has `recordSend`, which does exactly this.
  // `canonicalInstant` takes a string, not a clock value: this library
  // exports nothing that reads a clock, because a clock is the host's (§13).
  final at = canonicalInstant('2026-10-28T19:40:00.000Z');
  final records = <Map<String, dynamic>>[];
  for (var i = 0; i < paidTo.length; i++) {
    final settlement = paidTo[i];
    final entry = <String, dynamic>{
      'v': 1,
      'author': me,
      'kind': 'recordPayment',
      'at': at,
      'payment': {
        'id': '$txid:${settlement.to}',
        'from': me,
        'to': settlement.to,
        'amount': settlement.amount,
        'method': 'shieldedZec',
        'at': at,
        'reference': txid,
        'zatoshi': payments[i].zatoshi,
        'paidAtRate': rateToJson(rate),
      },
    };
    entry['id'] = deriveEntryId(entry); // §9.5: the id IS the digest
    records.add(checkEntry(entry));
  }

  final ids = {for (final r in records) (r['payment'] as Map)['id']};
  print('records: ${records.length}, ids: ${ids.join(', ')}');
  if (records.length != 2 || ids.length != 2) {
    throw StateError('one record per payee, each its own id');
  }
}
