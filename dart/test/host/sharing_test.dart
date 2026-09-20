import 'package:test/test.dart';
import 'package:splitz_core/host.dart';

import 'support/fake_host.dart';

({BillLog log, FakeHost ana, String key}) opened() {
  final ana = FakeHost(me: 'ana', payToAddress: 'u1ana');
  final key = fakeKey('ana');
  final create =
      createBill(host: ana, name: 'Dinner', currency: 'EUR', creatorKey: key);
  ana.tick();
  final join = joinBill(host: ana, name: 'Ana', payTo: 'u1ana');
  return (log: BillLog(ana)..add([create, join]), ana: ana, key: key);
}

void main() {
  test('an invite round-trips through the protocol parser', () {
    final o = opened();
    final uri = inviteFor(bill: o.log.fold().bill, key: o.key, name: 'Dinner');

    final scan = readScan(uri);
    expect(scan, isA<ScannedInvite>());
    final invite = (scan as ScannedInvite).invite;
    expect(invite.billId, o.log.fold().bill.id);
    expect(invite.key, o.key);
  });

  test('a whole bill travels in one square and opens on the other side', () {
    final o = opened();
    final square =
        shareableBill(log: o.log, key: o.key, bill: o.log.fold().bill);
    expect(square, isNotNull);
    expect(square, startsWith('splitz1:'));

    final scan = readScan(square!);
    expect(scan, isA<ScannedBill>());
    final got = scan as ScannedBill;
    expect(got.invite?.billId, o.log.fold().bill.id);

    // A second device, holding nothing, ends up with the same bill.
    final ben = FakeHost(me: 'ben', payToAddress: 'u1ben');
    final theirs = BillLog(ben);
    expect(acceptScan(theirs, got), isEmpty, reason: 'nothing to refuse');
    expect(theirs.fold().bill.id, o.log.fold().bill.id);
    expect(hasJoined(ben, theirs.fold().bill), isFalse,
        reason: 'holding a bill is not being on it');
  });

  test('a delta carries only what the peer has not seen, and no key', () {
    final o = opened();
    final ben = FakeHost(me: 'ben', payToAddress: 'u1ben');
    final theirs = BillLog(ben)
      ..add((readScan(shareableBill(
              log: o.log, key: o.key, bill: o.log.fold().bill)!) as ScannedBill)
          .entries);

    // ben joins on his own device; ana has not seen it.
    ben.tick();
    theirs.add([joinBill(host: ben, name: 'Ben', payTo: 'u1ben')]);

    final delta = deltaFor(
        log: theirs, theyHave: o.log.entries.map((e) => e['id'] as String));
    expect(delta, isA<DeltaSquare>());
    expect((delta as DeltaSquare).entryCount, 1);
    expect(delta.uri, startsWith('splitzd1:'));

    final scan = readScan(delta.uri) as ScannedBill;
    expect(scan.entries, hasLength(1));
    expect(scan.invite, isNull,
        reason: 'a delta reader already holds the key (§11.2)');

    o.log.add(scan.entries);
    expect(o.log.fold().bill.participants.map((p) => p.id), ['ana', 'ben']);
  });

  test('nothing missing and too much missing are different answers', () {
    final o = opened();
    expect(
        deltaFor(
            log: o.log, theyHave: o.log.entries.map((e) => e['id'] as String)),
        isA<NothingMissing>());

    // A peer that has seen none of it, on a log too long for one square. Both
    // used to answer null, so a wallet told somebody their bill was up to
    // date while every entry on it had never reached them.
    final ana = FakeHost(me: 'ana');
    final key = fakeKey('ana');
    final big = BillLog(ana)
      ..add([
        createBill(host: ana, name: 'D', currency: 'EUR', creatorKey: key),
      ]);
    for (var i = 0; i < 25; i++) {
      ana.tick();
      big.add([
        addExpense(
          host: ana,
          expenseId: 'x$i',
          paidBy: 'ana',
          amount: 100,
          split: const {
            'type': 'equal',
            'among': ['ana'],
          },
        ),
      ]);
    }

    final behind = deltaFor(log: big, theyHave: const []);
    expect(behind, isA<TooBigForOneSquare>());
    expect((behind as TooBigForOneSquare).entryCount, big.entries.length);
    expect(behind.code, 'payload_too_large');
  });

  test('a bill past the scan cap is null, not an exception', () {
    final ana = FakeHost(me: 'ana');
    final key = fakeKey('ana');
    final log = BillLog(ana)
      ..add(
          [createBill(host: ana, name: 'D', currency: 'EUR', creatorKey: key)]);

    // §11.2 caps a payload. Enough participants carrying real addresses and it
    // no longer fits — which is a state to show, not an error to report.
    for (var i = 0; i < 8; i++) {
      ana.tick();
      final who = FakeHost(me: 'p$i', at: DateTime.utc(2026, 10, 28, 20, i));
      log.add([
        joinBill(
            host: who,
            name: 'Participant number $i',
            payTo: 'u1${'x' * 104}',
            identityKey: fakeKey('p'))
      ]);
    }
    expect(shareableBill(log: log, key: key, bill: log.fold().bill), isNull);
  });

  test('something that is neither is refused with a code', () {
    final scan = readScan('https://example.com/not-a-bill');
    expect(scan, isA<ScanRefused>());
    expect((scan as ScanRefused).code, 'invite_not_an_invite');
  });
}
