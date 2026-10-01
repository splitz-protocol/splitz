// The snippet INTEGRATING.md's "The wallet seam" section shows, compiled and
// run. A snippet that has never been through a compiler is a claim about the
// library that nothing in the tree backs.
//
// ignore_for_file: avoid_print
// docs:begin
import 'dart:typed_data';

import 'package:splitz_core/host.dart';
import 'package:splitz_core/splitz_core.dart' as splitz;

class MyWallet extends BillHost {
  @override
  String get me => 'ana';
  @override
  Clock get now => DateTime.now;
  @override
  Randomness get randomBytes => secureRandom;
  @override
  Broadcast get broadcast => (uri) async => Sent.sent(await send(uri));
  // `sign` and `verify` default to null. Without them §10.7 binds no key and
  // a folded bill reports no identity binding rather than claiming one.

  /// The address this wallet is paid at, which its join states.
  String get myAddress => 'u1ana000000000000000000';

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

  // The key the bill's entries are sealed under, which its create commits to
  // (§9.4): a key handed over with this bill's id and any other key is then
  // refused rather than opening a copy only its holder sees.
  final billKey = base64UrlNoPad(host.randomBytes(32));

  final log = BillLog(host)
    ..add([
      createBill(
        host: host,
        name: 'Dinner',
        currency: 'EUR',
        creatorKey: myEd25519PublicKeyBase64Url,
        billKey: billKey,
      ),
    ]);

  // A bill nobody has joined and nothing has been spent on still folds.
  log.add([joinBill(host: host, name: 'Ana', payTo: host.myAddress)]);

  final folded = log.fold(); // §10.3, plus what it refused
  final owed = obligationFor(host, folded); // null when the bill has no rate
  if (owed != null) await settle(host, log, owed);

  print('bill       ${folded.bill.id}');
  print('entries    ${log.entries.length}');
  print('setAside   ${folded.setAside.length}');
  print('obligation ${owed == null ? 'no rate yet' : owed.uri}');
  print('identities ${folded.identities.bound.length} bound');
  print('splitz     ${splitz.billVersion}');
}
