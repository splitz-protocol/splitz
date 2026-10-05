/// Development scaffolding, kept out of `package:splitz_host/splitz_host.dart`
/// so a wallet's shipped import carries none of it: the client of the
/// loopback seed driver a development run fetches its wallets from, one at a
/// time and never through a build define.
library;

export 'src/testing/seed_driver.dart';
