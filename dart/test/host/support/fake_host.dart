/// A wallet that does nothing, for tests that are not about the wallet.
///
/// The clock is held still and the randomness is a counter: §9.3 instants
/// order a log and §9.4 derives a bill id from a nonce, so a log that moves
/// between runs cannot be asserted against a fixed expectation.
library;

import 'dart:typed_data';

import 'package:splitz_core/host.dart';

class FakeHost implements BillHost {
  FakeHost({
    required this.me,
    this.payToAddress,
    DateTime? at,
    this.sign,
    this.verify,
    this.readsAddress,
  }) : _at = at ?? DateTime.utc(2026, 10, 28, 19, 30);

  @override
  final String me;

  /// The address this fake is paid at, for a test that writes it into a join.
  final String? payToAddress;
  @override
  final SignEntry? sign;
  @override
  final VerifyEntry? verify;
  @override
  final ReadsAddress? readsAddress;

  DateTime _at;
  int _counter = 0;

  /// Moves the clock on, so two entries written in one test are two entries
  /// and §10.2 has an order to put them in.
  void tick([Duration by = const Duration(minutes: 1)]) => _at = _at.add(by);

  @override
  Clock get now => () => _at;

  @override
  Randomness get randomBytes => (int n) {
        _counter++;
        return Uint8List.fromList(List<int>.generate(n, (i) => _counter + i));
      };

  @override
  Broadcast get broadcast => (uri) async => Sent.sent('tx-${uri.hashCode}');
}

/// A 32-byte key in the encoding §9.4 wants, distinct per participant.
String fakeKey(String who) =>
    base64UrlNoPad(List<int>.generate(32, (i) => who.codeUnitAt(0) + i));
