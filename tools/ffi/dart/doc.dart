/// The smallest Dart wallet that renders a payment request.
///
/// INTEGRATING.md quotes everything below the marker verbatim;
/// `tools/docs/blocks.py` fails when the two drift, and `tools/ffi/dart.sh`
/// runs it, so the document's sample is a sample that executed.
// docs:begin
import 'dart:convert';
import 'dart:typed_data';

import 'package:splitz_dart_consumer/splitz_ffi.dart';

/// The facts §15.1 says a wallet owns, for one call. A §9.3 instant and
/// sixteen unpredictable bytes are the wallet's to supply: this library reads
/// no clock (§13) and owns no entropy.
HostFacts facts(String me, String payTo, String at, int nonce) => HostFacts(
  me: me,
  payTo: payTo,
  now: at,
  nonce: Uint8List.fromList(List.generate(16, (i) => (nonce + i) & 0xff)),
);

/// The Ed25519 seed a wallet keeps in the platform keychain, as §9.4 writes a
/// key: 32 bytes, unpadded base64url.
String seed(int first) => base64Url
    .encode(List.generate(32, (i) => (first + i) & 0xff))
    .replaceAll('=', '');

void main(List<String> args) {
  configureDefaultBindings(libraryPath: args[0]);

  final anaSeed = seed(1);
  final benSeed = seed(90);

  // Ana's device writes four entries. Each comes back as the JSON §9.3
  // canonicalises, with §9.5's id already derived; the wallet stores the
  // string and never inspects it.
  final anaLog = [
    createBillEntry(
      facts('ana', 'u1ana', '2026-10-28T19:31:00.000Z', 1),
      'Dinner',
      'EUR',
      'equal',
      identityKeyFromSeed(anaSeed),
      anaSeed,
    ),
    joinBillEntry(
      facts('ana', 'u1ana', '2026-10-28T19:32:00.000Z', 2),
      'Ana',
      'u1ana',
      identityKeyFromSeed(anaSeed),
      anaSeed,
    ),
    addExpenseEntry(
      facts('ana', 'u1ana', '2026-10-28T19:33:00.000Z', 3),
      'x1',
      'ana',
      9000,
      '{"type":"equal","among":["ana","ben"]}',
      'dinner',
      anaSeed,
    ),
    // §7 snapshots one rate onto the bill, so six devices do not price one
    // dinner six ways. 300000 minor units per ZEC is €3000.00.
    setRateEntry(
      facts('ana', 'u1ana', '2026-10-28T19:34:00.000Z', 4),
      'EUR',
      300000,
      'a fixed feed',
      anaSeed,
    ),
  ];

  // Ben's own device writes Ben's join: §10.4 decides what an entry's author
  // may say, and a participant joins for themselves.
  final benLog = [
    joinBillEntry(
      facts('ben', 'u1ben', '2026-10-28T19:35:00.000Z', 5),
      'Ben',
      'u1ben',
      identityKeyFromSeed(benSeed),
      benSeed,
    ),
  ];

  // Merging is how two devices come to agree (§10.2). It is a set union by
  // id, in either direction, any number of times.
  final log = mergeEntries(anaLog, benLog).entries;

  final benFacts = facts('ben', 'u1ben', '2026-10-28T19:36:00.000Z', 6);
  final folded = foldEntries(benFacts, log);
  print('on the bill: ${folded.bill.participants.map((p) => p.id).join(', ')}');
  // Render these. An entry the fold set aside is one a person cannot see
  // otherwise, and its §12 code is what a wallet turns into a sentence.
  print('set aside: ${folded.setAside}');

  // Null when the bill carries no rate: an unpriced bill is an ordinary bill,
  // not a refusal. The third argument names the contested participants the
  // payer has been shown and chosen to pay anyway (§10.7).
  final owed = obligationOf(benFacts, log, const []);
  if (owed == null) throw StateError('a bill with a rate owes something');

  final settlement = owed.settlements.single;
  print('ben pays ${settlement.amount} to ${settlement.to}');
  // The wallet broadcasts this; sending is not the library's (§13.3).
  print('request: ${owed.request.uri}');
  // Never dropped. A request that silently covers three debts of four is
  // indistinguishable, to the payer, from one that covers all of them.
  print('withheld: ${owed.request.withheldMinorUnits}');

  if (settlement.to != 'ana' || settlement.amount != 4500) {
    throw StateError(
      'half of 9000 is 4500 to ana, saw '
      '${settlement.amount} to ${settlement.to}',
    );
  }
  if (!owed.request.uri!.startsWith('zcash:u1ana'))
    throw StateError(owed.request.uri!);
  if (owed.request.withheldMinorUnits != 0) {
    throw StateError('${owed.request.withheldMinorUnits}');
  }
  print('DOC RESULT: dart');
}
