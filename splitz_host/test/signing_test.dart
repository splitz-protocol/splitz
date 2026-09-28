import 'package:test/test.dart';
import 'package:splitz_core/host.dart' as splitz;
import 'package:splitz_core/splitz_core.dart' as protocol;
import 'package:splitz_host/splitz_host.dart';

import 'support/fake_wallet.dart';

/// A distinct 32-byte Ed25519 seed per person. Fixed, so a failing run can be
/// repeated: §9.4's nonce is what separates two bills, not this.
List<int> seedFor(String who) =>
    List<int>.generate(SplitsSigner.seedBytes, (i) => who.codeUnitAt(0) + i);

/// The bill every entry in this file is signed and verified on (§10.6).
const String _bill = 'signing-test-bill';

void main() {
  final signer = SplitsSigner();

  test(
    'a public key is 32 bytes, which is what §9.4 asks a key to be',
    () async {
      final key = await signer.publicKeyFromSeed(seedFor('ana'));
      expect(SplitsSigner.decode(key).length, 32);
      expect(key.length, 43, reason: '32 bytes is 43 unpadded base64url chars');

      // Long enough to be a creatorKey, which §9.4 refuses at any other length.
      final wallet = FakeWallet();
      final host = WalletBillHost(wallet);
      final create = splitz.createBill(
        host: host,
        name: 'Dinner',
        currency: 'EUR',
        creatorKey: key,
      );
      protocol.checkEntry(create);
    },
  );

  test('signing is deterministic, so a re-pushed entry is one blob', () async {
    final wallet = FakeWallet();
    final seed = seedFor('ana');
    final host = WalletBillHost(wallet, sign: signer.signerFor(seed));

    final entry = splitz.joinBill(host: host, name: 'Ana');
    final once = await splitz.signEntry(
      host: host,
      entry: entry,
      billId: _bill,
    );
    final twice = await splitz.signEntry(
      host: host,
      entry: entry,
      billId: _bill,
    );
    expect(once['sig'], twice['sig']);
    expect(
      once['id'],
      entry['id'],
      reason: '§9.5 excludes sig from the digest, so signing cannot move it',
    );
  });

  test('a real signature binds a key to a participant under §10.7', () async {
    final wallet = FakeWallet();
    final anaSeed = seedFor('ana');
    final benSeed = seedFor('ben');
    final anaKey = await signer.publicKeyFromSeed(anaSeed);
    final benKey = await signer.publicKeyFromSeed(benSeed);

    final ana = WalletBillHost(wallet, sign: signer.signerFor(anaSeed));
    // §10.7: a participant who publishes a key is named by the id it derives.
    final benId = splitz.participantId(benKey)!;
    final benWallet = FakeWallet(id: benId, payTo: 'u1ben');
    final ben = WalletBillHost(benWallet, sign: signer.signerFor(benSeed));

    final create = splitz.createBill(
      host: ana,
      name: 'Dinner',
      currency: 'EUR',
      creatorKey: anaKey,
    );
    final bill = create['id'] as String;
    final entries = <Map<String, dynamic>>[
      await splitz.signEntry(host: ana, entry: create, billId: bill),
    ];
    wallet.tick();
    entries.add(
      await splitz.signEntry(
        host: ana,
        entry: splitz.joinBill(
          host: ana,
          name: 'Ana',
          payTo: 'u1ana',
          identityKey: anaKey,
        ),
        billId: bill,
      ),
    );
    benWallet.tick();
    benWallet.tick();
    entries.add(
      await splitz.signEntry(
        host: ben,
        entry: splitz.joinBill(
          host: ben,
          name: 'Ben',
          payTo: 'u1ben',
          identityKey: benKey,
        ),
        billId: bill,
      ),
    );

    final folded = await foldVerified(
      wallet,
      entries,
      billId: billIdOf(entries),
      signer: signer,
    );
    expect(folded.setAside, isEmpty);
    expect(folded.identities.bound['ana'], anaKey);
    expect(folded.identities.bound[benId], benKey);
  });

  test('a host that signs speaks as the id its key derives, not its '
      'account handle', () async {
    final wallet = FakeWallet();
    final anaSeed = seedFor('ana');
    final benSeed = seedFor('ben');
    final anaKey = await signer.publicKeyFromSeed(anaSeed);
    final benKey = await signer.publicKeyFromSeed(benSeed);
    final ana = WalletBillHost(wallet, sign: signer.signerFor(anaSeed));
    final create = splitz.createBill(
      host: ana,
      name: 'Dinner',
      currency: 'EUR',
      creatorKey: anaKey,
    );
    final bill = create['id'] as String;
    final signedCreate = await splitz.signEntry(
      host: ana,
      entry: create,
      billId: bill,
    );

    // The same account, filed under a handle the wallet assigned.
    final benWallet = FakeWallet(id: 'account-7', payTo: 'u1ben');
    Future<Map<String, dynamic>> joinAs(WalletBillHost host) =>
        splitz.signEntry(
          host: host,
          entry: splitz.joinBill(
            host: host,
            name: 'Ben',
            payTo: 'u1ben',
            identityKey: benKey,
          ),
          billId: bill,
        );

    final derived = await signer.participantIdFromSeed(benSeed);
    expect(derived, splitz.participantId(benKey));
    final speaking = WalletBillHost(
      benWallet,
      me: derived,
      sign: signer.signerFor(benSeed),
    );
    benWallet.tick();
    final bound = await foldVerified(
      wallet,
      [signedCreate, await joinAs(speaking)],
      billId: bill,
      signer: signer,
    );
    expect(bound.setAside, isEmpty);
    expect(bound.identities.bound[derived], benKey);

    final handle = WalletBillHost(benWallet, sign: signer.signerFor(benSeed));
    benWallet.tick();
    final refused = await foldVerified(
      wallet,
      [signedCreate, await joinAs(handle)],
      billId: bill,
      signer: signer,
    );
    expect(refused.setAside.map((a) => a.code), [
      protocol.SplitCode.participantIdNotDerived,
    ]);
    expect(refused.bill.participant('account-7'), isNull);
  });

  test('a create signed by the wrong key opens no bill at all', () async {
    final wallet = FakeWallet();
    final anaSeed = seedFor('ana');
    final anaKey = await signer.publicKeyFromSeed(anaSeed);
    // Signs with somebody else's seed while claiming ana's key.
    final ana = WalletBillHost(wallet, sign: signer.signerFor(seedFor('zzz')));

    final create = splitz.createBill(
      host: ana,
      name: 'Dinner',
      currency: 'EUR',
      creatorKey: anaKey,
    );
    final bill = create['id'] as String;
    final entries = <Map<String, dynamic>>[
      await splitz.signEntry(host: ana, entry: create, billId: bill),
    ];
    wallet.tick();
    entries.add(
      await splitz.signEntry(
        host: ana,
        entry: splitz.joinBill(
          host: ana,
          name: 'Ana',
          payTo: 'u1ana',
          identityKey: anaKey,
        ),
        billId: bill,
      ),
    );

    // Stronger than "unbound". §10.3 sets aside a create whose signature does
    // not verify against the key it itself states, and a log with no surviving
    // create opens nothing — so writing down somebody else's key does not get
    // a bill off the ground, it stops one existing.
    await expectLater(
      () => foldVerified(
        wallet,
        entries,
        billId: billIdOf(entries),
        signer: signer,
      ),
      throwsA(
        isA<protocol.SplitError>().having(
          (e) => e.code,
          'code',
          'log_no_create',
        ),
      ),
    );

    // And the same log folds fine with no verifier: §10.7 then binds nothing
    // rather than refusing, which is the honest answer for a device that
    // cannot check a signature.
    final unchecked = foldUnverified(
      wallet,
      entries,
      billId: billIdOf(entries),
    );
    expect(unchecked.bill.participants.single.id, 'ana');
    expect(unchecked.identities.bound, isEmpty);
  });

  test('a second key claiming a bound participant binds nothing', () async {
    final wallet = FakeWallet();
    final anaSeed = seedFor('ana');
    final benSeed = seedFor('ben');
    final impostorSeed = seedFor('zzz');
    final anaKey = await signer.publicKeyFromSeed(anaSeed);
    final benKey = await signer.publicKeyFromSeed(benSeed);
    final impostorKey = await signer.publicKeyFromSeed(impostorSeed);

    final ana = WalletBillHost(wallet, sign: signer.signerFor(anaSeed));
    final benId = splitz.participantId(benKey)!;
    final benWallet = FakeWallet(id: benId, payTo: 'u1ben');
    final ben = WalletBillHost(benWallet, sign: signer.signerFor(benSeed));
    final impostorWallet = FakeWallet(id: benId, payTo: 'u1impostor');
    final impostor = WalletBillHost(
      impostorWallet,
      sign: signer.signerFor(impostorSeed),
    );

    final create = splitz.createBill(
      host: ana,
      name: 'Dinner',
      currency: 'EUR',
      creatorKey: anaKey,
    );
    final bill = create['id'] as String;
    final entries = <Map<String, dynamic>>[
      await splitz.signEntry(host: ana, entry: create, billId: bill),
    ];
    wallet.tick();
    entries.add(
      await splitz.signEntry(
        host: ana,
        entry: splitz.joinBill(
          host: ana,
          name: 'Ana',
          payTo: 'u1ana',
          identityKey: anaKey,
        ),
        billId: bill,
      ),
    );
    benWallet.tick();
    benWallet.tick();
    entries.add(
      await splitz.signEntry(
        host: ben,
        entry: splitz.joinBill(
          host: ben,
          name: 'Ben',
          payTo: 'u1ben',
          identityKey: benKey,
        ),
        billId: bill,
      ),
    );
    for (var i = 0; i < 4; i++) {
      impostorWallet.tick();
    }
    entries.add(
      await splitz.signEntry(
        host: impostor,
        entry: splitz.joinBill(
          host: impostor,
          name: 'Ben',
          payTo: 'u1impostor',
          identityKey: impostorKey,
        ),
        billId: bill,
      ),
    );

    final folded = await foldVerified(
      wallet,
      entries,
      billId: billIdOf(entries),
      signer: signer,
    );
    expect(
      folded.identities.bound[benId],
      benKey,
      reason: "a second key cannot derive ben's id, so ben stays bound",
    );
    expect(folded.bill.participant(benId)!.payTo, 'u1ben');
  });

  test('an unsigned entry verifies against nothing', () async {
    final unsigned = <String, dynamic>{'id': 'e1', 'author': 'ana'};
    expect(
      await signer.verifyEntry(unsigned, 'A' * 43, billId: _bill),
      isFalse,
    );
  });

  test('a malformed key or signature is false, not a crash', () async {
    final wallet = FakeWallet();
    final seed = seedFor('ana');
    final host = WalletBillHost(wallet, sign: signer.signerFor(seed));
    final signed = await splitz.signEntry(
      host: host,
      entry: splitz.joinBill(host: host, name: 'Ana'),
      billId: _bill,
    );

    expect(
      await signer.verifyEntry(signed, 'not base64url!!', billId: _bill),
      isFalse,
    );
    expect(
      await signer.verifyEntry(signed, 'AAAA', billId: _bill),
      isFalse,
      reason: 'four characters is three bytes, not a 32-byte key',
    );

    final tampered = <String, dynamic>{...signed, 'sig': '!!!!'};
    expect(
      await signer.verifyEntry(tampered, 'A' * 43, billId: _bill),
      isFalse,
    );
  });

  test('every question the fold asks was answered in advance', () async {
    final wallet = FakeWallet();
    final seed = seedFor('ana');
    final key = await signer.publicKeyFromSeed(seed);
    final host = WalletBillHost(wallet, sign: signer.signerFor(seed));

    final create = splitz.createBill(
      host: host,
      name: 'Dinner',
      currency: 'EUR',
      creatorKey: key,
    );
    final bill = create['id'] as String;
    final entries = <Map<String, dynamic>>[
      await splitz.signEntry(host: host, entry: create, billId: bill),
    ];
    wallet.tick();
    entries.add(
      await splitz.signEntry(
        host: host,
        entry: splitz.joinBill(
          host: host,
          name: 'Ana',
          payTo: 'u1ana',
          identityKey: key,
        ),
        billId: bill,
      ),
    );

    final verified = await signer.prepare(entries, billId: bill);
    protocol.foldLog(entries, verify: verified.verify);
    expect(
      verified.unanswered,
      isEmpty,
      reason: 'a pair nobody answered would read as an invalid signature',
    );
  });

  test('a fold that asks an unanticipated question fails loudly', () async {
    // The guard's own witness: prepare nothing, then fold a log that does ask.
    final wallet = FakeWallet();
    final seed = seedFor('ana');
    final key = await signer.publicKeyFromSeed(seed);
    final host = WalletBillHost(wallet, sign: signer.signerFor(seed));
    final unsigned = splitz.createBill(
      host: host,
      name: 'Dinner',
      currency: 'EUR',
      creatorKey: key,
    );
    final bill = unsigned['id'] as String;
    final create = await splitz.signEntry(
      host: host,
      entry: unsigned,
      billId: bill,
    );

    final empty = await signer.prepare(
      const <Map<String, dynamic>>[],
      billId: bill,
    );
    // The fold asks, gets `false` for want of an answer, and drops the create
    // — so it refuses the whole log. The question still went on the record.
    expect(
      () => protocol.foldLog([create], verify: empty.verify),
      throwsA(isA<protocol.SplitError>()),
    );
    expect(empty.unanswered, isNotEmpty);

    // Through the path a caller uses it surfaces as the verifier's fault and
    // not the log's. `log_no_create` would name the wrong thing.
    await expectLater(
      () => foldVerified(
        wallet,
        [create],
        billId: create['id'] as String,
        signer: _PreparesNothing(),
      ),
      throwsA(isA<UnansweredSignatureQuestion>()),
    );
  });
}

/// A signer whose preparation answers nothing, to trip the guard.
class _PreparesNothing extends SplitsSigner {
  @override
  Future<VerifiedLog> prepare(
    Iterable<Map<String, dynamic>> entries, {
    required String billId,
  }) => super.prepare(const <Map<String, dynamic>>[], billId: billId);
}
