/// §14.3's guard: a send is written down before the wallet is called, and
/// blocks the next one from the same bill until it is resolved.
library;

import 'package:splitz_core/host.dart' as entries;
import 'package:splitz_core/splitz_core.dart' as splitz;
import 'package:splitz_host/splitz_host.dart';
import 'package:test/test.dart';

import 'support/fake_wallet.dart';

const _txid =
    'aa00000000000000000000000000000000000000000000000000000000000001';

PendingSend _send({String billId = 'b1', Map<String, int>? carried}) =>
    PendingSend(
      billId: billId,
      uri: 'zcash:u1ben?amount=0.1',
      carried: carried ?? const {'ben': 1000},
      at: '2026-10-28T19:30:00.000Z',
      sent: const {'ben': 10000000},
      rate: const splitz.ExchangeRate(
        currency: 'USD',
        minorUnitsPerZec: 10000,
        at: '2026-10-28T19:00:00.000Z',
        source: 'a named feed',
      ),
    );

/// Storage whose reads and writes can be made to fail, key by key.
class _Flaky extends InMemoryBillStorage {
  bool unreadable = false;
  bool refuseWrites = false;

  @override
  Future<String?> read(String key) async {
    if (unreadable) throw BillStorageUnreadable(key, 'damaged');
    return super.read(key);
  }

  @override
  Future<void> write(String key, String value) async {
    if (refuseWrites) throw StateError('disk full');
    return super.write(key, value);
  }
}

void main() {
  group('a send is written down before the wallet is called', () {
    test('and the next send from that bill is refused', () async {
      final sends = PendingSends(InMemoryBillStorage());
      await sends.begin(_send());
      await sends.end('b1', SendEnded.unresolved);
      final held = await sends.of('b1');
      expect(held?.uri, 'zcash:u1ben?amount=0.1');
      expect(held?.carried, {'ben': 1000});
      expect(held?.rate?.minorUnitsPerZec, 10000);
      await expectLater(
        sends.begin(_send()),
        throwsA(
          isA<SendInFlight>().having((e) => e.pending?.billId, 'held', 'b1'),
        ),
      );
    });

    test('another bill is not blocked', () async {
      final sends = PendingSends(InMemoryBillStorage());
      await sends.begin(_send());
      await sends.begin(_send(billId: 'b2'));
      expect(await sends.of('b2'), isNotNull);
    });

    test(
      'a second send started before the first is written is refused',
      () async {
        final sends = PendingSends(InMemoryBillStorage());
        final first = sends.begin(_send());
        await expectLater(
          sends.begin(_send()),
          throwsA(
            isA<SendInFlight>().having((e) => e.pending, 'pending', isNull),
          ),
        );
        await first;
      },
    );

    test('a note that failed to write lets the next send try again', () async {
      final storage = _Flaky()..refuseWrites = true;
      final sends = PendingSends(storage);
      await expectLater(sends.begin(_send()), throwsStateError);
      storage.refuseWrites = false;
      await sends.begin(_send());
      expect(await sends.of('b1'), isNotNull);
    });
  });

  group('§14.3: what each outcome does to the note', () {
    Future<PendingSends> started() async {
      final sends = PendingSends(InMemoryBillStorage());
      await sends.begin(_send());
      return sends;
    }

    test('refused: the note goes, and the debt can be sent again', () async {
      final sends = await started();
      await sends.end('b1', SendEnded.refused);
      expect(await sends.of('b1'), isNull);
      await sends.begin(_send());
    });

    test('reached the network and recorded: the note goes', () async {
      final sends = await started();
      await sends.end(
        'b1',
        SendEnded.reachedNetwork,
        txid: _txid,
        recorded: true,
      );
      expect(await sends.of('b1'), isNull);
    });

    test('reached the network and not recorded: it stays, with the '
        'transaction', () async {
      final sends = await started();
      await sends.end('b1', SendEnded.reachedNetwork, txid: _txid);
      expect((await sends.of('b1'))?.txid, _txid);
      await expectLater(sends.begin(_send()), throwsA(isA<SendInFlight>()));
    });

    test(
      'unresolved: it stays, with the transaction the wallet built',
      () async {
        final sends = await started();
        await sends.end('b1', SendEnded.unresolved, txid: _txid);
        expect((await sends.of('b1'))?.txid, _txid);
      },
    );

    test('unresolved with no transaction named: it stays as written', () async {
      final sends = await started();
      await sends.end('b1', SendEnded.unresolved);
      final held = await sends.of('b1');
      expect(held, isNotNull);
      expect(held!.txid, isNull);
    });

    test('resolving removes it', () async {
      final sends = await started();
      await sends.end('b1', SendEnded.unresolved);
      await sends.resolve('b1');
      expect(await sends.of('b1'), isNull);
    });
  });

  group('a note that will not read still blocks', () {
    test('when it is not JSON', () async {
      final storage = InMemoryBillStorage();
      await storage.write('pendingsend/b1', '{not json');
      final sends = PendingSends(storage);
      final held = await sends.of('b1');
      expect(held?.damaged, isTrue);
      await expectLater(sends.begin(_send()), throwsA(isA<SendInFlight>()));
    });

    test('when it is JSON this class did not write', () async {
      final storage = InMemoryBillStorage();
      await storage.write('pendingsend/b1', '{"billId":"b1","uri":""}');
      expect((await PendingSends(storage).of('b1'))?.damaged, isTrue);
    });

    test('when it names another bill', () async {
      final storage = InMemoryBillStorage();
      final other = _send(billId: 'b2').toJson();
      await storage.write('pendingsend/b1', '$other'.replaceAll("'", '"'));
      expect((await PendingSends(storage).of('b1'))?.damaged, isTrue);
    });

    test('when the storage cannot read it', () async {
      final storage = _Flaky();
      final sends = PendingSends(storage);
      await sends.begin(_send());
      await sends.end('b1', SendEnded.unresolved);
      storage.unreadable = true;
      expect((await sends.of('b1'))?.damaged, isTrue);
    });
  });

  group('recording a send that turned out to land', () {
    ({entries.BillLog log, FakeHost ana}) bill() {
      final ana = FakeHost(me: 'ana', payToAddress: 'u1ana');
      final ben = FakeHost(me: 'ben');
      final log = entries.BillLog(
        ana,
        entries: [
          entries.createBill(
            host: ana,
            name: 'D',
            currency: 'USD',
            creatorKey: fakeKey('ana'),
          ),
          entries.joinBill(host: ana, name: 'Ana', payTo: 'u1ana'),
          entries.joinBill(host: ben, name: 'Ben', payTo: 'u1ben'),
        ],
      );
      ana.tick();
      return (log: log, ana: ana);
    }

    test('records each recipient under the transaction', () async {
      final b = bill();
      final records = await PendingSends(
        InMemoryBillStorage(),
      ).recordsFor(b.ana, b.log, _send(), '  ${_txid.toUpperCase()} ');
      expect(records, hasLength(1));
      final payment = b.log.fold().bill.payments.single;
      expect(payment.id, entries.paymentIdForSend(b.ana.me, _txid, 'ben'));
      expect(payment.amount, 1000);
      expect(payment.to, 'ben');
    });

    test('does not record a recipient twice', () async {
      final b = bill();
      final sends = PendingSends(InMemoryBillStorage());
      await sends.recordsFor(b.ana, b.log, _send(), _txid);
      final again = await sends.recordsFor(b.ana, b.log, _send(), _txid);
      expect(again, isEmpty);
      expect(b.log.fold().bill.payments, hasLength(1));
    });

    test('refuses what is not a transaction id', () async {
      final b = bill();
      await expectLater(
        PendingSends(
          InMemoryBillStorage(),
        ).recordsFor(b.ana, b.log, _send(), _txid.substring(1)),
        throwsA(
          isA<Unrecordable>().having(
            (e) => e.reason,
            'reason',
            UnrecordableReason.notATransactionId,
          ),
        ),
      );
    });

    test('refuses a note that lost its details', () async {
      final b = bill();
      await expectLater(
        PendingSends(
          InMemoryBillStorage(),
        ).recordsFor(b.ana, b.log, const PendingSend.damaged('b1'), _txid),
        throwsA(
          isA<Unrecordable>().having(
            (e) => e.reason,
            'reason',
            UnrecordableReason.detailsLost,
          ),
        ),
      );
    });

    test('refuses a swap, which is recorded by its reference', () async {
      final b = bill();
      final swap = PendingSend(
        billId: 'b1',
        uri: 'zcash:t1deposit?amount=0.1',
        carried: const {'ben': 1000},
        at: '2026-10-28T19:30:00.000Z',
        swap: const SwapWatch(
          billId: 'b1',
          reference: 'ref-1',
          to: 'ben',
          depositAddress: 't1deposit',
          assetSymbol: 'USDC',
          assetChain: 'base',
        ),
        zatoshi: 10000000,
      );
      await expectLater(
        PendingSends(
          InMemoryBillStorage(),
        ).recordsFor(b.ana, b.log, swap, _txid),
        throwsA(
          isA<Unrecordable>().having(
            (e) => e.reason,
            'reason',
            UnrecordableReason.isASwap,
          ),
        ),
      );
    });
  });

  test('a note reads back as it was written', () {
    final send = _send().sentAs(_txid);
    final back = PendingSend.fromJson(send.toJson());
    expect(back?.toJson(), send.toJson());
  });

  group('§14.3: saying a send left nothing in the wallet', () {
    final note = _send();
    const before = OwnTransaction(
      txid: 'aa',
      created: '2026-10-28T19:29:59.000Z',
    );
    const sameSecond = OwnTransaction(
      txid: 'bb',
      created: '2026-10-28T19:30:00.000Z',
    );
    const after = OwnTransaction(
      txid: 'cc',
      created: '2026-10-28T19:31:12.000Z',
    );

    test('a transaction built since the note holds it, and is named', () {
      final r = unsentClaimRefusal(
        note,
        stillSending: false,
        own: const [before, after],
      );
      expect(r?.claim, UnsentClaim.builtSince);
      expect(r?.txid, 'cc');
    });

    test('one built in the note\'s own second holds it too', () {
      // The wallet stamps whole seconds; the note was written first.
      expect(
        unsentClaimRefusal(
          note,
          stillSending: false,
          own: const [sameSecond],
        )?.claim,
        UnsentClaim.builtSince,
      );
    });

    test('one built before the note does not hold it', () {
      expect(
        unsentClaimRefusal(note, stillSending: false, own: const [before]),
        isNull,
      );
      expect(
        unsentClaimRefusal(note, stillSending: false, own: const []),
        isNull,
      );
    });

    test('a later payment of something else does not hold it', () {
      // The note's request sends 10000000 zatoshi; the shop took 2500000.
      const shop = OwnTransaction(
        txid: 'ab',
        created: '2026-10-28T20:30:00.000Z',
        sent: 2500000,
      );
      expect(
        unsentClaimRefusal(note, stillSending: false, own: const [shop]),
        isNull,
      );
    });

    test('a later transaction that sent what the note sends holds it', () {
      const maybe = OwnTransaction(
        txid: 'cc',
        created: '2026-10-28T19:31:12.000Z',
        sent: 10000000,
      );
      final r = unsentClaimRefusal(
        note,
        stillSending: false,
        own: const [maybe],
      );
      expect(r?.claim, UnsentClaim.builtSince);
      expect(r?.txid, 'cc');
      // One zatoshi either way is another payment.
      for (final other in [9999999, 10000001]) {
        expect(
          unsentClaimRefusal(
            note,
            stillSending: false,
            own: [
              OwnTransaction(
                txid: 'dd',
                created: '2026-10-28T19:31:12.000Z',
                sent: other,
              ),
            ],
          ),
          isNull,
        );
      }
    });

    test('a note that does not say what it sends is held by any later one', () {
      final silent = PendingSend(
        billId: 'b1',
        uri: note.uri,
        carried: note.carried,
        at: note.at,
      );
      const shop = OwnTransaction(
        txid: 'ab',
        created: '2026-10-28T20:30:00.000Z',
        sent: 2500000,
      );
      expect(
        unsentClaimRefusal(
          silent,
          stillSending: false,
          own: const [shop],
        )?.claim,
        UnsentClaim.builtSince,
      );
      // A swap deposit's note says it in `zatoshi`.
      final deposit = PendingSend(
        billId: 'b1',
        uri: note.uri,
        carried: note.carried,
        at: note.at,
        zatoshi: 2500000,
      );
      expect(
        unsentClaimRefusal(deposit, stillSending: false, own: const [shop]),
        isNotNull,
      );
      expect(
        unsentClaimRefusal(
          deposit,
          stillSending: false,
          own: const [
            OwnTransaction(
              txid: 'ab',
              created: '2026-10-28T20:30:00.000Z',
              sent: 7,
            ),
          ],
        ),
        isNull,
      );
    });

    test('anything still sending holds it, whatever it is', () {
      expect(
        unsentClaimRefusal(note, stillSending: true, own: const [])?.claim,
        UnsentClaim.stillSending,
      );
    });

    test('a note that will not read is held only by what is still sending', () {
      const damaged = PendingSend.damaged('b1');
      expect(
        unsentClaimRefusal(damaged, stillSending: false, own: const [after]),
        isNull,
      );
      expect(
        unsentClaimRefusal(damaged, stillSending: true, own: const [])?.claim,
        UnsentClaim.stillSending,
      );
    });
  });

  group("§14.4: withdrawing one's own record of a shielded payment", () {
    splitz.PaymentRecord payment({
      String from = 'me',
      String method = 'shieldedZec',
      String? reference = 'tx1',
    }) => splitz.PaymentRecord(
      id: 'p1',
      from: from,
      to: 'ben',
      amount: 1000,
      currency: 'USD',
      method: method,
      at: '2026-10-28T19:30:00.000Z',
      reference: reference,
    );

    test('refused while the wallet shows it mined or still sending', () {
      expect(
        ownPaymentWithdrawalRefusal(
          payment(),
          me: 'me',
          state: TransactionState.mined,
        ),
        OwnPaymentWithdrawal.mined,
      );
      expect(
        ownPaymentWithdrawalRefusal(
          payment(),
          me: 'me',
          state: TransactionState.waiting,
        ),
        OwnPaymentWithdrawal.waiting,
      );
    });

    test('held when the history could not be read', () {
      // §14.4: a failed read is not "absent". Taking it for that would offer
      // the debt again while the payment may still land.
      expect(
        ownPaymentWithdrawalRefusal(
          payment(),
          me: 'me',
          state: TransactionState.unread,
        ),
        OwnPaymentWithdrawal.unread,
      );
    });

    test('free once it expired, or when the history does not hold it', () {
      expect(
        ownPaymentWithdrawalRefusal(
          payment(),
          me: 'me',
          state: TransactionState.expired,
        ),
        isNull,
      );
      expect(
        ownPaymentWithdrawalRefusal(payment(), me: 'me', state: null),
        isNull,
      );
    });

    test('cash, swaps, another payer\'s record and a record with no '
        'transaction are not this rule\'s', () {
      for (final p in [
        payment(method: 'cash'),
        payment(method: 'swap'),
        payment(from: 'ben'),
        payment(reference: null),
      ]) {
        expect(
          ownPaymentWithdrawalRefusal(
            p,
            me: 'me',
            state: TransactionState.mined,
          ),
          isNull,
        );
      }
    });
  });

  group('§14.3: clearing a note that names its transaction', () {
    final named = _send().sentAs('ab' * 32);

    test(
      'refused while the wallet may still send it, or once it went through',
      () {
        expect(
          namedSendRefusal(named, state: TransactionState.waiting),
          NamedSendRefusal.waiting,
        );
        expect(
          namedSendRefusal(named, state: TransactionState.mined),
          NamedSendRefusal.mined,
        );
      },
    );

    test('allowed once it expired, or the history does not hold it', () {
      expect(namedSendRefusal(named, state: TransactionState.expired), isNull);
      // A history that could not be read keeps the note: clearing it on a
      // failed read sends the debt twice.
      expect(
        namedSendRefusal(named, state: TransactionState.unread),
        NamedSendRefusal.unread,
      );
      expect(namedSendRefusal(named, state: null), isNull);
    });

    test('a note naming none is the other rule\'s', () {
      expect(
        namedSendRefusal(_send(), state: TransactionState.waiting),
        isNull,
      );
    });
  });
}
