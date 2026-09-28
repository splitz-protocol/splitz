import 'dart:math';

import 'package:test/test.dart';
import 'package:splitz_core/host.dart' as splitz;
import 'package:splitz_host/splitz_host.dart';

import 'support/fake_wallet.dart';

/// One device: its own storage and keychain, sharing a relay with the others.
class Device {
  Device(this.wallet, SplitsRelay relay, {int seed = 1})
    : store = BillStore(InMemoryBillStorage()),
      keys = SplitsKeys(store: InMemorySecretStore(), random: Random(seed)) {
    sync = SplitsSync(store: store, keys: keys, relay: relay);
    host = WalletBillHost(wallet);
  }

  final FakeWallet wallet;
  final BillStore store;
  final SplitsKeys keys;
  late final SplitsSync sync;
  late final WalletBillHost host;
}

void main() {
  test('two devices that sync the same bill hold the same entries', () async {
    final relay = InMemorySplitsRelay();
    final ana = Device(FakeWallet(), relay, seed: 1);
    final ben = Device(FakeWallet(id: 'ben', payTo: 'u1ben'), relay, seed: 2);

    final key = await ana.keys.ensureBillKey('placeholder');
    final create = splitz.createBill(
      host: ana.host,
      name: 'Dinner',
      currency: 'EUR',
      creatorKey: 'A' * 43,
    );
    final billId = create['id'] as String;
    await ana.keys.storeBillKey(billId, key);
    await ana.store.merge(billId, [create]);

    ana.wallet.tick();
    await ana.store.merge(billId, [
      splitz.joinBill(host: ana.host, name: 'Ana', payTo: 'u1ana'),
    ]);

    // Ben scanned the invite, so he holds the key and the bill id.
    await ben.keys.storeBillKey(billId, key);

    await ana.sync.sync(billId);
    final pulled = await ben.sync.pull(billId);
    expect(pulled.entries.length, 2);
    expect(pulled.unopenable, 0);

    // Ben joins, pushes, and ana catches up.
    ben.wallet.tick();
    ben.wallet.tick();
    await ben.store.merge(billId, [
      splitz.joinBill(host: ben.host, name: 'Ben', payTo: 'u1ben'),
    ]);
    await ben.sync.push(billId);
    final back = await ana.sync.pull(billId);

    expect(back.entries.length, 3);
    expect(
      await ana.store.read(billId),
      await ben.store.read(billId),
      reason: '§10.2 merges by set union, so order cannot separate them',
    );

    final folded = foldUnverified(ana.wallet, back.entries, billId: billId);
    expect(folded.bill.participants.map((p) => p.id), ['ana', 'ben']);
    expect(folded.setAside, isEmpty);
  });

  test('syncing twice changes nothing', () async {
    final relay = InMemorySplitsRelay();
    final ana = Device(FakeWallet(), relay);
    final create = splitz.createBill(
      host: ana.host,
      name: 'Dinner',
      currency: 'EUR',
      creatorKey: 'A' * 43,
    );
    final billId = create['id'] as String;
    await ana.keys.storeBillKey(billId, ana.keys.generateKey());
    await ana.store.merge(billId, [create]);

    final first = await ana.sync.sync(billId);
    final second = await ana.sync.sync(billId);
    expect(second.entries, first.entries);
    expect(
      await relay.fetch(SplitsChannel.forBill(billId)),
      hasLength(1),
      reason: 'a blob is keyed by its content, so re-pushing stores one copy',
    );
  });

  test('a blob from another bill is skipped, not fatal', () async {
    final relay = InMemorySplitsRelay();
    final ana = Device(FakeWallet(), relay);
    final create = splitz.createBill(
      host: ana.host,
      name: 'Dinner',
      currency: 'EUR',
      creatorKey: 'A' * 43,
    );
    final billId = create['id'] as String;
    await ana.keys.storeBillKey(billId, ana.keys.generateKey());
    await ana.store.merge(billId, [create]);
    await ana.sync.push(billId);

    // Somebody else's blob lands in this channel: same relay, different key.
    final stranger = SplitsSealing();
    final theirKey = SplitsKeys(store: InMemorySecretStore()).generateKey();
    await relay.push(SplitsChannel.forBill(billId), [
      await stranger.seal(<String, dynamic>{
        'v': 1,
        'kind': 'joinBill',
      }, theirKey),
    ]);

    final pulled = await ana.sync.pull(billId);
    expect(pulled.unopenable, 1, reason: 'counted, so a wrong key is visible');
    expect(pulled.entries.length, 1, reason: 'the bill itself is unaffected');
  });

  test('the channel is the bill id\'s hash, never the id', () {
    final channel = SplitsChannel.forBill('b1');
    expect(channel, isNot(contains('b1')));
    expect(channel.length, 64, reason: 'SHA-256, hex encoded');
    expect(SplitsChannel.forBill('b1'), channel);
    expect(SplitsChannel.forBill('b2'), isNot(channel));
  });

  test('a bill with no key cannot be synced, and says so', () async {
    final ana = Device(FakeWallet(), InMemorySplitsRelay());
    await expectLater(
      () => ana.sync.pull('b-unknown'),
      throwsA(isA<SplitsSyncException>()),
    );
  });

  test(
    'a build with no relay fails loudly rather than doing nothing',
    () async {
      final ana = Device(FakeWallet(), const UnconfiguredSplitsRelay());
      final create = splitz.createBill(
        host: ana.host,
        name: 'Dinner',
        currency: 'EUR',
        creatorKey: 'A' * 43,
      );
      final billId = create['id'] as String;
      await ana.keys.storeBillKey(billId, ana.keys.generateKey());
      await ana.store.merge(billId, [create]);

      await expectLater(
        () => ana.sync.push(billId),
        throwsA(
          isA<SplitsRelayException>().having(
            (e) => e.isTransient,
            'isTransient',
            isFalse,
          ),
        ),
      );
    },
  );

  test(
    'pushing signs nothing, so a peer cannot borrow this device\'s key',
    () async {
      // A peer pushes an unsigned entry written in ana's name — a confirmation
      // of a payment she never received. Pull stores what opens, as it must;
      // §10.7 sets the entry aside at fold time because ana's key did not sign
      // it. A push that signed every unsigned entry authored as ana would sign
      // it here, with ana's key, and the next fold would apply it.
      final relay = InMemorySplitsRelay();
      final ana = Device(FakeWallet(), relay);
      final create = splitz.createBill(
        host: ana.host,
        name: 'Dinner',
        currency: 'EUR',
        creatorKey: 'A' * 43,
      );
      final billId = create['id'] as String;
      await ana.keys.storeBillKey(billId, ana.keys.generateKey());
      final forged = splitz.confirmPayment(
        host: ana.host,
        paymentId: 'y1',
        method: 'recipientConfirmed',
        record: 'r',
      );
      expect(forged.containsKey('sig'), isFalse);
      await ana.store.merge(billId, [create, forged]);

      await ana.sync.push(billId);

      final key = (await ana.keys.readBillKey(billId))!;
      final sealing = SplitsSealing();
      final onTheWire = [
        for (final blob in await relay.fetch(SplitsChannel.forBill(billId)))
          await sealing.open(blob, key),
      ];
      expect(onTheWire, hasLength(2));
      expect(
        onTheWire.where((e) => e['sig'] != null),
        isEmpty,
        reason: 'push sends what the store holds, and signs none of it',
      );
    },
  );

  test('a peer\'s entry is merged without judging who wrote it', () async {
    // Authorship at arrival would make the stored log depend on network order.
    // §10.7 decides it at fold time over the whole log instead.
    final relay = InMemorySplitsRelay();
    final ana = Device(FakeWallet(), relay);
    final create = splitz.createBill(
      host: ana.host,
      name: 'Dinner',
      currency: 'EUR',
      creatorKey: 'A' * 43,
    );
    final billId = create['id'] as String;
    final key = ana.keys.generateKey();
    await ana.keys.storeBillKey(billId, key);
    await ana.store.merge(billId, [create]);

    // An entry authored by somebody who has not joined yet.
    final strangerWallet = FakeWallet(id: 'zzz');
    strangerWallet.tick();
    final theirs = splitz.joinBill(
      host: WalletBillHost(strangerWallet),
      name: 'Zoe',
      payTo: 'u1zoe',
    );
    await relay.push(SplitsChannel.forBill(billId), [
      await SplitsSealing().seal(theirs, key),
    ]);

    final pulled = await ana.sync.pull(billId);
    expect(pulled.entries.length, 2, reason: 'merged, not filtered');
    expect(pulled.refused, isEmpty);
  });
}
