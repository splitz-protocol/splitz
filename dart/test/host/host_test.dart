import 'dart:typed_data';

import 'package:test/test.dart';
import 'package:splitz_core/host.dart';

/// The least a wallet can implement: no signing, no address to be paid at.
class BareHost extends BillHost {
  @override
  String get me => 'ana';
  String? get payToAddress => null;
  @override
  Clock get now => () => DateTime.utc(2026, 10, 28, 19, 30);
  @override
  Randomness get randomBytes => (n) => Uint8List(n);
  @override
  Broadcast get broadcast => (uri) async => const Sent.sent('tx1');
}

void main() {
  test('a wallet that does not sign says so, rather than supplying nothing',
      () {
    final host = BareHost();
    // Null and "a signer that returns an empty signature" are different
    // claims: the first leaves a participant unauthenticated, the second
    // asserts a binding that would not verify.
    expect(host.sign, isNull);
    expect(host.verify, isNull);
  });

  test('a wallet with no address to be paid at is still a host', () {
    expect(BareHost().payToAddress, isNull);
  });

  test('the clock and the randomness come from the host, not from the package',
      () {
    final host = BareHost();
    expect(host.now(), DateTime.utc(2026, 10, 28, 19, 30));
    expect(host.randomBytes(16).length, 16);
  });
}
