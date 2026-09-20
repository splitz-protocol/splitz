/// Everything this package needs from the wallet it is embedded in.
///
/// Declared here as interfaces rather than imported from the app. A feature
/// written inside a wallet reaches for whatever is next to it, and then cannot
/// leave: it is the difference between a package a wallet depends on and a
/// directory that only compiles in one tree. Nothing under `lib/` imports
/// outside this package, and the analyzer enforces that by having nothing to
/// import.
///
/// It is the same shape `splitz`'s own `BillHost` has, one layer out: the
/// protocol hands transport, keys, signing, broadcast and the clock to its host
/// (SPEC.md §13), and this package is not a wallet either.
library;

import 'dart:typed_data';

/// How a wallet's send ended.
///
/// Four states, not two. `pendingBroadcast` is a transaction that was built and
/// signed but not handed to the network: it may still land, so it is neither
/// paid nor unpaid. Collapsing it into either loses money — recorded as paid, a
/// transaction that never lands leaves a real debt showing as settled; recorded
/// as nothing, one that does land is paid a second time.
enum WalletSendPhase { succeeded, pendingBroadcast, failed, aborted }

/// What a broadcast attempt reported.
class WalletSendOutcome {
  const WalletSendOutcome({
    required this.phase,
    this.txid,
    this.statusMessage,
    this.error,
  });

  final WalletSendPhase phase;

  /// Present when and only when [phase] is [WalletSendPhase.succeeded]. It
  /// becomes the id of the payment entry, so the record of a payment and the
  /// transaction that made it carry one identifier.
  final String? txid;

  /// What to put in front of a person while the send is unresolved.
  final String? statusMessage;

  /// What to put in front of a person when it failed.
  final String? error;
}

/// The wallet's own send path.
///
/// One call takes the whole ZIP 321 URI, which is why a payer who owes four
/// people signs once. Splitting it into one transaction per recipient would
/// cost four fees and four rounds of proving, and would let a person walk away
/// after the second.
abstract interface class WalletSender {
  /// Builds and signs a transaction paying every output in [paymentRequestUri],
  /// then broadcasts it.
  Future<WalletSendOutcome> send(String paymentRequestUri);

  /// The address this device is paid at, or null when it has none to offer.
  ///
  /// A participant with no address is reported as unpayable rather than
  /// silently dropped from a settlement, so this being null is a state to show
  /// and not an error.
  String? get payToAddress;
}

/// Where this package's secrets live.
///
/// The platform keychain in a real build. An interface so a test can exercise
/// key handling without a plugin, and so the backing store can change without
/// the callers noticing.
abstract interface class SecretStore {
  Future<String?> read(String key);
  Future<void> write(String key, String value);
  Future<void> delete(String key);
}

/// Secrets held in memory only.
///
/// For tests, and for nothing else: a bill key that does not outlive the
/// process cannot decrypt that bill tomorrow.
class InMemorySecretStore implements SecretStore {
  final Map<String, String> _values = {};

  @override
  Future<String?> read(String key) async => _values[key];

  @override
  Future<void> write(String key, String value) async => _values[key] = value;

  @override
  Future<void> delete(String key) async => _values.remove(key);
}

/// Which wallet account is speaking, and what makes its identity recoverable.
class WalletAccount {
  const WalletAccount({required this.id, this.viewingKey});

  /// The participant id this device speaks as on every bill. Every entry it
  /// writes is authored by this id, and §10.4 decides what that authorises.
  final String id;

  /// A unified full viewing key, when the wallet can supply one.
  ///
  /// It makes this account's signing identity recoverable: a viewing key is
  /// derived from the wallet seed, so the same mnemonic yields the same
  /// identity on a reinstalled device. An account identifier assigned by the
  /// wallet's own database does not — it is handed out at import time — so an
  /// identity filed only under that is a stranger to every bill naming it
  /// after a restore.
  ///
  /// Null is allowed and means the identity is random and unrecoverable. It
  /// still signs correctly.
  final String? viewingKey;
}

/// The wallet, as this package needs it.
///
/// One object rather than loose callbacks, so a host implements one thing and
/// a test fakes one thing.
abstract interface class SplitsWallet {
  WalletAccount get account;
  WalletSender get sender;
  SecretStore get secrets;

  /// A moment, taken from the wallet rather than from a clock this package
  /// reads. §9.3 instants order a log, and a log that reorders between runs
  /// cannot be asserted.
  DateTime now();

  /// Bytes nobody can predict.
  ///
  /// §9.4 derives a bill's id from a nonce, so two bills created in the same
  /// second by the same person are the same bill unless this is unpredictable.
  /// The wallet supplies it because the wallet knows what secure randomness
  /// means on its platform.
  Uint8List randomBytes(int byteCount);
}
