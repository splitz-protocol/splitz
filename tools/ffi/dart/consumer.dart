/// A Dart wallet over the callback-free binding.
///
/// It holds its own entries, keeps its own clock and its own randomness, and
/// hands the library facts rather than implementing seven interfaces. Nothing
/// calls back into Dart.
import 'dart:convert';
import 'dart:typed_data';

import 'package:splitz_dart_consumer/splitz_ffi.dart';

int failures = 0;
void check(String name, bool ok, String saw) {
  print('  ${ok ? "PASS" : "FAIL"}  $name — $saw');
  if (!ok) failures += 1;
}

/// One device: its log, its clock, its randomness.
class Device {
  Device(this.me, this.payTo, this.seedByte);
  final String me;
  final String? payTo;
  final int seedByte;
  final List<String> entries = [];
  int minute = 0;

  /// A §9.3 instant: UTC, exactly three fractional digits, fixed width.
  String now() {
    minute += 1;
    final total = 19 * 60 + 30 + minute;
    return '2026-10-28T${(total ~/ 60).toString().padLeft(2, "0")}:'
        '${(total % 60).toString().padLeft(2, "0")}:00.000Z';
  }

  /// §9.4 derives a bill's id from this. A shipped wallet uses the platform's
  /// own entropy.
  Uint8List nonce() => Uint8List.fromList(
    List.generate(16, (i) => (seedByte + minute + i) & 0xff),
  );

  HostFacts facts() =>
      HostFacts(me: me, payTo: payTo, now: now(), nonce: nonce());

  /// The Ed25519 seed this account signs with, as §9.4 writes a key: unpadded
  /// base64url, which is what a keychain holds.
  String signingSeed() => base64Url
      .encode(List.generate(32, (i) => (seedByte + i) & 0xff))
      .replaceAll('=', '');

  void add(String entry) {
    final merged = mergeEntries(entries, [entry]);
    entries
      ..clear()
      ..addAll(merged.entries);
  }
}

void main(List<String> args) {
  configureDefaultBindings(libraryPath: args[0]);

  final ana = Device('ana', 'u1ana', 1);
  final ben = Device('ben', 'u1ben', 90);

  print('ana opens a bill and joins it');
  final anaKey = identityKeyFromSeed(ana.signingSeed());
  final create = createBillEntry(
    ana.facts(),
    'Dinner',
    'EUR',
    'equal',
    anaKey,
    ana.signingSeed(),
  );
  ana.add(create);
  final billId = (jsonDecode(create) as Map)['id'] as String;
  check('the bill has a §9.4 id', billId.isNotEmpty, billId);

  ana.add(
    joinBillEntry(ana.facts(), 'Ana', 'u1ana', anaKey, ana.signingSeed()),
  );

  print('ben joins, and the two logs merge');
  final benKey = identityKeyFromSeed(ben.signingSeed());
  ben.entries.addAll(ana.entries);
  ben.add(
    joinBillEntry(ben.facts(), 'Ben', 'u1ben', benKey, ben.signingSeed()),
  );

  print('ana adds an expense they share, and prices it');
  ana.entries
    ..clear()
    ..addAll(mergeEntries(ana.entries, ben.entries).entries);
  ana.add(
    addExpenseEntry(
      ana.facts(),
      'x1',
      'ana',
      9000,
      '{"type":"equal","among":["ana","ben"]}',
      'dinner',
      ana.signingSeed(),
    ),
  );
  ana.add(
    setRateEntry(ana.facts(), 'EUR', 300000, 'a fixed feed', ana.signingSeed()),
  );

  final folded = foldEntries(ana.facts(), billId, ana.entries);
  check(
    'both people are on the bill',
    folded.bill.participants.length == 2,
    folded.bill.participants.map((p) => p.id).join(', '),
  );
  check('nothing was set aside', folded.setAside.isEmpty, '${folded.setAside}');
  check(
    'both keys are bound under §10.7',
    folded.identities.bound.length == 2,
    '${folded.identities.bound.keys}',
  );
  check(
    'no identity is contested',
    folded.identities.contested.isEmpty,
    '${folded.identities.contested}',
  );
  check(
    'the expense is nine thousand minor units',
    folded.bill.expenses.single.amount == 9000,
    '${folded.bill.expenses.single.amount}',
  );

  print('ben owes half of it');
  ben.entries
    ..clear()
    ..addAll(mergeEntries(ben.entries, ana.entries).entries);
  final owed = obligationOf(ben.facts(), billId, ben.entries, const []);
  check('ben has an obligation', owed != null, owed?.request.uri ?? 'none');
  final settlement = owed!.settlements.single;
  check(
    'it is four and a half thousand to ana',
    settlement.to == 'ana' && settlement.amount == 4500,
    '${settlement.to} ${settlement.amount}',
  );
  check(
    'the request is a ZIP 321 URI naming ana\'s address',
    owed.request.uri!.startsWith('zcash:u1ana'),
    owed.request.uri!,
  );
  check(
    'nothing is withheld',
    owed.request.withheldMinorUnits == 0,
    '${owed.request.withheldMinorUnits}',
  );

  print('the wallet sends it, then records what §14.3 allows');
  ben.add(
    recordPaymentEntry(
      ben.facts(),
      const PaymentDraft(
        paymentId: 'tx-ben-1',
        to: 'ana',
        amount: 4500,
        method: 'shieldedZec',
        reference: null,
        zatoshi: null,
        note: null,
      ),
      ben.signingSeed(),
    ),
  );
  ana.entries
    ..clear()
    ..addAll(mergeEntries(ana.entries, ben.entries).entries);
  final afterPayment = foldEntries(ana.facts(), billId, ana.entries);
  check(
    'ana sees the payment',
    afterPayment.bill.payments.length == 1,
    '${afterPayment.bill.payments.map((p) => p.id)}',
  );
  check(
    'and it is not confirmed',
    afterPayment.bill.confirmedPayments.isEmpty,
    '${afterPayment.bill.confirmedPayments}',
  );
  final stillOwed = obligationOf(ben.facts(), billId, ben.entries, const [])!;
  check(
    'so ben is asked for nothing twice',
    stillOwed.settlements.isEmpty,
    '${stillOwed.settlements}',
  );
  check(
    'and is told what is in flight',
    stillOwed.awaiting.single.paid == 4500,
    '${stillOwed.awaiting}',
  );

  // A payee confirms a payment they can see, by the id the bill carries. One
  // transaction paying several people writes one record each, so the id is not
  // the transaction's — the transaction is in `reference`.
  final toConfirm = afterPayment.bill.payments.single.id;
  ana.add(
    confirmPaymentEntry(
      ana.facts(),
      toConfirm,
      'recipientConfirmed',
      null,
      ana.signingSeed(),
    ),
  );
  ben.entries
    ..clear()
    ..addAll(mergeEntries(ben.entries, ana.entries).entries);
  final settled = obligationOf(ben.facts(), billId, ben.entries, const [])!;
  check(
    'once confirmed, the debt is gone',
    settled.settlements.isEmpty && settled.awaiting.isEmpty,
    'settlements=${settled.settlements.length} awaiting=${settled.awaiting.length}',
  );

  print('the log reads as a history');
  final history = historyOf(ana.facts(), billId, ana.entries);
  final kinds = history.map((e) => e.kind).toSet();
  check(
    'every kind a person needs is there',
    kinds.containsAll([
      BillEventKind.opened,
      BillEventKind.joined,
      BillEventKind.expenseAdded,
      BillEventKind.priced,
      BillEventKind.paymentRecorded,
      BillEventKind.paymentConfirmed,
    ]),
    '$kinds',
  );
  check(
    'newest first',
    history.first.at.compareTo(history.last.at) >= 0,
    '${history.first.at} .. ${history.last.at}',
  );

  print(
    failures == 0
        ? 'CONSUMER RESULT: dart drives a whole bill with no callbacks, $failures failures'
        : 'CONSUMER RESULT: $failures check(s) failed',
  );
}
