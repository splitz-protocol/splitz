@TestOn('vm')
library;

import 'dart:convert';
import 'dart:io';
import 'dart:typed_data';

import 'package:splitz_core/host.dart' as splitz;
import 'package:splitz_core/splitz_core.dart' show billToJson, canonicalJson;
import 'package:splitz_host/splitz_host.dart';
import 'package:test/test.dart';
import 'support/process_port.dart';

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
  final up = await startOnFreePort('../tools/relay/server.py', const []);
  return (
    process: up.process,
    origin: Uri.parse('http://127.0.0.1:${up.port}'),
  );
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
  Device._(this.name, this.me, Uri relayOrigin)
    : store = BillStore(InMemoryBillStorage()),
      keys = SplitsKeys(store: InMemorySecretStore()) {
    sync = SplitsSync(
      store: store,
      keys: keys,
      relay: _relayClient(relayOrigin),
    );
  }

  /// A phone for [name], speaking as the participant id its key derives
  /// (§10.7): a participant who publishes a key is named by it.
  static Future<Device> named(String name, Uri relayOrigin) async {
    final key = await SplitsSigner().publicKeyFromSeed(_seedFor(name));
    return Device._(name, splitz.participantId(key)!, relayOrigin);
  }

  static List<int> _seedFor(String name) =>
      List<int>.generate(32, (i) => i + name.codeUnitAt(0));

  /// What a person calls them; [me] is what the bill calls them.
  final String name;
  @override
  final String me;
  final BillStore store;
  final SplitsKeys keys;
  late final SplitsSync sync;
  int _tick = 0;

  String? get payToAddress => 'u1$name';
  @override
  splitz.Clock get now =>
      () => DateTime.utc(2026, 10, 28, 19, 0).add(Duration(minutes: _tick++));
  @override
  splitz.Randomness get randomBytes =>
      (n) => Uint8List.fromList(
        List<int>.generate(n, (i) => i + name.codeUnitAt(0)),
      );
  @override
  splitz.Broadcast get broadcast =>
      (uri) async => throw StateError('a rehearsal sends no money');

  /// The Ed25519 seed this device signs with. A real one lives in a keychain;
  /// what matters here is that no other device has it.
  late final List<int> seed = _seedFor(name);

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
      store.merge(billId, [
        await splitz.signEntry(host: this, entry: entry, billId: billId),
      ]);

  Future<splitz.FoldedBill> fold(String billId) async {
    final entries = await store.read(billId);
    final prepared = await _signer.prepare(entries, billId: billId);
    _verify = prepared.verify;
    return splitz.BillLog(this, entries: entries).fold();
  }
}

/// Bound rather than written inline: Dart 3.11's formatter and 3.13's wrap
/// a collection literal in an argument list differently, and CI runs 3.13.
const List<splitz.Settlement> noSettlements = [];

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
  /// is be somebody: §10.7 names a participant who publishes a key by the id
  /// that key derives, so a second key's claim to them is refused and the
  /// payer pays the address the participant published.
  test('an intruder with the bill key cannot take a payee\'s payout', () async {
    final ana = await Device.named('ana', origin);
    final ben = await Device.named('ben', origin);
    final mallory = await Device.named('mallory', origin);

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
    await ana.sync.sync(billId);

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
        paidBy: ben.me,
        amount: 60,
        split: {
          'type': 'equal',
          'among': [ana.me, ben.me]..sort(),
        },
      ),
    );
    await ben.sync.sync(billId);
    await ana.sync.pull(billId);
    await ana.write(
      billId,
      splitz.setRate(host: ana, currency: 'USD', minorUnitsPerZec: 100000),
    );
    await ana.sync.sync(billId);

    // Ana owes Ben thirty, to Ben's address.
    final honest = await ana.fold(billId);
    final before = splitz.obligationFor(ana, honest)!;
    expect(before.settlements.single.to, ben.me);
    expect(honest.bill.participant(ben.me)!.payTo, 'u1ben');

    // Mallory has the key — leaked, shared, or from a device that was lent
    // out — and claims to be Ben, at Mallory's own address.
    await mallory.keys.storeBillKey(billId, billKey);
    await mallory.sync.pull(billId);
    final forged = await splitz.signEntry(
      host: mallory,
      entry: splitz.joinBill(
        host: _Claiming(mallory, ben.me),
        name: 'ben',
        payTo: 'u1mallory',
        identityKey: await mallory.identityKey,
      ),
      billId: billId,
    );
    await mallory.store.merge(billId, [forged]);
    await mallory.sync.push(billId);

    await ana.sync.pull(billId);
    final after = await ana.fold(billId);

    // Ben stays bound to Ben's key: Mallory's key does not derive Ben's id,
    // and an entry written as Ben is applied only from a copy Ben's key signed.
    expect(after.identities.bound[ben.me], await ben.identityKey);
    expect(
      after.setAside.map((a) => a.id),
      contains(forged['id']),
      reason: 'the forged claim is refused and reported',
    );
    expect(after.bill.participant(ben.me)!.payTo, 'u1ben');
    expect(
      after.replacedAddresses,
      isEmpty,
      reason: 'no address changed, so there is none to put in front of Ana',
    );

    final owed = splitz.obligationFor(ana, after)!;
    expect(owed.settlements.single.to, ben.me);
    expect(
      owed.request.uri,
      before.request.uri,
      reason: "Ana's request pays the address Ben published",
    );
    expect(owed.request.uri, isNot(contains('u1mallory')));
  });

  test('four devices, different subsets, one bill and one plan', () async {
    final ana = await Device.named('ana', origin);
    final ben = await Device.named('ben', origin);
    final cai = await Device.named('cai', origin);
    final dee = await Device.named('dee', origin);

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
      expect(
        seen,
        isNotEmpty,
        reason: '${who.name} pulled the bill Ana pushed',
      );
      await who.write(
        billId,
        splitz.joinBill(
          host: who,
          name: who.name,
          payTo: 'u1${who.name}',
          identityKey: await who.identityKey,
        ),
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
          expenseId: 'x-${who.name}',
          paidBy: who.me,
          amount: 60,
          split: {
            'type': 'equal',
            'among': [ana.me, who.me]..sort(),
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
      folds[who.name] = await who.fold(billId);
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
      // signed its own entries, so each key is bound.
      final everyone = {ana.me, ben.me, cai.me, dee.me};
      expect(
        f.identities.bound.keys.toSet(),
        everyone,
        reason: 'every participant signed for the key its join named',
      );
    }

    // --- the money: every device agrees who owes whom ---------------------
    final owed = splitz.obligationFor(ana, folds['ana']!)!;
    expect(owed.settlements.length, 3, reason: 'Ana owes each of the three');
    expect(owed.settlements.every((s) => s.amount == 30), isTrue);
    for (final who in [ben, cai, dee]) {
      final theirs = splitz.obligationFor(who, folds[who.name]!);
      expect(
        theirs?.settlements ?? noSettlements,
        isEmpty,
        reason: '${who.name} is owed, so ${who.name} pays nobody',
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
