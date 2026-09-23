import 'dart:typed_data';

import 'package:splitz_core/host.dart' as splitz;
import 'package:splitz_host/splitz_host.dart';

/// A wallet that does nothing, for tests that are not about the wallet.
///
/// The clock is held still and the randomness is a counter: §9.3 instants order
/// a log and §9.4 derives a bill id from a nonce, so a log that moves between
/// runs cannot be asserted against a fixed expectation.
/// A secret derived from the account id, standing in for a mnemonic's.
const Object _derived = Object();

class FakeWallet implements SplitsWallet {
  FakeWallet({
    String id = 'ana',
    String? payTo = 'u1ana000000000000000000',
    Object? identitySecret = _derived,
    WalletSendOutcome outcome = const WalletSendOutcome(
      phase: WalletSendPhase.succeeded,
      txid: 'tx-1',
    ),
  }) : account = WalletAccount(
         id: id,
         identitySecret: identical(identitySecret, _derived)
             ? 'secret-$id'.codeUnits
             : identitySecret as List<int>?,
       ),
       sender = FakeSender(payToAddress: payTo, outcome: outcome);

  @override
  final WalletAccount account;

  @override
  final FakeSender sender;

  @override
  final SecretStore secrets = InMemorySecretStore();

  DateTime _at = DateTime.utc(2026, 10, 28, 19, 30);
  int _counter = 0;

  /// Moves the clock on, so two entries written in one test are two entries and
  /// §10.2 has an order to put them in.
  void tick([Duration by = const Duration(minutes: 1)]) => _at = _at.add(by);

  @override
  DateTime now() => _at;

  @override
  Uint8List randomBytes(int byteCount) {
    _counter++;
    return Uint8List.fromList(
      List<int>.generate(byteCount, (i) => _counter + i),
    );
  }
}

class FakeSender implements WalletSender {
  FakeSender({required this.payToAddress, required this.outcome});

  @override
  final String? payToAddress;

  final WalletSendOutcome outcome;
  final List<String> sent = [];

  @override
  Future<WalletSendOutcome> send(String paymentRequestUri) async {
    sent.add(paymentRequestUri);
    return outcome;
  }
}

/// A 32-byte key in the encoding §9.4 wants, distinct per participant.
String fakeKey(String who) => splitz.base64UrlNoPad(
  List<int>.generate(splitz.creatorKeyBytes, (i) => who.codeUnitAt(0) + i),
);

/// A bill host that does nothing, for tests about entries rather than wallets.
///
/// The clock is held still and the randomness is a counter: §9.3 instants
/// order a log and §9.4 derives a bill id from a nonce, so a log that moved
/// between runs could not be asserted against a fixed expectation.
class FakeHost implements splitz.BillHost {
  FakeHost({required this.me, this.payToAddress, this.sign, this.verify});

  @override
  final String me;
  @override
  final String? payToAddress;
  @override
  final splitz.SignEntry? sign;
  @override
  final splitz.VerifyEntry? verify;

  DateTime _at = DateTime.utc(2026, 10, 28, 19, 30);
  int _counter = 0;

  /// Moves the clock on, so two entries written in one test are two entries
  /// and §10.2 has an order to put them in.
  void tick([Duration by = const Duration(minutes: 1)]) => _at = _at.add(by);

  @override
  splitz.Clock get now =>
      () => _at;

  @override
  splitz.Randomness get randomBytes => (int n) {
    _counter++;
    return Uint8List.fromList(List<int>.generate(n, (i) => _counter + i));
  };

  @override
  splitz.Broadcast get broadcast =>
      (uri) async => splitz.Sent.sent('tx-${uri.hashCode}');
}

/// The bill a test log opens: the id of its create entry.
String billIdOf(List<Map<String, dynamic>> entries) =>
    entries.firstWhere((e) => e['kind'] == 'createBill')['id'] as String;
