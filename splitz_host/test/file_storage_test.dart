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
    final folded = foldUnverified(wallet, await store.read(id));
    expect(folded.bill.expenses.single.description, 'Pizza');
    // And the bill's key came back, so its contents can still be sealed.
    expect(await keys.readBillKey(id), isNotNull);
  });

  test(
    'the identity comes back, so other devices still recognise this one',
    () async {
      final secrets = FileSecretStore(File('${dir.path}/secrets.json'));
      final keys = SplitsKeys(store: secrets, random: Random(1));
      const account = WalletAccount(id: 'ana', viewingKey: 'uview1abc');

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

  test('deleting a bill takes its file with it', () async {
    final storage = FileBillStorage(dir);
    await storage.write('splitz_bill_b1', 'x');
    await storage.delete('splitz_bill_b1');
    expect(await storage.keys('splitz_bill_'), isEmpty);
    expect(await storage.read('splitz_bill_b1'), isNull);
  });
}
