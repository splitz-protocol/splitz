import 'package:test/test.dart';
import 'package:splitz_core/host.dart' as splitz;
import 'package:splitz_core/splitz_core.dart' as protocol;
import 'package:splitz_host/splitz_host.dart';

import 'support/fake_wallet.dart';

/// A distinct 32-byte Ed25519 seed per person. Fixed, so a failing run can be
/// repeated: §9.4's nonce is what separates two bills, not this.
List<int> seedFor(String who) =>
    List<int>.generate(SplitsSigner.seedBytes, (i) => who.codeUnitAt(0) + i);

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
    final once = await splitz.signEntry(host: host, entry: entry);
    final twice = await splitz.signEntry(host: host, entry: entry);
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
    final benWallet = FakeWallet(id: 'ben', payTo: 'u1ben');
    final ben = WalletBillHost(benWallet, sign: signer.signerFor(benSeed));

    final entries = <Map<String, dynamic>>[
      await splitz.signEntry(
        host: ana,
        entry: splitz.createBill(
          host: ana,
          name: 'Dinner',
          currency: 'EUR',
          creatorKey: anaKey,
        ),
      ),
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
      ),
    );

    final folded = await foldVerified(wallet, entries, signer: signer);
    expect(folded.setAside, isEmpty);
    expect(folded.identities.bound['ana'], anaKey);
    expect(folded.identities.bound['ben'], benKey);
    expect(folded.identities.contested, isEmpty);
  });

  test('a create signed by the wrong key opens no bill at all', () async {
    final wallet = FakeWallet();
    final anaSeed = seedFor('ana');
    final anaKey = await signer.publicKeyFromSeed(anaSeed);
    // Signs with somebody else's seed while claiming ana's key.
    final ana = WalletBillHost(wallet, sign: signer.signerFor(seedFor('zzz')));

    final entries = <Map<String, dynamic>>[
      await splitz.signEntry(
        host: ana,
        entry: splitz.createBill(
          host: ana,
          name: 'Dinner',
          currency: 'EUR',
          creatorKey: anaKey,
        ),
      ),
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
      ),
    );

    // Stronger than "unbound". §10.3 sets aside a create whose signature does
    // not verify against the key it itself states, and a log with no surviving
    // create opens nothing — so writing down somebody else's key does not get
    // a bill off the ground, it stops one existing.
    await expectLater(
      () => foldVerified(wallet, entries, signer: signer),
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
    final unchecked = foldUnverified(wallet, entries);
    expect(unchecked.bill.participants.single.id, 'ana');
    expect(unchecked.identities.bound, isEmpty);
  });

  test('two keys claiming one id leaves that id contested', () async {
    final wallet = FakeWallet();
    final anaSeed = seedFor('ana');
    final benSeed = seedFor('ben');
    final impostorSeed = seedFor('zzz');
    final anaKey = await signer.publicKeyFromSeed(anaSeed);
    final benKey = await signer.publicKeyFromSeed(benSeed);
    final impostorKey = await signer.publicKeyFromSeed(impostorSeed);

    final ana = WalletBillHost(wallet, sign: signer.signerFor(anaSeed));
    final benWallet = FakeWallet(id: 'ben', payTo: 'u1ben');
    final ben = WalletBillHost(benWallet, sign: signer.signerFor(benSeed));
    final impostorWallet = FakeWallet(id: 'ben', payTo: 'u1impostor');
    final impostor = WalletBillHost(
      impostorWallet,
      sign: signer.signerFor(impostorSeed),
    );

    final entries = <Map<String, dynamic>>[
      await splitz.signEntry(
        host: ana,
        entry: splitz.createBill(
          host: ana,
          name: 'Dinner',
          currency: 'EUR',
          creatorKey: anaKey,
        ),
      ),
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
      ),
    );

    final folded = await foldVerified(wallet, entries, signer: signer);
    expect(folded.identities.contested, contains('ben'));
    expect(
      folded.identities.bound.containsKey('ben'),
      isFalse,
      reason: 'nothing inside the log says which claim is the person',
    );
  });

  test('an unsigned entry verifies against nothing', () async {
    final unsigned = <String, dynamic>{'id': 'e1', 'author': 'ana'};
    expect(await signer.verifyEntry(unsigned, 'A' * 43), isFalse);
  });

  test('a malformed key or signature is false, not a crash', () async {
    final wallet = FakeWallet();
    final seed = seedFor('ana');
    final host = WalletBillHost(wallet, sign: signer.signerFor(seed));
    final signed = await splitz.signEntry(
      host: host,
      entry: splitz.joinBill(host: host, name: 'Ana'),
    );

    expect(await signer.verifyEntry(signed, 'not base64url!!'), isFalse);
    expect(
      await signer.verifyEntry(signed, 'AAAA'),
      isFalse,
      reason: 'four characters is three bytes, not a 32-byte key',
    );

    final tampered = <String, dynamic>{...signed, 'sig': '!!!!'};
    expect(await signer.verifyEntry(tampered, 'A' * 43), isFalse);
  });

  test('every question the fold asks was answered in advance', () async {
    final wallet = FakeWallet();
    final seed = seedFor('ana');
    final key = await signer.publicKeyFromSeed(seed);
    final host = WalletBillHost(wallet, sign: signer.signerFor(seed));

    final entries = <Map<String, dynamic>>[
      await splitz.signEntry(
        host: host,
        entry: splitz.createBill(
          host: host,
          name: 'Dinner',
          currency: 'EUR',
          creatorKey: key,
        ),
      ),
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
      ),
    );

    final verified = await signer.prepare(entries);
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
    final create = await splitz.signEntry(
      host: host,
      entry: splitz.createBill(
        host: host,
        name: 'Dinner',
        currency: 'EUR',
        creatorKey: key,
      ),
    );

    final empty = await signer.prepare(const <Map<String, dynamic>>[]);
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
      () => foldVerified(wallet, [create], signer: _PreparesNothing()),
      throwsA(isA<UnansweredSignatureQuestion>()),
    );
  });
}

/// A signer whose preparation answers nothing, to trip the guard.
class _PreparesNothing extends SplitsSigner {
  @override
  Future<VerifiedLog> prepare(Iterable<Map<String, dynamic>> entries) =>
      super.prepare(const <Map<String, dynamic>>[]);
}
