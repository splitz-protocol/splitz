import 'package:test/test.dart';
import 'package:splitz_core/splitz_core.dart' as splitz;
import 'package:splitz_core/host.dart';

import 'support/fake_host.dart';

/// A bill two people share: ana pays 90.00, split evenly, so ben owes 45.00.
({BillLog log, FakeHost ana, FakeHost ben}) dinner() {
  final ana = FakeHost(me: 'ana', payToAddress: 'u1ana');
  final ben = FakeHost(me: 'ben', payToAddress: 'u1ben');

  final create = createBill(
      host: ana, name: 'Dinner', currency: 'EUR', creatorKey: fakeKey('ana'));
  ana.tick();
  final joinAna = joinBill(host: ana, name: 'Ana', payTo: 'u1ana');
  ben.tick();
  ben.tick();
  final joinBen = joinBill(host: ben, name: 'Ben', payTo: 'u1ben');
  ana.tick();
  final expense = addExpense(
    host: ana,
    expenseId: 'x1',
    paidBy: 'ana',
    amount: 9000,
    split: const {
      'type': 'equal',
      'among': ['ana', 'ben'],
    },
  );
  ana.tick();
  final rate = setRate(host: ana, currency: 'EUR', minorUnitsPerZec: 51234);

  final log = BillLog(ana);
  final refused = log.add([create, joinAna, joinBen, expense, rate]);
  expect(refused, isEmpty, reason: 'every entry this package writes is valid');
  return (log: log, ana: ana, ben: ben);
}

/// Three on a bill, and one of them owes the other two: Ben pays 90.00 and Cat
/// pays 90.00, each split evenly across all three, so Ana owes 30.00 to Ben
/// and 30.00 to Cat — two settlements carried by one transaction.
({BillLog log, FakeHost ana}) twoDebts() {
  final ana = FakeHost(me: 'ana', payToAddress: 'u1ana');
  final ben = FakeHost(me: 'ben', payToAddress: 'u1ben');
  final cat = FakeHost(me: 'cat', payToAddress: 'u1cat');

  final create = createBill(
      host: ana, name: 'Dinner', currency: 'EUR', creatorKey: fakeKey('ana'));
  ana.tick();
  final joinAna = joinBill(host: ana, name: 'Ana', payTo: 'u1ana');
  ben.tick();
  ben.tick();
  final joinBen = joinBill(host: ben, name: 'Ben', payTo: 'u1ben');
  cat.tick();
  cat.tick();
  cat.tick();
  final joinCat = joinBill(host: cat, name: 'Cat', payTo: 'u1cat');
  ben.tick();
  final e1 = addExpense(
    host: ben,
    expenseId: 'x1',
    paidBy: 'ben',
    amount: 9000,
    split: const {
      'type': 'equal',
      'among': ['ana', 'ben', 'cat'],
    },
  );
  cat.tick();
  final e2 = addExpense(
    host: cat,
    expenseId: 'x2',
    paidBy: 'cat',
    amount: 9000,
    split: const {
      'type': 'equal',
      'among': ['ana', 'ben', 'cat'],
    },
  );
  ana.tick();
  final rate = setRate(host: ana, currency: 'EUR', minorUnitsPerZec: 51234);

  final log = BillLog(ana);
  final refused = log.add([create, joinAna, joinBen, joinCat, e1, e2, rate]);
  expect(refused, isEmpty, reason: 'every entry this package writes is valid');
  return (log: log, ana: ana);
}

void main() {
  test('a whole bill, from nothing to a payment request', () {
    final d = dinner();
    final folded = d.log.fold();

    expect(folded.bill.participants.map((p) => p.id), ['ana', 'ben']);

    // A reviewed request stands until the bill changes under it.
    final reviewed = obligationFor(d.ben, folded)!;
    expect(requestStands(d.ben, d.log.fold(), reviewed), isTrue);
    expect(folded.setAside, isEmpty);

    // §4: 90.00 split evenly is 45.00 each; ana paid, so ben owes ana 45.00.
    final owed = splitz.netBalances(folded.bill);
    expect(owed['ana'], 4500);
    expect(owed['ben'], -4500);

    // ana is owed, so ana has nothing to pay.
    final anaOwes = obligationFor(d.ana, folded)!;
    expect(anaOwes.settlements, isEmpty);

    // ben owes, and ana can be paid, so one request carries the whole debt.
    final benOwes = obligationFor(d.ben, folded)!;
    expect(benOwes.settlements.single.to, 'ana');
    expect(benOwes.settlements.single.amount, 4500);
    expect(benOwes.unpayable, isEmpty);
    expect(benOwes.isComplete, isTrue);
    expect(benOwes.uri, startsWith('zcash:u1ana'));
  });

  test('a reviewed request stops standing once the bill changes under it', () {
    final d = dinner();
    final reviewed = obligationFor(d.ben, d.log.fold())!;
    d.ana.tick();
    final more = addExpense(
      host: d.ana,
      expenseId: 'x2',
      paidBy: 'ana',
      amount: 1000,
      split: const {
        'type': 'equal',
        'among': ['ana', 'ben'],
      },
    );
    expect(d.log.add([more]), isEmpty);
    expect(requestStands(d.ben, d.log.fold(), reviewed), isFalse);
  });

  test('a refusal is known before the entry is written', () {
    final d = dinner();
    String idOf(String kind, [String? author]) => d.log.entries.firstWhere(
          (e) => e['kind'] == kind && (author == null || e['author'] == author),
        )['id'] as String;
    final expense = idOf('addExpense');
    final benJoin = idOf('joinBill', 'ben');

    // Ben withdrawing Ana's expense is §10.8's to refuse.
    expect(
      d.log.refusalOf(voidEntry(host: d.ben, targetId: expense)),
      splitz.SplitCode.unauthorizedEntry,
    );
    // Taking Ben off while the dinner names him.
    expect(
      d.log.refusalOf(voidEntry(host: d.ana, targetId: benJoin)),
      splitz.SplitCode.participantStillNamed,
    );
    // An entry this device has not seen yet: written, it applies later.
    final later = d.log.refusalOf(voidEntry(host: d.ana, targetId: 'not-yet'));
    expect(later, splitz.SplitCode.unknownEntry);
    expect(codesAnEntryOutgrows, contains(later));
    // Ana withdrawing her own expense applies.
    expect(d.log.refusalOf(voidEntry(host: d.ana, targetId: expense)), isNull);
    // A second create on a log that names no bill leaves it opening none.
    final unnamed = BillLog(d.ana, entries: d.log.entries);
    expect(
      unnamed.refusalOf(createBill(
        host: d.ben,
        name: 'Other',
        currency: 'EUR',
        creatorKey: fakeKey('ben'),
      )),
      splitz.SplitCode.ambiguousCreate,
    );
    // Nothing was written by asking.
    expect(d.log.entries.where((e) => e['kind'] == 'voidEntry'), isEmpty);
  });

  test('a recipient with no address is reported, never dropped', () {
    final ana = FakeHost(me: 'ana');
    final ben = FakeHost(me: 'ben');
    final create = createBill(
        host: ana, name: 'Dinner', currency: 'EUR', creatorKey: fakeKey('ana'));
    ana.tick();
    // ana joins with no payTo: nobody can send to her.
    final joinAna = joinBill(host: ana, name: 'Ana');
    ben.tick();
    ben.tick();
    final joinBen = joinBill(host: ben, name: 'Ben', payTo: 'u1ben');
    ana.tick();
    final expense = addExpense(
      host: ana,
      expenseId: 'x1',
      paidBy: 'ana',
      amount: 9000,
      split: const {
        'type': 'equal',
        'among': ['ana', 'ben'],
      },
    );
    ana.tick();
    final rate = setRate(host: ana, currency: 'EUR', minorUnitsPerZec: 51234);

    final log = BillLog(ana)..add([create, joinAna, joinBen, expense, rate]);
    final benOwes = obligationFor(ben, log.fold())!;

    // The debt exists and cannot be carried. Both facts survive.
    expect(benOwes.settlements.single.to, 'ana');
    expect(benOwes.unpayable.single.id, 'ana');
    expect(benOwes.unpayable.single.reason, 'no_address');
    expect(benOwes.withheldMinorUnits, 4500);
    expect(benOwes.isComplete, isFalse,
        reason: 'a request that covers less than the plan must say so');
  });

  test('an unpriced bill is an ordinary bill, not an error', () {
    final ana = FakeHost(me: 'ana');
    final create = createBill(
        host: ana, name: 'Dinner', currency: 'EUR', creatorKey: fakeKey('ana'));
    ana.tick();
    final joinAna = joinBill(host: ana, name: 'Ana', payTo: 'u1ana');
    final log = BillLog(ana)..add([create, joinAna]);

    expect(log.fold().bill.rate, isNull);
    expect(obligationFor(ana, log.fold()), isNull,
        reason: 'there is no refusal code for unpriced, so there is none here');
  });

  test('the record says what was owed when the request was made', () async {
    final d = dinner();
    final benOwes = obligationFor(d.ben, d.log.fold())!;
    expect(benOwes.settlements.single.amount, 4500);

    // A wallet whose send triggers a sync: a peer's expense lands while the
    // transaction is in flight, and what ben owes changes underneath. The
    // transaction that was sent paid the earlier figure and cannot be unsent,
    // so the record has to be that figure too.
    final ana2 = FakeHost(me: 'ana', at: DateTime.utc(2026, 10, 28, 20));
    late final BillLog log;
    final syncing = _HostThatSyncsOnSend(d.ben, () {
      log.add([
        addExpense(
          host: ana2,
          expenseId: 'x2',
          paidBy: 'ana',
          amount: 5000,
          split: const {
            'type': 'equal',
            'among': ['ana', 'ben'],
          },
        )
      ]);
    });
    log = d.log;

    final settled = await settle(syncing, log, benOwes);

    // What ben owes is now 7000. The record must still be 4500 — the amount
    // the request carried and the transaction moved.
    expect(splitz.netBalances(log.fold().bill)['ben'], -7000);
    final record = settled.records.single['payment'] as Map<String, dynamic>;
    expect(record['amount'], 4500,
        reason: 'the record is what was sent, not what is owed now');
    expect(record['id'], 'ben:${settled.txid}:ana',
        reason: 'a record carries its own id; the transaction is the '
            'reference');
    expect(record['reference'], settled.txid);
  });

  test('the record of a send is signed, so a verifying fold keeps it',
      () async {
    // A stand-in curve: an entry verifies when it carries the one signature
    // this host produces. Enough to bind everyone who signed, which is what
    // makes §10.3 insist on a verifying copy of every entry they author.
    Future<String> sign(List<int> message) async => 'good';
    bool verify(Map<String, dynamic> e, String key) => e['sig'] == 'good';

    // §10.7: a participant who publishes a key is named by the id it derives.
    final benId = splitz.participantId(fakeKey('ben'))!;

    Future<({BillLog log, FakeHost ben})> signedDinner(
        {required bool benSigns}) async {
      final ana = FakeHost(
          me: 'ana', payToAddress: 'u1ana', sign: sign, verify: verify);
      final ben = FakeHost(
        me: benId,
        payToAddress: 'u1ben',
        sign: benSigns ? sign : null,
        verify: verify,
      );
      final signer = FakeHost(me: benId, payToAddress: 'u1ben', sign: sign);
      final create = createBill(
          host: ana,
          name: 'Dinner',
          currency: 'EUR',
          creatorKey: fakeKey('ana'));
      final billId = create['id'] as String;
      final entries = <Map<String, dynamic>>[
        await signEntry(host: ana, entry: create, billId: billId),
      ];
      ana.tick();
      entries.add(await signEntry(
          billId: billId,
          host: ana,
          entry: joinBill(host: ana, name: 'Ana', payTo: 'u1ana')));
      for (var i = 0; i < 2; i++) {
        signer.tick();
      }
      entries.add(await signEntry(
          billId: billId,
          host: signer,
          entry: joinBill(
              host: signer,
              name: 'Ben',
              payTo: 'u1ben',
              identityKey: fakeKey('ben'))));
      ana.tick();
      entries.add(await signEntry(
          billId: billId,
          host: ana,
          entry: addExpense(
            host: ana,
            expenseId: 'x1',
            paidBy: 'ana',
            amount: 9000,
            split: {
              'type': 'equal',
              'among': ['ana', benId],
            },
          )));
      ana.tick();
      entries.add(await signEntry(
          billId: billId,
          host: ana,
          entry: setRate(host: ana, currency: 'EUR', minorUnitsPerZec: 51234)));
      final log = BillLog(ben, billId: entries.first['id'] as String);
      expect(log.add(entries), isEmpty);
      for (var i = 0; i < 5; i++) {
        ben.tick();
      }
      return (log: log, ben: ben);
    }

    final d = await signedDinner(benSigns: true);
    expect(d.log.fold().identities.bound.keys, containsAll(['ana', benId]));
    final settled =
        await settle(d.ben, d.log, obligationFor(d.ben, d.log.fold())!);
    expect(settled.records.single['sig'], 'good');
    final after = d.log.fold();
    expect(after.setAside, isEmpty);
    expect(after.bill.payments.single.from, benId,
        reason: "ben's own record is on the bill on ben's own device");

    // The rule the signature satisfies: the same record unsigned is not the
    // bound payer speaking, and a verifying fold sets it aside.
    final u = await signedDinner(benSigns: false);
    final unsigned =
        await settle(u.ben, u.log, obligationFor(u.ben, u.log.fold())!);
    expect(unsigned.records.single.containsKey('sig'), isFalse);
    final refused = u.log.fold();
    expect(refused.bill.payments, isEmpty);
    expect(refused.setAside.single.code, splitz.SplitCode.unauthorizedEntry);
  });

  test('a send that was built but not broadcast records nothing', () async {
    // A wallet answers with three outcomes, not two. A transaction built and
    // not yet handed to the network may still land: recorded as paid it
    // settles a debt nothing on chain settled, and treated as failed it gets
    // paid twice on the retry.
    final d = dinner();
    final pending = _FixedOutcome(
      d.ben,
      const Sent.pending(detail: 'created, not broadcast'),
    );
    final benOwes = obligationFor(pending, d.log.fold())!;

    final settled = await settle(pending, d.log, benOwes);

    expect(settled.result, SendResult.pending);
    expect(settled.records, isEmpty);
    expect(settled.txid, isNull);
    expect(settled.detail, 'created, not broadcast');
    expect(d.log.fold().bill.payments, isEmpty,
        reason: 'nothing on chain has moved, so nothing is recorded');
    expect(splitz.netBalances(d.log.fold().bill)['ben'], -4500,
        reason: 'the debt stands exactly as it did before the attempt');
  });

  test('a pending send keeps the transaction the wallet built', () async {
    // Nothing is recorded from it, but the id is what a person looks for in
    // the wallet's history to learn which way the send went.
    final d = dinner();
    final pending = _FixedOutcome(
        d.ben, const Sent.pending(detail: 'not confirmed', txid: 'tx-held'));
    final settled =
        await settle(pending, d.log, obligationFor(pending, d.log.fold())!);
    expect(settled.result, SendResult.pending);
    expect(settled.txid, 'tx-held');
    expect(settled.records, isEmpty);
    expect(d.log.fold().bill.payments, isEmpty);
  });

  test('a record states what it sent in ZEC and the rate it was priced at',
      () async {
    // §9.2. The payee confirms against a figure they can compare with what
    // arrived: a fiat amount alone hides a rate lowered before paying.
    final d = twoDebts();
    final owed = obligationFor(d.ana, d.log.fold())!;
    final settled = await settle(d.ana, d.log, owed);
    final byTo = {
      for (final r in settled.records)
        (r['payment'] as Map)['to']: (r['payment'] as Map)
    };
    for (var i = 0; i < owed.request.payments.length; i++) {
      final to = owed.request.recipients[i];
      expect(byTo[to]!['zatoshi'], owed.request.payments[i].zatoshi);
      expect(byTo[to]!['paidAtRate'], splitz.rateToJson(owed.rate));
    }
    final paid = d.log.fold().bill.payments;
    expect(paid.map((p) => p.zatoshi), everyElement(isNotNull));
    expect(paid.map((p) => p.paidAtRate?.minorUnitsPerZec),
        everyElement(owed.rate.minorUnitsPerZec));
  });

  test('a pending send found on chain later records what a sent one would',
      () async {
    // Two logs from one bill: in one the send succeeds at once; in the other
    // it is left pending and recorded afterwards from the txid. The records
    // are the same, so a payee confirming either confirms the same payment.
    final now = twoDebts();
    final sent =
        await settle(now.ana, now.log, obligationFor(now.ana, now.log.fold())!);
    expect(sent.result, SendResult.sent);

    final later = twoDebts();
    final pending = _FixedOutcome(later.ana, const Sent.pending());
    final owed = obligationFor(pending, later.log.fold())!;
    expect((await settle(pending, later.log, owed)).records, isEmpty);
    expect(owed.carriedTo, {'ben': 3000, 'cat': 3000});

    final records = await recordSend(
        later.ana, later.log, owed.carriedTo, sent.txid!,
        zatoshi: owed.carriedZatoshi, rate: owed.rate);
    // `at` is when the record was written, which is later for a recovery.
    Map<String, dynamic> payment(Map<String, dynamic> r) =>
        {...r['payment'] as Map<String, dynamic>}..remove('at');
    expect(records.map(payment).toList(), sent.records.map(payment).toList());
    expect(later.log.fold().setAside, isEmpty);
    expect(later.log.fold().bill.payments.map((p) => p.id).toSet(),
        {'ana:${sent.txid}:ben', 'ana:${sent.txid}:cat'});
  });

  test('what a request carries leaves out whom it cannot pay', () {
    // Ana owes ben 30.00 and cat 30.00; cat has published no address, so the
    // request carries ben's share alone and a send records only that.
    final ana = FakeHost(me: 'ana', payToAddress: 'u1ana');
    final ben = FakeHost(me: 'ben');
    final cat = FakeHost(me: 'cat');
    final create = createBill(
        host: ana, name: 'Dinner', currency: 'EUR', creatorKey: fakeKey('ana'));
    ana.tick();
    final joinAna = joinBill(host: ana, name: 'Ana', payTo: 'u1ana');
    ben.tick();
    ben.tick();
    final joinBen = joinBill(host: ben, name: 'Ben', payTo: 'u1ben');
    cat.tick();
    cat.tick();
    cat.tick();
    final joinCat = joinBill(host: cat, name: 'Cat');
    final among = const {
      'type': 'equal',
      'among': ['ana', 'ben', 'cat'],
    };
    ben.tick();
    final e1 = addExpense(
        host: ben, expenseId: 'x1', paidBy: 'ben', amount: 9000, split: among);
    cat.tick();
    final e2 = addExpense(
        host: cat, expenseId: 'x2', paidBy: 'cat', amount: 9000, split: among);
    ana.tick();
    final rate = setRate(host: ana, currency: 'EUR', minorUnitsPerZec: 51234);
    final log = BillLog(ana);
    expect(log.add([create, joinAna, joinBen, joinCat, e1, e2, rate]), isEmpty);

    final owed = obligationFor(ana, log.fold())!;
    expect(owed.settlements.map((s) => s.to), ['ben', 'cat']);
    expect(owed.unpayable.map((u) => u.id), ['cat']);
    expect(owed.carriedTo, {'ben': 3000});
  });

  test('an address the payer cannot read is unpayable, and the rest is paid',
      () {
    // Ana owes ben 30.00 and cat 30.00. Cat's `payTo` passes §8.3's alphabet
    // but this wallet's reader refuses it; a request naming it would be
    // refused whole, so ben's share goes out alone.
    final d = twoDebts();
    final reading = FakeHost(
      me: 'ana',
      payToAddress: 'u1ana',
      readsAddress: (a) => a != 'u1cat',
    );
    final owed = obligationFor(reading, d.log.fold())!;
    expect(
        owed.unpayable.map((u) => '${u.id}:${u.reason}'), ['cat:bad_address']);
    expect(owed.carriedTo, {'ben': 3000});
    expect(owed.uri, isNot(contains('u1cat')));

    final everything = obligationFor(d.ana, d.log.fold())!;
    expect(everything.unpayable, isEmpty);
    expect(everything.carriedTo, {'ben': 3000, 'cat': 3000});
  });

  test('a send that failed records nothing and says why', () async {
    final d = dinner();
    final failing = _FixedOutcome(d.ben, const Sent.failed(detail: 'no funds'));
    final benOwes = obligationFor(failing, d.log.fold())!;

    final settled = await settle(failing, d.log, benOwes);

    expect(settled.result, SendResult.failed);
    expect(settled.records, isEmpty);
    expect(settled.detail, 'no funds');
    expect(d.log.fold().bill.payments, isEmpty);
  });

  test('a part payment holds the whole debt, and reports both figures', () {
    // Requesting only the remainder would overpay by the pending amount if
    // that payment lands, and an overpayment cannot be recovered. Waiting
    // costs time. So the whole debt is held — and `awaiting` says what is
    // owed and what is actually in flight, which are different numbers.
    final d = dinner();
    d.ben.tick();
    d.log.add([
      recordPayment(host: d.ben, paymentId: 'tx1', to: 'ana', amount: 2000),
    ]);

    final o = obligationFor(d.ben, d.log.fold())!;
    expect(o.settlements, isEmpty);
    expect(o.uri, isNull);
    expect(o.awaiting.single.to, 'ana');
    expect(o.awaiting.single.owed, 4500);
    expect(o.awaiting.single.paid, 2000,
        reason: 'reporting 4500 in flight would be a false statement: '
            'only 2000 was sent');
  });

  test('two part payments to one payee add up', () {
    final d = dinner();
    d.ben.tick();
    d.log.add([
      recordPayment(host: d.ben, paymentId: 'tx1', to: 'ana', amount: 2000),
    ]);
    d.ben.tick();
    d.log.add([
      recordPayment(host: d.ben, paymentId: 'tx2', to: 'ana', amount: 1500),
    ]);

    final o = obligationFor(d.ben, d.log.fold())!;
    expect(o.awaiting.single.paid, 3500);
    expect(o.awaiting.single.owed, 4500);
  });

  test('a confirmation is what clears the debt', () async {
    final d = dinner();
    final benOwes = obligationFor(d.ben, d.log.fold())!;
    final settled = await settle(d.ben, d.log, benOwes);

    d.ana.tick();
    // §10.5: `onChain` needs a reference, and is the payee's to state.
    final confirm = confirmPayment(
      host: d.ana,
      paymentId: 'ben:${settled.txid}:ana',
      method: 'onChain',
      reference: settled.txid,
      record: d.log.fold().paymentDigests['ben:${settled.txid}:ana']!,
    );
    d.log.add([confirm]);

    final after = d.log.fold();
    expect(after.bill.confirmedPayments, contains('ben:${settled.txid}:ana'));
    expect(splitz.netBalances(after.bill)['ben'], 0);
    expect(splitz.netBalances(after.bill)['ana'], 0);
  });

  test('a debt already paid and not yet confirmed is not requested again', () {
    // Section 10.5: a payment record is a claim, so the balance does not move
    // until the payee confirms. The debt therefore still stands in the plan,
    // and a request built from the plan alone asks the payer to send it twice.
    final d = dinner();

    final first = obligationFor(d.ben, d.log.fold())!;
    expect(first.settlements.single.to, 'ana');
    expect(first.settlements.single.amount, 4500);

    d.ben.tick();
    d.log.add([
      recordPayment(host: d.ben, paymentId: 'tx1', to: 'ana', amount: 4500),
    ]);

    final second = obligationFor(d.ben, d.log.fold())!;
    expect(
      second.settlements.where((s) => s.to == 'ana'),
      isEmpty,
      reason: 'ana was already paid 4500 and has not confirmed; asking '
          'again sends the same money twice',
    );
    expect(second.awaiting.single.to, 'ana');
    expect(second.awaiting.single.owed, 4500);
    expect(second.awaiting.single.paid, 4500,
        reason: 'the whole debt was sent, so the whole debt is in flight');
    expect(second.uri, isNull,
        reason: 'nothing left to request, so there is no request');
  });

  test('voiding the record of a payment that never landed restores the debt',
      () {
    // What the guard above turns away is a retry after a send that failed:
    // the record says paid, nobody confirms, and the debt sits in `awaiting`
    // forever. Voiding the record is the way out, and it has to work.
    final d = dinner();
    d.ben.tick();
    final record =
        recordPayment(host: d.ben, paymentId: 'tx1', to: 'ana', amount: 4500);
    d.log.add([record]);

    expect(obligationFor(d.ben, d.log.fold())!.settlements, isEmpty);

    d.ben.tick();
    d.log.add([voidEntry(host: d.ben, targetId: record['id'] as String)]);

    final retry = obligationFor(d.ben, d.log.fold())!;
    expect(retry.awaiting, isEmpty);
    expect(retry.settlements.single.to, 'ana');
    expect(retry.settlements.single.amount, 4500);
    expect(retry.uri, startsWith('zcash:u1ana'));
  });

  test('one transaction paying two people is two records, each confirmable',
      () async {
    // §10.5 gives every record its own id. Under one shared id the fold sets
    // the second aside as `duplicate_payment`, so a payment that was made
    // leaves no record on the bill: its payee is still shown as owed, cannot
    // confirm — the surviving record names somebody else — and the payer has
    // already sent the money.
    final d = twoDebts();
    final owed = obligationFor(d.ana, d.log.fold())!;
    expect(owed.settlements.map((s) => s.to), ['ben', 'cat']);

    final settled = await settle(d.ana, d.log, owed);
    final txid = settled.txid!;
    expect(settled.records.length, 2);
    expect(
      settled.records.map((r) => (r['payment'] as Map<String, dynamic>)['id']),
      ['ana:$txid:ben', 'ana:$txid:cat'],
    );
    expect(
      settled.records
          .map((r) => (r['payment'] as Map<String, dynamic>)['reference']),
      [txid, txid],
      reason: 'the transaction ties both records to the chain, and is what '
          '`onChain` reads',
    );

    final after = d.log.fold();
    expect(after.setAside, isEmpty,
        reason: 'neither record is a duplicate of the other');
    // §10.2 orders the log, and these two records share an instant, so the
    // tiebreak is by entry id — which depends on this test's own clock and
    // nonce. What is asserted is that both survive, not the order they land in.
    expect(
        after.bill.payments.map((p) => p.to).toList()..sort(), ['ben', 'cat']);

    // Ben confirms his own record and clears his own half. Cat's stands.
    final ben = FakeHost(me: 'ben', payToAddress: 'u1ben');
    for (var i = 0; i < 8; i++) {
      ben.tick();
    }
    expect(
      d.log.add([
        confirmPayment(
          host: ben,
          paymentId: 'ana:$txid:ben',
          method: 'recipientConfirmed',
          record: d.log.fold().paymentDigests['ana:$txid:ben']!,
        )
      ]),
      isEmpty,
    );

    final end = d.log.fold();
    expect(end.setAside, isEmpty);
    expect(end.bill.confirmedPayments, {'ana:$txid:ben'});
    expect(splitz.netBalances(end.bill)['ben'], 0);
    expect(splitz.netBalances(end.bill)['cat'], 3000);
    expect(splitz.netBalances(end.bill)['ana'], -3000);
  });
}

/// A host whose send makes the world move: the wallet syncs while the
/// transaction is in flight.
class _HostThatSyncsOnSend implements BillHost {
  @override
  ReadsAddress? get readsAddress => null;
  _HostThatSyncsOnSend(this._inner, this._onSend);
  final BillHost _inner;
  final void Function() _onSend;

  @override
  String get me => _inner.me;
  @override
  Clock get now => _inner.now;
  @override
  Randomness get randomBytes => _inner.randomBytes;
  @override
  SignEntry? get sign => _inner.sign;
  @override
  VerifyEntry? get verify => _inner.verify;

  @override
  Broadcast get broadcast => (uri) async {
        _onSend();
        return const Sent.sent('tx-broadcast');
      };
}

/// A host whose every send ends the same way.
class _FixedOutcome implements BillHost {
  @override
  ReadsAddress? get readsAddress => null;
  _FixedOutcome(this._inner, this._outcome);

  final BillHost _inner;
  final Sent _outcome;

  @override
  String get me => _inner.me;
  @override
  Clock get now => _inner.now;
  @override
  Randomness get randomBytes => _inner.randomBytes;
  @override
  SignEntry? get sign => _inner.sign;
  @override
  VerifyEntry? get verify => _inner.verify;

  @override
  Broadcast get broadcast => (uri) async => _outcome;
}
