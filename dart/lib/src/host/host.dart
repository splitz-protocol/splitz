/// What the host layer needs from the wallet that embeds it.
///
/// The protocol hands transport, keys, signing, broadcast and the clock to its
/// host (§13), and this layer is not a wallet either — it holds no key,
/// opens no socket and reads no clock of its own. Everything it cannot do is
/// declared here, so the whole feature can be exercised without one.
library;

import 'dart:typed_data';

/// A moment, as the protocol writes one.
///
/// Taken from the host rather than from `DateTime.now()` so a test can hold
/// the clock still: §9.3 instants order a log, and a log that reorders between
/// runs cannot be asserted.
typedef Clock = DateTime Function();

/// Bytes nobody can predict.
///
/// §9.4 derives a bill's id from a nonce, so two bills created in the same
/// second by the same person are the same bill unless this is unpredictable.
/// The host supplies it because the host knows what secure randomness means on
/// its platform.
typedef Randomness = Uint8List Function(int byteCount);

/// How a send ended.
///
/// A wallet has a third answer between success and failure: a transaction
/// built and signed but not yet handed to the network, which may still land
/// later. It is neither paid nor unpaid, and collapsing it into either one
/// loses money — recorded as paid, a transaction that never lands leaves a
/// real debt showing as settled; recorded as nothing, one that does land is
/// paid a second time.
enum SendResult {
  /// The network has the transaction.
  sent,

  /// Built, not broadcast. Nothing may be recorded from it, and no retry is
  /// safe until the wallet says which way it went.
  pending,

  /// It will not land. Nothing was spent.
  failed,
}

/// What a send produced.
class Sent {
  const Sent({required this.result, this.txid, this.detail});

  const Sent.sent(String this.txid)
      : result = SendResult.sent,
        detail = null;

  const Sent.pending({this.detail})
      : result = SendResult.pending,
        txid = null;

  const Sent.failed({this.detail})
      : result = SendResult.failed,
        txid = null;

  final SendResult result;

  /// Present when and only when [result] is [SendResult.sent]. It becomes the
  /// id of the payment entry, so the record of a payment and the transaction
  /// that made it carry one identifier.
  final String? txid;

  /// What to put in front of a person: why it failed, or what to check before
  /// trying again.
  final String? detail;
}

/// What the wallet does with a payment request this layer renders.
typedef Broadcast = Future<Sent> Function(String paymentRequestUri);

/// The host's signature over an entry's signing message (§10.6).
///
/// Optional: without it every participant is unauthenticated and the fold says
/// so rather than claiming otherwise. With it, §10.7 binds a key to a
/// participant and a contested identity is reported as contested.
typedef SignEntry = Future<String> Function(List<int> signingMessage);

/// Verifies a signature the way §10.7 asks.
typedef VerifyEntry = bool Function(Map<String, dynamic> entry, String key);

/// The wallet, as this layer needs it.
///
/// A single object rather than loose callbacks so a host implements one thing
/// and a test fakes one thing.
abstract class BillHost {
  /// The participant id this device speaks as. Every entry it writes is
  /// authored by this id, and §10.4 decides what that authorises.
  String get me;

  /// The address this device is paid at, or null when it has none to offer.
  /// A participant with no address is reported as unpayable rather than
  /// silently dropped from a settlement.
  String? get payToAddress;

  Clock get now;
  Randomness get randomBytes;
  Broadcast get broadcast;

  /// Null when this wallet does not sign entries. The fold then reports no
  /// identity binding rather than an empty one, which are different claims.
  ///
  /// These two default to null for a host that `extends BillHost`. A host that
  /// `implements` it must state them, because Dart carries no implementation
  /// across `implements`.
  SignEntry? get sign => null;
  VerifyEntry? get verify => null;
}
