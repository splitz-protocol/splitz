import 'dart:async';
import 'dart:io';

import 'package:splitz_core/host.dart' as splitz;
import 'package:splitz_host/io.dart';
import 'package:splitz_host/splitz_host.dart';
import 'package:test/test.dart';

import 'support/fake_wallet.dart';

/// Storage whose reads take a turn of the event loop, so two merges can
/// interleave the way a sync and a settle do on a phone.
class _SlowStorage extends InMemoryBillStorage {
  @override
  Future<String?> read(String key) async {
    // The value is taken first and handed back a turn later: the window in
    // which another writer can change what this reader already holds.
    final value = await super.read(key);
    await Future<void>.delayed(const Duration(milliseconds: 5));
    return value;
  }
}

/// A relay whose fetch answers only when the test says so.
class _HeldRelay implements SplitsRelay {
  final InMemorySplitsRelay inner = InMemorySplitsRelay();
  final Completer<void> release = Completer<void>();
  final Completer<void> fetching = Completer<void>();

  @override
  Future<void> push(String channel, List<String> blobs) =>
      inner.push(channel, blobs);

  @override
  Future<List<String>> fetch(String channel) async {
    fetching.complete();
    await release.future;
    return inner.fetch(channel);
  }
}

void main() {
  final wallet = FakeWallet();
  final host = WalletBillHost(wallet);

  test('two merges into one bill at once keep both', () async {
    final store = BillStore(_SlowStorage());
    final create = splitz.createBill(
      host: host,
      name: 'Dinner',
      currency: 'EUR',
      creatorKey: 'A' * 43,
    );
    final id = create['id'] as String;
    await store.merge(id, [create]);
    wallet.tick();
    final a = splitz.joinBill(host: host, name: 'Ana', payTo: 'u1ana');
    wallet.tick();
    final b = splitz.joinBill(
      host: WalletBillHost(FakeWallet(id: 'ben')),
      name: 'Ben',
      payTo: 'u1ben',
    );

    // A sync and a settle, both reading the log before either writes.
    await Future.wait([
      store.merge(id, [a]),
      store.merge(id, [b]),
    ]);

    final ids = [for (final e in await store.read(id)) e['id']];
    expect(ids, containsAll([create['id'], a['id'], b['id']]));
  });

  test('two stores over one directory keep each other\'s entries', () async {
    // A screen opened twice builds two stores over the same files. Their
    // merges into one bill must queue behind each other, not only behind
    // their own.
    final dir = await Directory.systemTemp.createTemp('twostores');
    addTearDown(() => dir.delete(recursive: true));
    final create = splitz.createBill(
      host: host,
      name: 'Dinner',
      currency: 'EUR',
      creatorKey: 'A' * 43,
    );
    final id = create['id'] as String;
    // Asked for twice, as two screens would.
    final first = BillStore(FileBillStorage(dir));
    final second = BillStore(FileBillStorage(Directory(dir.path)));
    await first.merge(id, [create]);
    wallet.tick();
    final a = splitz.joinBill(host: host, name: 'Ana', payTo: 'u1ana');
    final b = splitz.joinBill(
      host: WalletBillHost(FakeWallet(id: 'ben')),
      name: 'Ben',
      payTo: 'u1ben',
    );

    await Future.wait([
      first.merge(id, [a]),
      second.merge(id, [b]),
    ]);

    final ids = [for (final e in await first.read(id)) e['id']];
    expect(ids, containsAll([create['id'], a['id'], b['id']]));
  });

  test('a bill forgotten while a sync fetches is not written back', () async {
    final storage = InMemoryBillStorage();
    final store = BillStore(storage);
    final keys = SplitsKeys(store: InMemorySecretStore());
    final relay = _HeldRelay();
    final sync = SplitsSync(store: store, keys: keys, relay: relay);

    final create = splitz.createBill(
      host: host,
      name: 'Dinner',
      currency: 'EUR',
      creatorKey: 'A' * 43,
    );
    final id = create['id'] as String;
    await keys.ensureBillKey(id);
    await store.merge(id, [create]);
    await sync.push(id);

    final pulling = sync.pull(id);
    await relay.fetching.future;
    // The person removes the bill while the fetch is in flight: the key
    // first, then the log.
    await keys.forgetBill(id);
    await store.forget(id);
    relay.release.complete();

    await expectLater(pulling, throwsA(isA<SplitsSyncException>()));
    expect(
      await store.billIds(),
      isEmpty,
      reason: 'the bill is not written back without its key',
    );
  });

  test('a different key for a bill already held is refused', () async {
    final keys = SplitsKeys(store: InMemorySecretStore());
    final held = await keys.ensureBillKey('b1');
    final other = keys.generateKey();
    await expectLater(
      keys.storeBillKey('b1', other),
      throwsA(isA<BillKeyConflict>()),
    );
    expect(await keys.readBillKey('b1'), held);
    // The same key again is not a conflict.
    await keys.storeBillKey('b1', held);
  });

  test('a stored entry that is not an entry is skipped, not raised', () async {
    final storage = InMemoryBillStorage();
    await storage.write('splitz_bill_b1', '[{"kind":"joinBill"},{"kind":"x"}]');
    expect(await BillStore(storage).read('b1'), isEmpty);
  });

  group('file storage', () {
    late Directory dir;
    setUp(() async => dir = await Directory.systemTemp.createTemp('store'));
    tearDown(() => dir.delete(recursive: true));

    test('a key with a slash is listed as it was stored', () async {
      final files = FileBillStorage(dir);
      const watch = SwapWatch(
        billId: 'b1',
        reference: 'ref/1%',
        to: 'ben',
        depositAddress: '0xdeposit',
        assetSymbol: 'USDC',
        assetChain: 'base',
      );
      await SwapWatchList(files).add(watch);
      final held = await SwapWatchList(files).held();
      expect(held.single.reference, 'ref/1%');
      expect(await files.keys('swapwatch/'), ['swapwatch/ref%2F1%25']);
    });

    test('writes to one key at once never share a temporary file', () async {
      final files = FileBillStorage(dir);
      await Future.wait([for (var i = 0; i < 20; i++) files.write('k', 'v$i')]);
      expect(await files.read('k'), startsWith('v'));
      final names = [await for (final e in dir.list()) e.uri.pathSegments.last];
      expect(names, ['k'], reason: 'nothing left half-written');
    });

    test('a temporary left by an older version is not listed', () async {
      await File('${dir.path}/splitz_bill_x.writing').writeAsString('[');
      final files = FileBillStorage(dir);
      expect(await files.keys('splitz_bill_'), isEmpty);
      expect(await files.sweepUnfinishedWrites(), 1);
    });
  });
}
