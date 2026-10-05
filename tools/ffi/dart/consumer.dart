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
  final dinner = addExpenseEntry(
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
  );
  ana.add(dinner);
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

  print('ana plans taking ben off the bill (§10.8)');
  SplitzErrorExceptionHost? refusedBy(void Function() call) {
    try {
      call();
      return null;
    } on SplitzErrorExceptionHost catch (e) {
      return e;
    }
  }

  final dinnerId = (jsonDecode(dinner) as Map)['id'] as String;
  final unpaid = planRemoval(ana.facts(), billId, ana.entries, ben.me, ana.me);
  check(
    'her expense is offered, split without him, and nothing blocks it',
    unpaid.blockers.isEmpty &&
        unpaid.edits.length == 1 &&
        unpaid.edits.single.entryId == dinnerId &&
        jsonEncode(jsonDecode(unpaid.edits.single.splitJson)) ==
            jsonEncode({
              'type': 'equal',
              'among': [ana.me],
            }),
    '${unpaid.edits.map((e) => e.splitJson).toList()}',
  );
  // 90.00 between two is 45.00 each; Ana alone takes it all.
  final moved = removalShareChanges(unpaid);
  check(
    'the plan takes him off whole, and his 45.00 moves to her',
    unpaid.complete &&
        moved.length == 2 &&
        moved.every(
          (c) =>
              c.minorUnits == (c.participantId == ana.me ? 4500 : -4500) &&
              (c.participantId == ana.me || c.participantId == ben.me),
        ),
    '${unpaid.complete} ${moved.map((c) => '${c.participantId}:${c.minorUnits}').toList()}',
  );
  String? unsplit;
  try {
    removalShareChanges(
      RemovalPlan(
        edits: [
          RemovalEdit(
            entryId: unpaid.edits.single.entryId,
            seen: unpaid.edits.single.seen,
            author: unpaid.edits.single.author,
            splitJson: '{"type":"equal","among":[]}',
          ),
        ],
        blockers: unpaid.blockers,
        joins: unpaid.joins,
        complete: unpaid.complete,
      ),
    );
  } on SplitzErrorExceptionProtocol catch (e) {
    unsplit = e.code;
  }
  check(
    'and a plan whose split divides nothing is refused',
    unsplit == 'empty_split',
    '$unsplit',
  );
  check(
    'and the plan still stands while the bill has not moved',
    sameRemovalPlan(
          unpaid,
          planRemoval(ana.facts(), billId, ana.entries, ben.me, ana.me),
        ) ==
        RemovalPlanStanding.stands,
    'stands',
  );
  final without = splitWithout(
    jsonEncode({
      'type': 'equal',
      'among': [ana.me, ben.me],
    }),
    ben.me,
  );
  check(
    'a split without him crosses as JSON',
    without != null &&
        jsonEncode(jsonDecode(without)) ==
            jsonEncode({
              'type': 'equal',
              'among': [ana.me],
            }),
    '$without',
  );
  check(
    'and one only a person can redivide is answered with none',
    splitWithout(
          jsonEncode({
            'type': 'exact',
            'amounts': {ana.me: 1, ben.me: 1},
          }),
          ben.me,
        ) ==
        null,
    'none',
  );
  final notSplit = refusedBy(() => splitWithout('{', ben.me));
  check(
    'text that is not a split is refused',
    notSplit != null,
    '${notSplit?.detail}',
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

  print(
    'a send the wallet wrote down is not cleared on a person\'s word (§14.3)',
  );
  final txid = 'ab' * 32;
  final note = pendingSendNote(billId, owed, ben.now());
  final named = pendingSendAfter(
    billId,
    note,
    SendEnded.unresolved,
    txid,
    false,
  )!;
  check(
    'a note naming its transaction is not cleared while the wallet may still send it',
    pendingSendNamedRefusal(billId, named, TransactionState.waiting) ==
        NamedSendRefusal.waiting,
    '${pendingSendNamedRefusal(billId, named, TransactionState.waiting)}',
  );
  check(
    'nor once it went through, and may be once it expired',
    pendingSendNamedRefusal(billId, named, TransactionState.mined) ==
            NamedSendRefusal.mined &&
        pendingSendNamedRefusal(billId, named, TransactionState.expired) ==
            null,
    '${pendingSendNamedRefusal(billId, named, TransactionState.expired)}',
  );
  check(
    'nobody may say it never left while the wallet is still sending',
    pendingSendUnsentRefusal(billId, note, true, const []) ==
        const UnsentClaimRefusalStillSending(),
    '${pendingSendUnsentRefusal(billId, note, true, const [])}',
  );
  final builtSince = pendingSendUnsentRefusal(billId, note, false, [
    OwnTransaction(txid: txid, created: ben.now()),
  ]);
  check(
    'nor once the wallet built a transaction after the note was written',
    builtSince == UnsentClaimRefusalBuiltSince(txid: txid),
    '$builtSince',
  );
  check(
    'one built before it does not hold the note',
    pendingSendUnsentRefusal(billId, note, false, [
          OwnTransaction(txid: 'cd' * 32, created: '2026-10-28T19:30:00.000Z'),
        ]) ==
        null,
    'none',
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
  // The record names no transaction; the rule is asked as if it named `txid`.
  check(
    'ben may not withdraw his record while its transaction is mined',
    ownPaymentWithdrawalRefusal(
          paid.from,
          paid.method,
          txid,
          ben.me,
          TransactionState.mined,
        ) ==
        OwnPaymentWithdrawal.mined,
    '${ownPaymentWithdrawalRefusal(paid.from, paid.method, txid, ben.me, TransactionState.mined)}',
  );
  check(
    'and may once it expired unmined',
    ownPaymentWithdrawalRefusal(
          paid.from,
          paid.method,
          txid,
          ben.me,
          TransactionState.expired,
        ) ==
        null,
    'none',
  );
  check(
    'a record naming no transaction is not this rule\'s',
    ownPaymentWithdrawalRefusal(
          paid.from,
          paid.method,
          paid.reference,
          ben.me,
          TransactionState.mined,
        ) ==
        null,
    'none',
  );
  final paidPlan = planRemoval(
    ana.facts(),
    billId,
    ana.entries,
    ben.me,
    ana.me,
  );
  check(
    'once he has paid, taking ben off is blocked by the payment',
    paidPlan.blockers.length == 1 &&
        paidPlan.blockers.single.block == RemovalBlock.payment &&
        paidPlan.blockers.single.fromThem,
    '${paidPlan.blockers.map((b) => b.block).toList()}',
  );
  check(
    'and is no longer whole: he cannot come off',
    !paidPlan.complete,
    '${paidPlan.complete}',
  );
  check(
    'so the plan ana saw before no longer stands',
    sameRemovalPlan(unpaid, paidPlan) == RemovalPlanStanding.changed,
    'changed',
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
  check(
    'two sources within the tolerance agree on the higher',
    agreedPrice(138819, 138905, 200) == 138905,
    '${agreedPrice(138819, 138905, 200)}',
  );
  check(
    'and two that are not give no price',
    agreedPrice(138819, 152701, 200) == null,
    '${agreedPrice(138819, 152701, 200)}',
  );

  print(
    'a payout a person declares goes first, and replaces its own kind (§9.1)',
  );
  String listed(List<Payout> payouts) {
    String one(Payout p) => '${p.kind}:${p.address}:${p.asset}:${p.chain}';
    return payouts.map(one).join(' ');
  }

  final ranked = rankedPayouts(
    const Participant(
      id: 'p',
      name: 'P',
      payTo: 'zOld',
      identityKey: null,
      payouts: [],
    ),
    const Payout(kind: 'swap', address: '0xa', asset: 'USDC', chain: null),
  );
  check(
    'a pay-to-only record keeps its ZEC behind the new swap',
    listed(ranked) == 'swap:0xa:USDC:null zec:zOld:null:null',
    listed(ranked),
  );
  final replaced = rankedPayouts(
    const Participant(
      id: 'p',
      name: 'P',
      payTo: null,
      identityKey: null,
      payouts: [
        Payout(kind: 'cash', address: null, asset: null, chain: null),
        Payout(kind: 'zec', address: 'zA', asset: null, chain: null),
        Payout(kind: 'swap', address: '0xb', asset: 'USDT', chain: null),
      ],
    ),
    const Payout(kind: 'zec', address: 'zB', asset: null, chain: null),
  );
  check(
    'a new ZEC payout replaces the old one, the rest keep their order',
    listed(replaced) ==
        'zec:zB:null:null cash:null:null:null swap:0xb:USDT:null',
    listed(replaced),
  );

  print('a swap deposit is checked against the bill before it is sent (§15.7)');
  const onBase = Payout(
    kind: 'swap',
    address: '0xbenbase',
    asset: 'USDC',
    chain: 'base',
  );
  const onArb = Payout(
    kind: 'swap',
    address: '0xbenarb',
    asset: 'USDC',
    chain: 'arb',
  );
  final taxiCreate = createBillEntry(
    ana.facts(),
    'Taxi',
    'EUR',
    'equal',
    anaKey,
    null,
    ana.signingSeed(),
  );
  final taxiId = (jsonDecode(taxiCreate) as Map)['id'] as String;
  final taxi = [
    taxiCreate,
    joinBillEntry(
      ana.facts(),
      taxiId,
      'Ana',
      'u1ana',
      anaKey,
      const [],
      ana.signingSeed(),
    ),
    joinBillEntry(ben.facts(), taxiId, 'Ben', null, benKey, const [
      onBase,
      onArb,
    ], ben.signingSeed()),
    addExpenseEntry(
      ben.facts(),
      taxiId,
      't1',
      ben.me,
      8000,
      jsonEncode({
        'type': 'equal',
        'among': [ana.me, ben.me],
      }),
      null,
      ben.signingSeed(),
    ),
    setRateEntry(ana.facts(), taxiId, 'EUR', 51234, null, ana.signingSeed()),
  ];
  SwapQuote quote(
    String recipient,
    String chain, {
    String? memo,
    String? floor,
  }) => SwapQuote(
    depositAddress: 't1deposit',
    recipient: recipient,
    depositMemo: memo,
    amountInZatoshi: 7807316,
    amountOut: '39990000',
    minAmountOut: floor,
    asset: TradableAsset(
      assetId: 'nep141:$chain-usdc',
      symbol: 'USDC',
      chain: chain,
      decimals: 6,
    ),
    deadline: '2026-10-29T23:00:00.000Z',
    reference: 'intent-1',
  );
  check(
    'a deposit to ben\'s first payout, for what ana owes, may go',
    swapSendRefusal(
          ana.facts(),
          taxiId,
          taxi,
          quote('0xbenbase', 'base'),
          ben.me,
          4000,
          null,
        ) ==
        null,
    'none',
  );
  check(
    'one asked for his second payout may go too',
    swapSendRefusal(
          ana.facts(),
          taxiId,
          taxi,
          quote('0xbenarb', 'arb'),
          ben.me,
          4000,
          onArb,
        ) ==
        null,
    'none',
  );
  final wrongRecipient = swapSendRefusal(
    ana.facts(),
    taxiId,
    taxi,
    quote('0xbenbase', 'base'),
    ben.me,
    4000,
    onArb,
  );
  check(
    'one whose recipient is not the payout chosen is refused',
    wrongRecipient == const SwapSendRefusalRecipientChanged(),
    '$wrongRecipient',
  );
  check(
    'the payout chosen is found by type, address, asset and chain',
    declaredPayoutIndex(const [onBase, onArb], onArb) == 1 &&
        declaredPayoutIndex(
              const [onBase, onArb],
              const Payout(
                kind: 'swap',
                address: '0xbenarb',
                asset: 'USDC',
                chain: null,
              ),
            ) ==
            null,
    '${declaredPayoutIndex(const [onBase, onArb], onArb)}',
  );
  final swapRecord = recordPaymentEntry(
    ana.facts(),
    taxiId,
    PaymentDraft(
      paymentId: 'intent-1',
      to: ben.me,
      amount: 4000,
      method: 'swap',
      reference: 'intent-1',
      zatoshi: 7807316,
      paidAtRate: null,
      note: null,
    ),
    ana.signingSeed(),
  );
  taxi.add(swapRecord);
  final held = swapSendRefusal(
    ana.facts(),
    taxiId,
    taxi,
    quote('0xbenbase', 'base'),
    ben.me,
    4000,
    null,
  );
  check(
    'once a payment covers the debt, a second deposit is held for ben to confirm',
    held is SwapSendRefusalHeld && held.paidTo.join() == ben.me,
    '$held',
  );
  final failed = failedSwapWithdrawals(ana.facts(), taxiId, taxi, 'intent-1');
  check(
    'a swap that failed names ana\'s record of it to withdraw',
    failed.length == 1 &&
        failed.single == (jsonDecode(swapRecord) as Map)['id'] as String,
    '$failed',
  );
  check(
    'and nothing to ben, who did not write it',
    failedSwapWithdrawals(ben.facts(), taxiId, taxi, 'intent-1').isEmpty,
    'none',
  );

  print('a swap\'s deposit and its record come from the binding (§15.7)');
  const eur = ExchangeRate(
    currency: 'EUR',
    minorUnitsPerZec: 51234,
    at: '2026-10-28T19:30:00.000Z',
    source: null,
  );
  check(
    'a debt is sized in zatoshi at the bill\'s rate, rounding up',
    fiatToZatoshi(4000, eur) == 7807316,
    '${fiatToZatoshi(4000, eur)}',
  );
  final deposit = swapDeposit(
    taxiId,
    quote('0xbenbase', 'base'),
    ben.me,
    4000,
    eur,
    ana.now(),
  );
  check(
    'a deposit is one request to the quote\'s address for its zatoshi',
    deposit.uri.startsWith('zcash:t1deposit?amount=0.07807316'),
    deposit.uri,
  );
  check(
    'and its note carries the swap',
    deposit.note.contains('"reference":"intent-1"'),
    deposit.note,
  );
  final needsMemo = refusedBy(
    () => swapDeposit(
      taxiId,
      quote('0xbenbase', 'base', memo: '123'),
      ben.me,
      4000,
      eur,
      ana.now(),
    ),
  );
  check(
    'one whose deposit needs a memo is refused',
    needsMemo != null,
    '${needsMemo?.detail}',
  );
  final swapEntry = swapPaymentEntry(
    ana.facts(),
    taxiId,
    quote('0xbenbase', 'base', floor: '39500000'),
    ben.me,
    4000,
    eur,
    ana.signingSeed(),
  );
  check(
    'its record names the asset, the chain and the floor',
    swapEntry.contains('"note":"at least 39.5 USDC on base"'),
    swapEntry,
  );
  check(
    'base units read as whole tokens',
    formatBaseUnits('39990000', 6) == '39.99' &&
        formatBaseUnits('x', 6) == null,
    '${formatBaseUnits('39990000', 6)}',
  );

  print('what every wallet derives, reads and asks before writing');
  const mnemonic =
      'abandon abandon abandon abandon abandon abandon abandon abandon '
      'abandon abandon abandon about';
  final derived = identitySeedFromMnemonic(mnemonic, '', 1);
  check(
    'a mnemonic derives the seed every wallet derives (§15.1)',
    derived == 'jWQijb3QECEvHgOUb4_W7UPiUujN7D-7Nq0jYg5mrKo',
    derived,
  );
  final noMnemonic = refusedBy(() => identitySeedFromMnemonic('', '', 0));
  check(
    'an empty mnemonic is refused',
    noMnemonic != null,
    '${noMnemonic?.detail}',
  );
  final digest = '${'00' * 31}ab';
  check(
    'a txid in digest order is reversed (§14.7)',
    txidInSendOrder(digest) == 'ab${'00' * 31}' &&
        txidInSendOrder('abc') == null,
    '${txidInSendOrder(digest)}',
  );
  check(
    'a typed figure is read in integers (§2.1)',
    parseAmountIn('12.34', 'EUR') == 1234 &&
        parseAmountIn('1,000', 'KWD') == null &&
        parseMinorUnits('12.5', 2) == 1250,
    '${parseAmountIn('12.34', 'EUR')}',
  );
  final gold = refusedBy(
    () => createBillEntry(
      ana.facts(),
      'Gold',
      'XAU',
      'equal',
      anaKey,
      null,
      ana.signingSeed(),
    ),
  );
  check(
    'no bill is opened in a currency with no minor unit',
    gold != null,
    '${gold?.detail}',
  );
  final taxiFolded = foldEntries(ana.facts(), taxiId, taxi);
  check(
    'the creator is the one the fold names',
    taxiFolded.creatorId == ana.me,
    taxiFolded.creatorId,
  );
  final benOff = planRemoval(ana.facts(), taxiId, taxi, ben.me, ana.me);
  check(
    'taking ben off lists his join to withdraw',
    benOff.joins.length == 1,
    '${benOff.joins}',
  );
  final off = voidEntryFor(
    ana.facts(),
    taxiId,
    benOff.joins.single,
    ana.signingSeed(),
  );
  check(
    'which is refused before it is written while the bill names him',
    entryRefusal(ana.facts(), taxiId, taxi, off) == 'participant_still_named',
    '${entryRefusal(ana.facts(), taxiId, taxi, off)}',
  );
  final own = voidEntryFor(
    ana.facts(),
    taxiId,
    (jsonDecode(swapRecord) as Map)['id'] as String,
    ana.signingSeed(),
  );
  check(
    'and ana withdrawing her own record is not',
    entryRefusal(ana.facts(), taxiId, taxi, own) == null,
    'none',
  );

  print('and the rest of what every wallet needs from the protocol');
  final fallback = payoutFallback(['not on base', null]);
  check(
    'a first payout this wallet cannot pay is passed over for the next it can (§14.8)',
    fallback?.index == 1 &&
        fallback?.passedOver == 'not on base' &&
        payoutFallback([null, 'x']) == null,
    '${fallback?.index}',
  );
  const wrapped = TradableAsset(
    assetId: 'nep141:near-zec',
    symbol: 'ZEC',
    chain: 'near',
    decimals: 8,
  );
  check(
    'native ZEC is the one on its own chain',
    zecAssetIn([
              wrapped,
              const TradableAsset(
                assetId: 'nep141:zec.omft.near',
                symbol: 'ZEC',
                chain: 'zec',
                decimals: 8,
              ),
            ]) ==
            'nep141:zec.omft.near' &&
        zecAssetIn([wrapped]) == null,
    'nep141:zec.omft.near',
  );
  check(
    'a rate 5% from the live price is told by how much',
    ratePercentOff(105, 100) == 5 && ratePercentOff(1, 0) == null,
    '${ratePercentOff(105, 100)}',
  );
  check(
    'names a reader cannot tell apart fold alike',
    nameSkeleton('\u0410na') == nameSkeleton('ana'),
    nameSkeleton('\u0410na'),
  );
  final names = displayNames(ana.facts(), taxiId, taxi);
  check(
    'every participant has a display name',
    names.keys.toSet().containsAll([ana.me, ben.me]) && names.length == 2,
    '$names',
  );
  final corrected = amendExpenseEntry(
    ben.facts(),
    taxiId,
    taxi,
    '${ben.me}:t1',
    null,
    9000,
    null,
    'taxi home',
    ben.signingSeed(),
  );
  check(
    'an expense is corrected from what the bill applies now',
    entryRefusal(ben.facts(), taxiId, taxi, corrected) == null,
    'none',
  );
  String? unknownExpense;
  try {
    amendExpenseEntry(
      ben.facts(),
      taxiId,
      taxi,
      '${ben.me}:t9',
      null,
      1,
      null,
      null,
      ben.signingSeed(),
    );
  } on SplitzErrorExceptionProtocol catch (e) {
    unknownExpense = e.code;
  }
  check(
    'and one the bill does not apply is refused',
    unknownExpense == 'unknown_entry',
    '$unknownExpense',
  );
  final concerns = concernsBeforeConfirming(
    ben.facts(),
    taxiId,
    taxi,
    '${ana.me}:intent-1',
    null,
  );
  check(
    'ana paying by a rate she set is a concern before ben confirms it',
    concerns.length == 1 && concerns.single == PaymentConcern.rateSetByPayer,
    '$concerns',
  );
  final toAna = recordPaymentEntry(
    ben.facts(),
    taxiId,
    PaymentDraft(
      paymentId: 'p-memo',
      to: ana.me,
      amount: 1000,
      method: 'shieldedZec',
      reference: 'ab' * 32,
      zatoshi: 2000000,
      paidAtRate: null,
      note: null,
    ),
    ben.signingSeed(),
  );
  final memos = memoTxids(ana.facts(), [
    HeldBill(billId: taxiId, entries: [...taxi, toAna]),
  ]);
  check(
    'memos are read for the transactions a record to this device names',
    memos.length == 1 &&
        memos.single == 'ab' * 32 &&
        memoTxids(ana.facts(), [
          HeldBill(billId: taxiId, entries: taxi),
        ]).isEmpty,
    '$memos',
  );

  print(
    failures == 0
        ? 'CONSUMER RESULT: dart drives a whole bill with no callbacks, $failures failures'
        : 'CONSUMER RESULT: $failures check(s) failed',
  );
  if (failures != 0) exit(1);
}
