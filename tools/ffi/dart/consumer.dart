/// A Dart wallet over the callback-free binding.
///
/// It holds its own entries, keeps its own clock and its own randomness, and
/// hands the library facts rather than implementing seven interfaces. Nothing
/// calls back into Dart.
import 'dart:convert';
import 'dart:io';
import 'dart:math';
import 'dart:typed_data';

import 'package:splitz_dart_consumer/splitz_ffi.dart';

int failures = 0;
void check(String name, bool ok, String saw) {
  print('  ${ok ? "PASS" : "FAIL"}  $name — $saw');
  if (!ok) failures += 1;
}

/// One device: its log, its clock, its randomness, and the key it signs with.
class Device {
  Device(this.seedByte);
  final int seedByte;

  /// The key this account publishes, and the participant id it derives
  /// (§10.7): a wallet that publishes a key writes every entry under the id
  /// that key derives, or the key binds nothing.
  late final String key = identityKeyFromSeed(signingSeed());
  late final String me = participantIdForKey(key);
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

  HostFacts facts() => HostFacts(me: me, now: now(), nonce: nonce());

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

  final ana = Device(1);
  final ben = Device(90);

  // Top-level bytes cross in a record: one generator writes a bare byte
  // string without its length, and the same mnemonic would then derive
  // another identity here than in Kotlin or Swift. Pinned in Rust.
  check(
    'a seed derived from a secret is the one the protocol pins',
    identitySeedFromSecret(SecretBytes(bytes: Uint8List.fromList([1, 2, 3]))) ==
        'MNp3HJmtVUpkGFp2KXoi4ysYoDqKi9Sf4upQw5qvOps',
    'MNp3…',
  );
  check(
    'and a long secret whose first byte is high crosses whole',
    identitySeedFromSecret(
          SecretBytes(bytes: Uint8List.fromList(List.filled(64, 0xAB))),
        ) ==
        'zHlJI6Xb7tQXjLuwAoEwVjjVEB8jPs5DA_NhM50W1YM',
    'zHlJ…',
  );

  print('ana opens a bill and joins it');
  final anaKey = ana.key;
  final random = Random.secure();
  final billKey = newBillKey(
    RandomBytes(
      bytes: Uint8List.fromList(List.generate(32, (_) => random.nextInt(256))),
    ),
  );
  check(
    'that key is one the cipher can use',
    billKeyProblem(billKey) == null,
    billKey,
  );
  // The key is minted first: the create entry commits to it (§9.4).
  final create = createBillEntry(
    ana.facts(),
    'Dinner',
    'EUR',
    'equal',
    anaKey,
    billKey,
    ana.signingSeed(),
  );
  ana.add(create);
  final billId = (jsonDecode(create) as Map)['id'] as String;
  check('the bill has a §9.4 id', billId.isNotEmpty, billId);

  ana.add(
    joinBillEntry(
      ana.facts(),
      billId,
      'Ana',
      'u1ana',
      anaKey,
      const [],
      ana.signingSeed(),
    ),
  );

  print('ben joins, and the two logs merge');
  final benKey = ben.key;
  ben.entries.addAll(ana.entries);
  ben.add(
    joinBillEntry(
      ben.facts(),
      billId,
      'Ben',
      'u1ben',
      benKey,
      const [],
      ben.signingSeed(),
    ),
  );
  // What ana holds, as she would report it: one key per copy (§14.5).
  final anaHolds = [for (final e in ana.entries) copyKey(e)];
  final behind = deltaForPeer(ben.facts(), billId, ben.entries, anaHolds);
  check(
    'ana lacks only ben\'s join, and it fits one code',
    behind.missing == 1 && behind.uri != null && behind.tooBigCode == null,
    '${behind.missing}',
  );

  print('the bill key is minted from the platform\'s entropy, and invites');
  final invite = inviteForBill(
    ana.facts(),
    billId,
    ana.entries,
    billKey,
    'Dinner',
    1800000000,
  );
  final link = renderInviteLink(invite, 'https://example.org/join');
  final stranger = newBillKey(
    RandomBytes(
      bytes: Uint8List.fromList(List.generate(32, (_) => random.nextInt(256))),
    ),
  );
  check(
    'a bill code carrying some other key is refused',
    readScanned(
          shareableBillPayload(ana.facts(), billId, ana.entries, stranger)!,
        ).refusedCode ==
        'invite_key_mismatch',
    'invite_key_mismatch',
  );
  check(
    'the invite reads back from an https link',
    readScanned(link).billId == billId,
    link,
  );
  check(
    'and expires by the wallet\'s clock',
    !inviteExpiry(invite, 1799999999).expired &&
        inviteExpiry(invite, 1800000001).expired,
    '1800000000',
  );

  print('ana adds an expense they share, and prices it');
  ana.entries
    ..clear()
    ..addAll(mergeEntries(ana.entries, ben.entries).entries);
  ana.add(
    addExpenseEntry(
      ana.facts(),
      billId,
      'x1',
      ana.me,
      9000,
      jsonEncode({
        'type': 'equal',
        'among': [ana.me, ben.me],
      }),
      'dinner',
      ana.signingSeed(),
    ),
  );
  ana.add(
    setRateEntry(
      ana.facts(),
      billId,
      'EUR',
      300000,
      'a fixed feed',
      ana.signingSeed(),
    ),
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
    folded.identities.bound[ana.me] == anaKey &&
        folded.identities.bound[ben.me] == benKey,
    '${folded.identities.bound.keys}',
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
  final owed = obligationOf(ben.facts(), billId, ben.entries);
  check('ben has an obligation', owed != null, owed?.request.uri ?? 'none');
  final settlement = owed!.settlements.single;
  check(
    'it is four and a half thousand to ana',
    settlement.to == ana.me && settlement.amount == 4500,
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

  print('ben\'s review screen shows what §14.2 says it must');
  // The screen is the wallet's; these are the strings it draws, with the
  // amount and the rate written as the binding writes them.
  final zec = renderAmount(owed.request.payments.single.zatoshi);
  final screen = [
    'Pay Ana $zec ZEC',
    'to u1ana',
    'at ${rateFigure(owed.rate)} EUR per ZEC, set by Ana',
  ];
  final shown = checkPayerReview(
    ben.facts(),
    billId,
    ben.entries,
    owed,
    screen,
    const {},
    const {},
    '',
  );
  check('a screen showing every fact passes', shown.isEmpty, '$screen');
  final noAddress = checkPayerReview(
    ben.facts(),
    billId,
    ben.entries,
    owed,
    [for (final l in screen) l == 'to u1ana' ? 'to your contact' : l],
    const {},
    const {},
    '',
  );
  check(
    'one without the output\'s address is told exactly that',
    noAddress.length == 1 &&
        noAddress.single.rule == ReviewRule.output &&
        noAddress.single.fact == 'the address Ana is paid at' &&
        noAddress.single.expected == 'u1ana',
    '$noAddress',
  );

  print('the wallet sends it, then records what §14.3 allows');
  ben.add(
    recordPaymentEntry(
      ben.facts(),
      billId,
      PaymentDraft(
        paymentId: 'tx-ben-1',
        to: ana.me,
        amount: 4500,
        method: 'shieldedZec',
        reference: null,
        zatoshi: null,
        paidAtRate: null,
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
  final paid = afterPayment.bill.payments.single;
  final mine = awaitingMyConfirmation(ana.facts(), billId, ana.entries);
  check(
    'ana is shown it as hers to confirm',
    mine.map((p) => p.id).toList().toString() == [paid.id].toString(),
    '${mine.map((p) => p.id).toList()}',
  );
  check(
    'and ben, who paid it, is shown nothing to confirm',
    awaitingMyConfirmation(ben.facts(), billId, ben.entries).isEmpty,
    'none',
  );
  // This record was written by hand, with no ZEC figure, rate or reference:
  // the screen says so in the wallet's own words.
  final zatoshi = paid.zatoshi;
  final rate = paid.paidAtRate;
  final confirmScreen = [
    'Ben says he paid you',
    zatoshi == null ? 'ZEC sent: not recorded' : '${renderAmount(zatoshi)} ZEC',
    rate == null
        ? 'rate: not recorded'
        : 'priced at ${rateFigure(rate)} EUR a ZEC',
    paid.reference == null
        ? 'reference: not recorded'
        : 'transaction ${paid.reference}',
  ];
  check(
    'ana\'s confirm screen shows what §14.2 says a payee must see',
    checkPayeeReview(paid, confirmScreen, 'not recorded').isEmpty,
    '$confirmScreen',
  );
  final unsaid = checkPayeeReview(paid, const [
    'Ben says he paid you',
  ], 'not recorded');
  check(
    'one that never says a figure is missing is told each one',
    unsaid.map((f) => f.rule).toList().toString() ==
        [
          ReviewRule.payeeZec,
          ReviewRule.payeeRate,
          ReviewRule.payeeReference,
        ].toString(),
    '$unsaid',
  );

  final stillOwed = obligationOf(ben.facts(), billId, ben.entries)!;
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
      billId,
      toConfirm,
      'recipientConfirmed',
      null,
      afterPayment.paymentDigests[toConfirm]!,
      ana.signingSeed(),
    ),
  );
  ben.entries
    ..clear()
    ..addAll(mergeEntries(ben.entries, ana.entries).entries);
  final settled = obligationOf(ben.facts(), billId, ben.entries)!;
  check(
    'once confirmed, the debt is gone',
    settled.settlements.isEmpty && settled.awaiting.isEmpty,
    'settlements=${settled.settlements.length} awaiting=${settled.awaiting.length}',
  );

  print('before signing, the wallet holds what it read against the request');
  final ask = owed.request.payments.single;
  final anaAddress = folded.bill.participants
      .firstWhere((p) => p.id == ana.me)
      .payTo!;
  check(
    'a reading that matches has nothing to say',
    proposalProblem(owed.request.uri!, [
          ProposedOutput(address: anaAddress, zatoshi: ask.zatoshi),
        ]) ==
        null,
    'none',
  );
  final problem = proposalProblem(owed.request.uri!, const []);
  check(
    'one that dropped the payment is a sentence, so nothing is built',
    problem != null && problem.isNotEmpty,
    '$problem',
  );

  print('a swap provider\'s answer is read by its status first');
  check(
    'a 2xx body is the answer',
    swapAnswer(200, '{"quote":1}') == '{"quote":1}',
    'read',
  );
  SplitzErrorExceptionHost? refusedBy(void Function() call) {
    try {
      call();
      return null;
    } on SplitzErrorExceptionHost catch (e) {
      return e;
    }
  }

  final noRoute = refusedBy(() => swapAnswer(400, '{"message":"no route"}'));
  check(
    'a 4xx is refused with the provider\'s own words, and waiting will not help',
    noRoute != null &&
        noRoute.detail.contains('no route') &&
        !noRoute.transient,
    '${noRoute?.detail}',
  );
  final upstream = refusedBy(() => swapAnswer(503, 'upstream down'));
  check(
    'a 5xx is refused as one to try again',
    upstream?.transient == true,
    '${upstream?.detail}',
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

  print('the fallback price sources, asked and read the same way');
  check(
    'binance is asked for the ZECUSDC ticker',
    binancePriceRequest('https://data-api.binance.vision') ==
        'https://data-api.binance.vision/api/v3/ticker/price?symbol=ZECUSDC',
    binancePriceRequest('https://data-api.binance.vision'),
  );
  check(
    'and prices USD alone',
    zecPriceFromBinance(
              '{"symbol":"ZECUSDC","price":"1390.54000000"}',
              'USD',
            ) ==
            139054 &&
        zecPriceFromBinance('{"symbol":"ZECUSDC","price":"1390.54"}', 'EUR') ==
            null,
    '139054',
  );
  check(
    'coinbase prices the rest from one answer',
    zecPriceFromCoinbase(
              '{"data":{"currency":"ZEC","rates":{"KES":"180159.79"}}}',
              'KES',
            ) ==
            18015979 &&
        coinbasePriceRequest('https://api.coinbase.com') ==
            'https://api.coinbase.com/v2/exchange-rates?currency=ZEC',
    '18015979',
  );

  print(
    failures == 0
        ? 'CONSUMER RESULT: dart drives a whole bill with no callbacks, $failures failures'
        : 'CONSUMER RESULT: $failures check(s) failed',
  );
  if (failures != 0) exit(1);
}
