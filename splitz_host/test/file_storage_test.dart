@TestOn('vm')
library;

import 'dart:io';
import 'dart:math';

import 'package:test/test.dart';
import 'package:splitz_host/io.dart';
import 'package:splitz_core/host.dart' as splitz;
import 'package:splitz_host/splitz_host.dart';

import 'support/fake_wallet.dart';

void main() {
  late Directory dir;

  setUp(() async {
    dir = await Directory.systemTemp.createTemp('splitz-store');
  });

  tearDown(() async {
    if (await dir.exists()) await dir.delete(recursive: true);
  });

  test('a bill written here is still here after a restart', () async {
    final wallet = FakeWallet();
    final host = WalletBillHost(wallet);
    final secrets = FileSecretStore(File('${dir.path}/secrets.json'));
    late String id;

    // One run of the app.
    {
      final store = BillStore(FileBillStorage(dir));
      final keys = SplitsKeys(store: secrets, random: Random(1));
      final create = splitz.createBill(
        host: host,
        name: 'Dinner',
        currency: 'EUR',
        creatorKey: 'A' * 43,
      );
      id = create['id'] as String;
      await keys.ensureBillKey(id);
      await store.merge(id, [create]);
      wallet.tick();
      await store.merge(id, [
        splitz.joinBill(host: host, name: 'Ana', payTo: 'u1ana'),
      ]);
      wallet.tick();
      await store.merge(id, [
        splitz.addExpense(
          host: host,
          expenseId: 'x1',
          paidBy: 'ana',
          amount: 9000,
          split: const {
            'type': 'equal',
            'among': ['ana'],
          },
          description: 'Pizza',
        ),
      ]);
    }

    // The next one: a new store and keychain over the same directory, as a
    // cold start is.
    final store = BillStore(FileBillStorage(dir));
    final keys = SplitsKeys(
      store: FileSecretStore(File('${dir.path}/secrets.json')),
    );

    expect(await store.billIds(), [id]);
    final folded = foldUnverified(wallet, await store.read(id), billId: id);
    expect(folded.bill.expenses.single.description, 'Pizza');
    // And the bill's key came back, so its contents can still be sealed.
    expect(await keys.readBillKey(id), isNotNull);
  });

  test(
    'the identity comes back, so other devices still recognise this one',
    () async {
      final secrets = FileSecretStore(File('${dir.path}/secrets.json'));
      final keys = SplitsKeys(store: secrets, random: Random(1));
      const account = WalletAccount(id: 'ana', identitySecret: [1, 2, 3]);

      final first = await keys.ensureIdentitySeed(account);
      final afterRestart = await SplitsKeys(
        store: secrets,
      ).ensureIdentitySeed(account);
      expect(afterRestart, first);
    },
  );

  test('a write that does not finish leaves the old contents', () async {
    final storage = FileBillStorage(dir);
    await storage.write('splitz_bill_b1', 'first');

    // What a process dying mid-write leaves behind: a temporary beside the
    // target, and the target untouched.
    await File(
      '${dir.path}/splitz_bill_b1.writing',
    ).writeAsString('half a log');

    expect(await storage.read('splitz_bill_b1'), 'first');
    // The list is bound first because a collection literal passed straight to
    // `expect` is wrapped differently by two formatter versions, and this
    // package is formatted by whichever one a checkout has.
    final listed = await storage.keys('splitz_bill_');
    expect(listed, [
      'splitz_bill_b1',
    ], reason: 'a half-written file is not a bill');

    expect(await storage.sweepUnfinishedWrites(), 1);
    expect(await storage.sweepUnfinishedWrites(), 0);
  });

  test(
    'a key that is not a plain name cannot write outside the directory',
    () async {
      final storage = FileBillStorage(dir);
      await storage.write('splitz_bill_../escaped', 'x');

      final outside = File('${dir.parent.path}/escaped');
      expect(await outside.exists(), isFalse);
      expect(await storage.read('splitz_bill_../escaped'), 'x');
    },
  );

  test(
    'an empty or missing directory lists nothing rather than raising',
    () async {
      final absent = FileBillStorage(Directory('${dir.path}/never-made'));
      expect(await absent.keys('splitz_bill_'), isEmpty);
      expect(await absent.read('splitz_bill_b1'), isNull);
      await absent.delete('splitz_bill_b1');
    },
  );

  test('a secrets file that is not JSON reads as no secrets', () async {
    final file = File('${dir.path}/secrets.json')..writeAsStringSync('{{{');
    final store = FileSecretStore(file);
    expect(await store.read('anything'), isNull);
    // And writing recovers it rather than failing forever.
    await store.write('k', 'v');
    expect(await store.read('k'), 'v');
  });

  test('the sweep clears what an interrupted write left', () async {
    final storage = FileBillStorage(dir);
    await storage.write('splitz_bill_b1', 'x');
    await File('${dir.path}/splitz_bill_b1.writing').writeAsString('torn');

    // Through `BillStore`, which is what a host calls once when it loads.
    expect(await BillStore(storage).sweepUnfinishedWrites(), 1);

    expect(await File('${dir.path}/splitz_bill_b1.writing').exists(), isFalse);
    expect(await storage.read('splitz_bill_b1'), 'x');
  });

  test('a bill file that cannot be read is never written over', () async {
    // Unreadable is not absent. A merge that took it for an empty log would
    // write the relay's copy over it and lose every entry only this device
    // held — a payment recorded and not yet pushed.
    final storage = FileBillStorage(dir);
    final store = BillStore(storage);
    final wallet = FakeWallet();
    final create = splitz.createBill(
      host: WalletBillHost(wallet),
      name: 'Dinner',
      currency: 'EUR',
      creatorKey: 'A' * 43,
    );
    final id = create['id'] as String;
    await store.merge(id, [create]);
    final file = File('${dir.path}/splitz_bill_$id');
    final before = await file.readAsString();

    await Process.run('chmod', ['000', file.path]);
    try {
      expect(await store.read(id), isEmpty, reason: 'shown as no entries');
      wallet.tick();
      await expectLater(
        store.merge(id, [
          splitz.joinBill(host: WalletBillHost(wallet), name: 'Ana'),
        ]),
        throwsA(isA<BillStorageUnreadable>()),
      );
    } finally {
      await Process.run('chmod', ['600', file.path]);
    }
    expect(await file.readAsString(), before, reason: 'not written over');
  });

  test('a sweep during a write leaves that write to finish', () async {
    // The sweep runs on every load, not only the first, so it meets writes in
    // flight. Deleting their temporary file fails the rename and loses the
    // entry: a payment sent and its record gone.
    final storage = FileBillStorage(dir);
    // What an earlier process left: its temporary names another process.
    await File(
      '${dir.path}/splitz_bill_b0~1-2~3-4.writing',
    ).writeAsString('left by a process that died');
    var failed = 0;
    for (var i = 0; i < 40; i++) {
      final writing = storage.write('splitz_bill_b$i', 'x' * 200000);
      await storage.sweepUnfinishedWrites();
      try {
        await writing;
      } on FileSystemException {
        failed++;
      }
    }
    expect(failed, 0, reason: 'no write lost its temporary file to a sweep');
    expect(
      await File('${dir.path}/splitz_bill_b0~1-2~3-4.writing').exists(),
      isFalse,
      reason: "another process's leftover is still swept",
    );
    expect((await storage.keys('splitz_bill_')).length, 40);
  });

  test('deleting a bill takes its file with it', () async {
    final storage = FileBillStorage(dir);
    await storage.write('splitz_bill_b1', 'x');
    await storage.delete('splitz_bill_b1');
    expect(await storage.keys('splitz_bill_'), isEmpty);
    expect(await storage.read('splitz_bill_b1'), isNull);
  });
}
