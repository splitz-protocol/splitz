import 'package:test/test.dart';
import 'package:splitz_core/host.dart' as splitz;
import 'package:splitz_core/splitz_core.dart' as protocol;
import 'package:splitz_host/splitz_host.dart';

import 'support/fake_wallet.dart';

void main() {
  test('a bill opened through the seam passes the protocol\'s own ingress', () {
    final wallet = FakeWallet();
    final host = WalletBillHost(wallet);

    final create = splitz.createBill(
      host: host,
      name: 'Dinner',
      currency: 'EUR',
      creatorKey: fakeKey('ana'),
    );

    // §9.4: the bill's id IS the digest of the entry that opens it.
    expect(create['id'], protocol.deriveBillId(create));
    expect(create['author'], 'ana');
    protocol.checkEntry(create);
  });

  test('the clock and the randomness come from the wallet', () {
    final wallet = FakeWallet();
    final host = WalletBillHost(wallet);

    final first = splitz.joinBill(host: host, name: 'Ana');
    wallet.tick();
    final second = splitz.joinBill(host: host, name: 'Ana');

    expect(first['at'], '2026-10-28T19:30:00.000Z');
    expect(second['at'], isNot(first['at']));

    // Two bills opened at one instant by one person are two bills: §9.4's
    // nonce is the only thing separating them.
    final a = splitz.createBill(
      host: host,
      name: 'D',
      currency: 'EUR',
      creatorKey: fakeKey('ana'),
    );
    final b = splitz.createBill(
      host: host,
      name: 'D',
      currency: 'EUR',
      creatorKey: fakeKey('ana'),
    );
    expect(a['at'], b['at']);
    expect(a['nonce'], isNot(b['nonce']));
    expect(a['id'], isNot(b['id']));
  });

  test('a wallet that does not sign says so rather than supplying nothing', () {
    final host = WalletBillHost(FakeWallet());
    expect(host.sign, isNull);
    expect(host.verify, isNull);
  });

  test(
    'a send that landed carries its transaction id into the record',
    () async {
      final wallet = FakeWallet(
        outcome: const WalletSendOutcome(
          phase: WalletSendPhase.succeeded,
          txid: 'tx-abc',
        ),
      );
      final host = WalletBillHost(wallet);

      final sent = await host.broadcast('zcash:u1ben?amount=0.045');
      expect(sent.result, splitz.SendResult.sent);
      expect(sent.txid, 'tx-abc');
      expect(wallet.sender.sent.single, 'zcash:u1ben?amount=0.045');
    },
  );

  test(
    'a send that was built but not broadcast is neither paid nor unpaid',
    () async {
      final host = WalletBillHost(
        FakeWallet(
          outcome: const WalletSendOutcome(
            phase: WalletSendPhase.pendingBroadcast,
            statusMessage: 'created, not broadcast',
          ),
        ),
      );

      final sent = await host.broadcast('zcash:u1ben?amount=0.045');
      expect(sent.result, splitz.SendResult.pending);
      expect(
        sent.txid,
        isNull,
        reason: 'nothing may be recorded from a send that has not landed',
      );
      expect(sent.detail, 'created, not broadcast');
    },
  );

  test(
    'aborted and failed are one answer to a bill: nothing was spent',
    () async {
      for (final phase in [WalletSendPhase.failed, WalletSendPhase.aborted]) {
        final host = WalletBillHost(
          FakeWallet(
            outcome: WalletSendOutcome(phase: phase, error: 'no funds'),
          ),
        );
        final sent = await host.broadcast('zcash:u1ben?amount=0.045');
        expect(sent.result, splitz.SendResult.failed, reason: '$phase');
        expect(sent.detail, 'no funds');
      }
    },
  );

  test(
    'a send reporting success with no transaction id is a failure',
    () async {
      // `WalletSendOutcome` cannot enforce this — a wallet is free to answer
      // `succeeded` with nothing. Recording a payment whose id is absent would
      // put an entry on the bill that no transaction backs.
      final host = WalletBillHost(
        FakeWallet(
          outcome: const WalletSendOutcome(phase: WalletSendPhase.succeeded),
        ),
      );

      final sent = await host.broadcast('zcash:u1ben?amount=0.045');
      expect(sent.result, splitz.SendResult.failed);
      expect(sent.detail, contains('no transaction id'));
    },
  );

  test('a bill folds, and what it refuses travels with it', () {
    final wallet = FakeWallet();
    final host = WalletBillHost(wallet);

    final log = splitz.BillLog(host)
      ..add([
        splitz.createBill(
          host: host,
          name: 'Dinner',
          currency: 'EUR',
          creatorKey: fakeKey('ana'),
        ),
      ]);
    wallet.tick();
    log.add([splitz.joinBill(host: host, name: 'Ana', payTo: 'u1ana')]);

    final folded = log.fold();
    expect(folded.bill.participants.single.id, 'ana');
    expect(folded.setAside, isEmpty);

    // No rate yet, so there is nothing to price. That is an ordinary bill and
    // not an error: there is no §12 code for unpriced.
    expect(splitz.obligationFor(host, folded), isNull);
  });
}
