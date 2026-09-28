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
HostFacts facts(String me, String at, int nonce) => HostFacts(
  me: me,
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
  // A wallet that publishes a key speaks as the participant id that key
  // derives (§10.7), or the key binds nothing.
  final anaKey = identityKeyFromSeed(anaSeed);
  final benKey = identityKeyFromSeed(benSeed);
  final ana = participantIdForKey(anaKey);
  final ben = participantIdForKey(benKey);

  // Ana's device writes four entries. Each comes back as the JSON §9.3
  // canonicalises, with §9.5's id already derived and signed; the wallet
  // stores the string and never inspects it.
  final create = createBillEntry(
    facts(ana, '2026-10-28T19:31:00.000Z', 1),
    'Dinner',
    'EUR',
    'equal',
    anaKey,
    anaSeed,
  );

  // The bill these entries belong to, read back from the entry that opened
  // it. Every other entry is signed on it (§10.6), and every fold names it,
  // so a create for another bill pushed into the channel cannot make this
  // one unopenable.
  final billId = (jsonDecode(create) as Map)['id'] as String;

  final anaLog = [
    create,
    joinBillEntry(
      facts(ana, '2026-10-28T19:32:00.000Z', 2),
      billId,
      'Ana',
      'u1ana',
      anaKey,
      const [],
      anaSeed,
    ),
    addExpenseEntry(
      facts(ana, '2026-10-28T19:33:00.000Z', 3),
      billId,
      'x1',
      ana,
      9000,
      jsonEncode({
        'type': 'equal',
        'among': [ana, ben],
      }),
      'dinner',
      anaSeed,
    ),
    // §7 snapshots one rate onto the bill, so six devices do not price one
    // dinner six ways. 300000 minor units per ZEC is €3000.00.
    setRateEntry(
      facts(ana, '2026-10-28T19:34:00.000Z', 4),
      billId,
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
      facts(ben, '2026-10-28T19:35:00.000Z', 5),
      billId,
      'Ben',
      'u1ben',
      benKey,
      const [],
      benSeed,
    ),
  ];

  // Merging is how two devices come to agree (§10.2). It is a set union by
  // id, in either direction, any number of times.
  final log = mergeEntries(anaLog, benLog).entries;

  final benFacts = facts(ben, '2026-10-28T19:36:00.000Z', 6);
  final folded = foldEntries(benFacts, billId, log);
  print(
    'on the bill: ${folded.bill.participants.map((p) => p.name).join(', ')}',
  );
  // Render these. An entry the fold set aside is one a person cannot see
  // otherwise, and its §12 code is what a wallet turns into a sentence.
  print('set aside: ${folded.setAside}');

  // Null when the bill carries no rate: an unpriced bill is an ordinary bill,
  // not a refusal.
  final owed = obligationOf(benFacts, billId, log);
  if (owed == null) throw StateError('a bill with a rate owes something');

  final settlement = owed.settlements.single;
  print('ben pays ${settlement.amount} to ana');
  // The wallet broadcasts this; sending is not the library's (§13.3).
  print('request: ${owed.request.uri}');
  // Never dropped. A request that silently covers three debts of four is
  // indistinguishable, to the payer, from one that covers all of them.
  print('withheld: ${owed.request.withheldMinorUnits}');

  if (settlement.to != ana || settlement.amount != 4500) {
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
