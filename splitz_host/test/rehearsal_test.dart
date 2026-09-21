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

  /// The Ed25519 seed this device signs with. A real one lives in a keychain;
  /// what matters here is that no other device has it.
  late final List<int> seed = List<int>.generate(
    32,
    (i) => i + me.codeUnitAt(0),
  );

  Future<String> get identityKey => _signer.publicKeyFromSeed(seed);

  final SplitsSigner _signer = SplitsSigner();

  @override
  splitz.SignEntry? get sign => _signer.signerFor(seed);

  /// Set from the log about to be folded. `prepare` is asynchronous and the
  /// fold is not, so the answers are computed first and handed in.
  splitz.VerifyEntry? _verify;
  @override
  splitz.VerifyEntry? get verify => _verify;

  /// Writes an entry this device authored into its own log, signed.
  ///
  /// Signing here rather than leaving it to `push`: a device that signed only
  /// on the way out could not fold its own log with a verifier until it had
  /// pulled its own entries back, and §10.1 opens no bill from an unsigned
  /// create when a verifier is present.
  Future<void> write(String billId, Map<String, dynamic> entry) async =>
      store.merge(billId, [await splitz.signEntry(host: this, entry: entry)]);

  Future<splitz.FoldedBill> fold(String billId) async {
    final entries = await store.read(billId);
    final prepared = await _signer.prepare(entries);
    _verify = prepared.verify;
    return splitz.BillLog(this, entries: entries).fold();
  }
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

  /// An intruder who has the bill key can push whatever it likes to the
  /// channel — the relay checks nothing and is not asked to. What it cannot do
  /// is be somebody: §10.7 binds a participant to the key that signed for it,
  /// and a second key claiming the same id contests it rather than replacing
  /// it, so the payer is asked before a penny moves.
  test('an intruder with the bill key cannot take a payee\'s payout', () async {
    final ana = Device('ana', origin);
    final ben = Device('ben', origin);
    final mallory = Device('mallory', origin);

    final create = splitz.createBill(
      host: ana,
      name: 'Dinner',
      currency: 'USD',
      creatorKey: await ana.identityKey,
    );
    final billId = create['id'] as String;
    await ana.write(billId, create);
    await ana.write(
      billId,
      splitz.joinBill(
        host: ana,
        name: 'Ana',
        payTo: 'u1ana',
        identityKey: await ana.identityKey,
      ),
    );
    final billKey = await ana.keys.ensureBillKey(billId);
    await ana.sync.sync(billId, signerSeed: ana.seed, authorId: ana.me);

    await ben.keys.storeBillKey(billId, billKey);
    await ben.sync.pull(billId);
    await ben.write(
      billId,
      splitz.joinBill(
        host: ben,
        name: 'ben',
        payTo: 'u1ben',
        identityKey: await ben.identityKey,
      ),
    );
    await ben.write(
      billId,
      splitz.addExpense(
        host: ben,
        expenseId: 'x-ben',
        paidBy: 'ben',
        amount: 60,
        split: const {
          'type': 'equal',
          'among': ['ana', 'ben'],
        },
      ),
    );
    await ben.sync.sync(billId, signerSeed: ben.seed, authorId: ben.me);
    await ana.sync.pull(billId);
    await ana.write(
      billId,
      splitz.setRate(host: ana, currency: 'USD', minorUnitsPerZec: 100000),
    );
    await ana.sync.sync(billId, signerSeed: ana.seed, authorId: ana.me);

    // Ana owes Ben thirty, to Ben's address.
    final honest = await ana.fold(billId);
    final before = splitz.obligationFor(ana, honest)!;
    expect(before.settlements.single.to, 'ben');
    expect(honest.bill.participant('ben')!.payTo, 'u1ben');
    expect(before.contested, isEmpty);

    // Mallory has the key — leaked, shared, or from a device that was lent
    // out — and claims to be Ben, at Mallory's own address.
    await mallory.keys.storeBillKey(billId, billKey);
    await mallory.sync.pull(billId);
    final forged = await splitz.signEntry(
      host: mallory,
      entry: splitz.joinBill(
        host: _Claiming(mallory, 'ben'),
        name: 'ben',
        payTo: 'u1mallory',
        identityKey: await mallory.identityKey,
      ),
    );
    await mallory.store.merge(billId, [forged]);
    await mallory.sync.push(billId);

    await ana.sync.pull(billId);
    final after = await ana.fold(billId);

    // Ben is contested, not replaced: nothing inside the log says which key is
    // the person, and resolving by time would hand the identity to whoever
    // backdates furthest.
    expect(after.identities.contested, contains('ben'));
    expect(after.identities.bound.containsKey('ben'), isFalse);

    final owed = splitz.obligationFor(ana, after);
    expect(
      owed?.settlements ?? const [],
      isEmpty,
      reason: 'a contested payee is held back until the payer is asked',
    );
    expect(owed!.contested.map((c) => c.to), contains('ben'));

    // What the payer is shown before deciding: the contest carries the address
    // standing on the bill, and §13's report names the change. The forged
    // address can win §10.2's ordering — `at` is whatever its author wrote —
    // so the defence is not that the honest address survives. It is that the
    // money stops, and that a person sees whose address it is now.
    expect(
      owed.contested.single.address,
      'u1mallory',
      reason: 'the payer is shown the address they would actually pay',
    );
    expect(
      after.replacedAddresses.map((r) => '${r.id}:${r.from}->${r.to}'),
      contains('ben:u1ben->u1mallory'),
      reason:
          '§13: a wallet MUST put a changed pay-to address in front of '
          'the payer before settling to it',
    );

    // Overriding is the payer's to make, and it pays what the bill shows —
    // which is why the two reports above have to be right.
    final anyway = splitz.obligationFor(ana, after, payAnyway: const {'ben'})!;
    expect(anyway.settlements.single.to, 'ben');
    expect(
      anyway.request.uri,
      contains('u1mallory'),
      reason:
          'a payer who overrides pays the address on the bill; nothing '
          'here silently substitutes a different one',
    );
  });

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
      // The creator names the key it signs with: a create whose signature
      // does not answer for its `creatorKey` opens no bill (§10.1).
      creatorKey: await ana.identityKey,
    );
    final billId = create['id'] as String;
    await ana.write(billId, create);
    await ana.write(
      billId,
      splitz.joinBill(
        host: ana,
        name: 'Ana',
        payTo: 'u1ana',
        identityKey: await ana.identityKey,
      ),
    );

    // The key travels with the invite, not over the relay.
    final billKey = await ana.keys.ensureBillKey(billId);
    await ana.sync.sync(billId, signerSeed: ana.seed, authorId: ana.me);

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
        splitz.joinBill(
          host: who,
          name: who.me,
          payTo: 'u1${who.me}',
          identityKey: await who.identityKey,
        ),
      );
      await who.sync.sync(billId, signerSeed: who.seed, authorId: who.me);
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
      await who.sync.sync(billId, signerSeed: who.seed, authorId: who.me);
    }
    await ana.write(
      billId,
      splitz.setRate(host: ana, currency: 'USD', minorUnitsPerZec: 100000),
    );
    await ana.sync.sync(billId, signerSeed: ana.seed, authorId: ana.me);
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
      // §10.7 over a log every device assembled differently: each participant
      // signed its own entries, so each key is bound and none is contested.
      expect(
        f.identities.bound.keys.toSet(),
        {'ana', 'ben', 'cai', 'dee'},
        reason: 'every participant signed for the key its join named',
      );
      expect(f.identities.contested, isEmpty);
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

/// [device]'s keys and clock, writing entries authored by somebody else.
///
/// The only way to forge: an entry's `author` is whatever its writer put
/// there, and §10.7 is what decides whether the log believes it.
class _Claiming implements splitz.BillHost {
  _Claiming(this._device, this.me);

  final Device _device;
  @override
  final String me;

  @override
  String? get payToAddress => _device.payToAddress;
  @override
  splitz.Clock get now => _device.now;
  @override
  splitz.Randomness get randomBytes => _device.randomBytes;
  @override
  splitz.Broadcast get broadcast => _device.broadcast;
  @override
  splitz.SignEntry? get sign => _device.sign;
  @override
  splitz.VerifyEntry? get verify => null;
}
