// A foreign key is discarded only while it is still the one held (§9.4).
import 'dart:async';
import 'dart:math';

import 'package:splitz_core/host.dart' as splitz;
import 'package:test/test.dart';
import 'package:splitz_host/splitz_host.dart';

import 'support/fake_wallet.dart';

/// Holds the [holdAt]th read of a key, after reading it, until [gate] opens.
class GatedSecrets implements SecretStore {
  final inner = InMemorySecretStore();
  int reads = 0;
  int? holdAt;
  final gate = Completer<void>();
  final reached = Completer<void>();
  @override
  Future<String?> read(String key) async {
    final v = await inner.read(key);
    if (++reads == holdAt) {
      reached.complete();
      await gate.future;
    }
    return v;
  }

  @override
  Future<void> write(String key, String value) => inner.write(key, value);
  @override
  Future<void> delete(String key) => inner.delete(key);
}

class Device {
  Device(this.wallet, SplitsRelay relay, this.secrets)
    : store = BillStore(InMemoryBillStorage()),
      keys = SplitsKeys(store: secrets, random: Random(9)) {
    sync = SplitsSync(store: store, keys: keys, relay: relay);
    host = WalletBillHost(wallet);
  }
  final FakeWallet wallet;
  final SecretStore secrets;
  final BillStore store;
  final SplitsKeys keys;
  late final SplitsSync sync;
  late final WalletBillHost host;
}

void main() {
  Future<
    ({InMemorySplitsRelay relay, String billId, String key, String foreign})
  >
  sealedForOthers() async {
    final relay = InMemorySplitsRelay();
    final ana = Device(FakeWallet(), relay, GatedSecrets());
    final key = ana.keys.generateKey();
    final create = splitz.createBill(
      host: ana.host,
      name: 'Dinner',
      currency: 'EUR',
      creatorKey: 'A' * 43,
      billKey: key,
    );
    final billId = create['id'] as String;
    await ana.keys.storeBillKey(billId, key);
    ana.wallet.tick();
    final entries = [
      create,
      splitz.joinBill(host: ana.host, name: 'Ana', payTo: 'u1ana'),
    ];
    await ana.store.merge(billId, entries);
    await ana.sync.push(billId);
    final foreign = ana.keys.generateKey();
    final sealing = SplitsSealing();
    await relay.push(SplitsChannel.forBill(billId), [
      for (final e in entries) await sealing.seal(e, foreign),
    ]);
    return (relay: relay, billId: billId, key: key, foreign: foreign);
  }

  for (final race in [false, true]) {
    test('the real key chosen while the foreign one is being discarded, '
        'race=$race', () async {
      final b = await sealedForOthers();
      final secrets = GatedSecrets();
      final vic = Device(
        FakeWallet(id: 'vic', payTo: 'u1vic'),
        b.relay,
        secrets,
      );
      await vic.keys.storeBillKey(b.billId, b.foreign);
      secrets.reads = 0;
      // read 1: _requireKey; read 2: _refuseForeignKey's `held`.
      secrets.holdAt = race ? 2 : null;
      final pull = vic.sync
          .pull(b.billId)
          .then<Object?>((_) => null, onError: (Object e) => e);
      if (race) {
        await secrets.reached.future;
        // The person takes the bill's real invite while the discard is
        // between its read and its delete.
        final replaced = vic.keys.replaceBillKey(b.billId, b.key);
        secrets.gate.complete();
        await replaced;
      }
      final err = await pull;
      if (!race) await vic.keys.replaceBillKey(b.billId, b.key);
      final held = await vic.keys.readBillKey(b.billId);
      expect((err as SplitsSyncException?)?.kind, SyncFailure.foreignKey);
      expect(held, b.key, reason: 'a key chosen since the sync began stays');
    });
  }
}
