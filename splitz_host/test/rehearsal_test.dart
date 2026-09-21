@TestOn('vm')
library;

import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:splitz_core/host.dart' as splitz;
import 'package:splitz_core/splitz_core.dart' show billToJson, canonicalJson;
import 'package:splitz_host/splitz_host.dart';
import 'package:test/test.dart';

/// Four devices, one bill, one relay.
///
/// Every other test here holds one log in one process. This holds four, each
/// with its own storage and its own keychain, sharing nothing but a socket to
/// `tools/relay/server.py`. That is what makes it a rehearsal rather than a
/// simulation: a device learns what the others did only by syncing, so an
/// entry that never reached the relay is an entry the others never see.
///
/// What it asserts is §10.2's claim — that devices seeing different subsets of
/// the log materialise the same bill and the same money.
Future<({Process process, Uri origin})> _relay() async {
  final port = 39500 + DateTime.now().microsecond % 2000;
  final process = await Process.start('python3', [
    '../tools/relay/server.py',
    '--port',
    '$port',
  ]);
  final client = HttpClient();
  for (var i = 0; i < 100; i++) {
    try {
      final request = await client.getUrl(
        Uri.parse('http://127.0.0.1:$port/c/${'0' * 64}'),
      );
      await (await request.close()).drain<void>();
      return (process: process, origin: Uri.parse('http://127.0.0.1:$port'));
    } on Object {
      await Future<void>.delayed(const Duration(milliseconds: 50));
    }
  }
  process.kill();
  throw StateError('the relay did not come up on $port');
}

SplitsRelay _relayClient(Uri origin) {
  final client = HttpClient();
  return HttpSplitsRelay(
    origin: origin,
    post: (url, body) async {
      final request = await client.postUrl(url);
      request.headers.contentType = ContentType.json;
      request.write(body);
      return (await request.close()).transform(utf8.decoder).join();
    },
    get: (url) async {
      final request = await client.getUrl(url);
      return (await request.close()).transform(utf8.decoder).join();
    },
  );
}

/// One participant's phone: its own log, its own keys, its own clock.
class Device implements splitz.BillHost {
  Device(this.me, Uri relayOrigin)
    : store = BillStore(InMemoryBillStorage()),
      keys = SplitsKeys(store: InMemorySecretStore()) {
    sync = SplitsSync(
      store: store,
      keys: keys,
      relay: _relayClient(relayOrigin),
    );
  }

  @override
  final String me;
  final BillStore store;
  final SplitsKeys keys;
  late final SplitsSync sync;
  int _tick = 0;

  @override
  String? get payToAddress => 'u1$me';
  @override
  splitz.Clock get now =>
      () => DateTime.utc(2026, 10, 28, 19, 0).add(Duration(minutes: _tick++));
  @override
  splitz.Randomness get randomBytes =>
      (n) => Uint8List.fromList(
        List<int>.generate(n, (i) => i + me.codeUnitAt(0)),
      );
  @override
  splitz.Broadcast get broadcast =>
      (uri) async => throw StateError('a rehearsal sends no money');
  @override
  splitz.SignEntry? get sign => null;
  @override
  splitz.VerifyEntry? get verify => null;

  /// Writes an entry this device authored into its own log.
  Future<void> write(String billId, Map<String, dynamic> entry) =>
      store.merge(billId, [entry]);

  Future<splitz.FoldedBill> fold(String billId) async =>
      splitz.BillLog(this, entries: await store.read(billId)).fold();
}

void main() {
  late Process relay;
  late Uri origin;

  setUpAll(() async {
    final up = await _relay();
    relay = up.process;
    origin = up.origin;
  });
  tearDownAll(() => relay.kill());

  test('four devices, different subsets, one bill and one plan', () async {
    final ana = Device('ana', origin);
    final ben = Device('ben', origin);
    final cai = Device('cai', origin);
    final dee = Device('dee', origin);

    // --- create: Ana opens the bill and is the only one who has it ---------
    final create = splitz.createBill(
      host: ana,
      name: 'Dinner',
      currency: 'USD',
      creatorKey: 'A' * 43,
    );
    final billId = create['id'] as String;
    await ana.write(billId, create);
    await ana.write(
      billId,
      splitz.joinBill(host: ana, name: 'Ana', payTo: 'u1ana'),
    );

    // The key travels with the invite, not over the relay.
    final billKey = await ana.keys.ensureBillKey(billId);
    await ana.sync.sync(billId);

    expect(
      (await ben.store.read(billId)),
      isEmpty,
      reason: 'a device that has not synced holds nothing',
    );

    // --- join: each joiner takes the key, pulls, and claims its own row ----
    for (final who in [ben, cai, dee]) {
      await who.keys.storeBillKey(billId, billKey);
      await who.sync.pull(billId);
      final seen = await who.store.read(billId);
      expect(seen, isNotEmpty, reason: '${who.me} pulled the bill Ana pushed');
      await who.write(
        billId,
        splitz.joinBill(host: who, name: who.me, payTo: 'u1${who.me}'),
      );
      await who.sync.sync(billId);
    }

    // Ana has not synced since, so she has not seen any of them yet.
    expect(
      (await ana.fold(billId)).bill.participants.length,
      1,
      reason: 'a device learns what it pulled, not what happened',
    );
    await ana.sync.pull(billId);
    expect((await ana.fold(billId)).bill.participants.length, 4);

    // --- the bill: each joiner covers one course, split with Ana ----------
    for (final who in [ben, cai, dee]) {
      await who.sync.pull(billId);
      await who.write(
        billId,
        splitz.addExpense(
          host: who,
          expenseId: 'x-${who.me}',
          paidBy: who.me,
          amount: 60,
          split: {
            'type': 'equal',
            'among': ['ana', who.me]..sort(),
          },
        ),
      );
      await who.sync.sync(billId);
    }
    await ana.write(
      billId,
      splitz.setRate(host: ana, currency: 'USD', minorUnitsPerZec: 100000),
    );
    await ana.sync.sync(billId);
    for (final who in [ben, cai, dee]) {
      await who.sync.pull(billId);
    }

    // --- convergence: §10.2's claim, over four logs that arrived differently
    final folds = <String, splitz.FoldedBill>{};
    for (final who in [ana, ben, cai, dee]) {
      folds[who.me] = await who.fold(billId);
    }
    final reference = canonicalJson(billToJson(folds['ana']!.bill));
    for (final who in ['ben', 'cai', 'dee']) {
      expect(
        canonicalJson(billToJson(folds[who]!.bill)),
        reference,
        reason: '$who materialised a different bill from the same log',
      );
    }
    for (final f in folds.values) {
      expect(f.setAside, isEmpty);
    }

    // --- the money: every device agrees who owes whom ---------------------
    final owed = splitz.obligationFor(ana, folds['ana']!)!;
    expect(owed.settlements.length, 3, reason: 'Ana owes each of the three');
    expect(owed.settlements.every((s) => s.amount == 30), isTrue);
    for (final who in [ben, cai, dee]) {
      final theirs = splitz.obligationFor(who, folds[who.me]!);
      expect(
        theirs?.settlements ?? const [],
        isEmpty,
        reason: '${who.me} is owed, so ${who.me} pays nobody',
      );
    }
  });
}
