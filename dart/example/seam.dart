// The snippet INTEGRATING.md's "The wallet seam" section shows, compiled and
// run. A snippet that has never been through a compiler is a claim about the
// library that nothing in the tree backs.
//
// ignore_for_file: avoid_print
import 'dart:typed_data';

import 'package:splitz/host.dart';
import 'package:splitz/splitz.dart' as splitz;

class MyWallet extends BillHost {
  @override
  String get me => 'ana';
  @override
  String? get payToAddress => 'u1ana000000000000000000';
  @override
  Clock get now => DateTime.now;
  @override
  Randomness get randomBytes => secureRandom;
  @override
  Broadcast get broadcast => (uri) async => Sent.sent(await send(uri));
  // `sign` and `verify` default to null. Without them §10.7 binds no key and
  // a folded bill reports no identity binding rather than claiming one.

  /// Stands in for the platform's secure random source.
  Uint8List secureRandom(int n) =>
      Uint8List.fromList(List<int>.generate(n, (i) => i * 7 + 1));

  /// Stands in for the wallet's send path.
  Future<String> send(String uri) async => 'tx-${uri.length}';
}

Future<void> main() async {
  final host = MyWallet();
  final myEd25519PublicKeyBase64Url = base64UrlNoPad(
    List<int>.generate(creatorKeyBytes, (i) => i),
  );

  final log = BillLog(host)
    ..add([
      createBill(
        host: host,
        name: 'Dinner',
        currency: 'EUR',
        creatorKey: myEd25519PublicKeyBase64Url,
      ),
    ]);

  // A bill nobody has joined and nothing has been spent on still folds.
  log.add([joinBill(host: host, name: 'Ana', payTo: host.payToAddress)]);

  final folded = log.fold(); // §10.3, plus what it refused
  final owed = obligationFor(host, folded); // null when the bill has no rate
  if (owed != null) await settle(host, log, owed);

  print('bill       ${folded.bill.id}');
  print('entries    ${log.entries.length}');
  print('setAside   ${folded.setAside.length}');
  print('obligation ${owed == null ? 'no rate yet' : owed.uri}');
  print('identities ${folded.identities.bound.length} bound, '
      '${folded.identities.contested.length} contested');
  print('splitz     ${splitz.billVersion}');
}
