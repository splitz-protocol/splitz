// Every identifier here is one the protocol's integration guide tells a
// wallet to call, reached only through the package's public surface. If
// this stops compiling, the guide and the package have drifted apart, and
// the guide is what a wallet author has.
//
// Its output is the result, so it prints.
// ignore_for_file: avoid_print
import 'package:splitz_core/splitz_core.dart';

void main() {
  const me = 'ana';
  final bill = decodeBill(<String, dynamic>{
    'v': 1,
    'id': 'b1',
    'name': 'Dinner',
    'currency': 'EUR',
    'splitMode': 'equal',
    'participants': [
      {'id': 'ana', 'name': 'Ana'},
      {'id': 'ben', 'name': 'Ben', 'payTo': 'u1benbenben'},
    ],
    'expenses': [
      {
        'id': 'x1',
        'paidBy': 'ben',
        'amount': 9000,
        'at': '2026-10-28T19:30:00.000Z',
        'split': {
          'type': 'equal',
          'among': ['ana', 'ben'],
        },
      },
    ],
    'payments': <Object?>[],
    'rate': {
      'currency': 'EUR',
      'minorUnitsPerZec': 51234,
      'at': '2026-10-28T19:30:00.000Z',
    },
  });

  final plan = settleBill(bill);
  final rate = bill.rate;
  if (rate == null) return;

  final payments = <Zip321Payment>[];
  final unpayable = <String>[];

  for (final settlement in plan.settlements) {
    if (settlement.from != me) continue;

    final address = bill.participant(settlement.to)?.payableAddress;
    if (address == null) {
      unpayable.add(settlement.to);
      continue;
    }

    payments.add(Zip321Payment(
      address: address,
      zatoshi: fiatToZatoshi(settlement.amount, rate),
      fiat: FiatPrice(bill.currency, settlement.amount),
      label: bill.participant(settlement.to)?.name,
    ));
  }

  final uri = renderUri(payments);

  // The guide's preferred form: it reports the recipients it could not carry
  // rather than leaving the caller to drop them.
  final obligation = renderObligation(
    plan.settlements.where((s) => s.from == me).toList(),
    bill,
    rate: rate,
    skipUnpayable: true,
  );

  // A bill closes because a payment is confirmed, not because one was sent.
  const txid = 'tx-1';
  final nowIso = DateTime.now().toUtc().toIso8601String();
  final entry = <String, dynamic>{
    'v': 1,
    'author': me,
    'kind': 'recordPayment',
    'at': canonicalInstant(nowIso),
    'payment': {
      'id': txid,
      'from': me,
      'to': 'ben',
      'amount': 4500,
      'method': 'shieldedZec',
      'at': canonicalInstant(nowIso),
    },
  };
  entry['id'] = deriveEntryId(entry);

  // A log folds only when something in it opens a bill (§10.3), so the
  // createBill entry is assembled the same way: every member but `id`, and
  // then the id the protocol will check it against.
  final create = <String, dynamic>{
    'v': 1,
    'author': me,
    'kind': 'createBill',
    'at': canonicalInstant(nowIso),
    'name': 'Dinner',
    'currency': 'EUR',
    'splitMode': 'equal',
    // 32 bytes and 16 bytes, unpadded base64url — 43 and 22 characters. §9.4
    // refuses either at any other length, and the bill's id is the digest of
    // the entry that states them.
    //
    // Both fillers are `A`, which is the zero sextet. A 43-character string
    // carries two spare bits and a 22-character one carries four, and those
    // bits MUST be zero: any other final character encodes a longer byte
    // string, and the entry is refused with `create_unbound`.
    'creatorKey': 'A' * 43,
    'nonce': 'A' * 22,
  };
  create['id'] = deriveBillId(create);

  // The payment entry names a participant nobody has joined as, so the fold
  // sets it aside and reports it rather than dropping it. That report is the
  // difference between an entry that was refused and one that never arrived.
  final folded = foldLog(<Map<String, dynamic>>[create, entry]);

  print('uri        $uri');
  print('obligation ${obligation.uri}');
  print('unpayable  $unpayable');
  print('entry id   ${entry['id']}');
  print('bill id    ${create['id']}');
  print('setAside   ${folded.setAside.length}'
      '${folded.setAside.isEmpty ? '' : ' (${folded.setAside.first.code})'}');
}
