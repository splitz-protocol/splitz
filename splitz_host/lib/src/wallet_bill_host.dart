/// The protocol's host seam, implemented over a wallet.
library;

import 'dart:typed_data';

import 'package:splitz_core/host.dart' as splitz;

import 'wallet.dart';

/// Hands `splitz` what SPEC.md §13 says a wallet owes it.
///
/// Two seams, not one. `splitz`'s `BillHost` is what the protocol asks for;
/// [SplitsWallet] is what a Zcash wallet already has. This maps the second onto
/// the first, so neither has to know about the other and either can change
/// without the other's callers noticing.
class WalletBillHost extends splitz.BillHost {
  WalletBillHost(
    this._wallet, {
    splitz.SignEntry? sign,
    splitz.VerifyEntry? verify,
  }) : _sign = sign,
       _verify = verify;

  final SplitsWallet _wallet;
  final splitz.SignEntry? _sign;
  final splitz.VerifyEntry? _verify;

  @override
  String get me => _wallet.account.id;

  @override
  String? get payToAddress => _wallet.sender.payToAddress;

  @override
  splitz.Clock get now => _wallet.now;

  @override
  splitz.Randomness get randomBytes =>
      (int n) => Uint8List.fromList(_wallet.randomBytes(n));

  /// Maps the wallet's four send phases onto the protocol's three.
  ///
  /// `aborted` and `failed` are one answer to a bill — nothing was spent — and
  /// differ only in the message a person is shown. `pendingBroadcast` keeps its
  /// own state: it is the one outcome from which nothing may be recorded and no
  /// retry is safe.
  @override
  splitz.Broadcast get broadcast => (String uri) async {
    final outcome = await _wallet.sender.send(uri);
    switch (outcome.phase) {
      case WalletSendPhase.succeeded:
        final txid = outcome.txid;
        // Pending, not failed: the wallet says money left, and without an
        // id nothing can be recorded — but a retry could pay it twice.
        if (txid == null) {
          return const splitz.Sent.pending(
            detail: 'the wallet reported a send with no transaction id',
          );
        }
        return splitz.Sent.sent(txid);
      case WalletSendPhase.pendingBroadcast:
        return splitz.Sent.pending(
          detail:
              outcome.statusMessage ??
              'The transaction was created but not broadcast yet. '
                  'Check its status before trying again.',
        );
      case WalletSendPhase.failed:
      case WalletSendPhase.aborted:
        return splitz.Sent.failed(
          detail: outcome.error ?? 'The transaction could not be sent.',
        );
    }
  };

  /// Null until an identity has been loaded for this account.
  ///
  /// Absent them, §10.7 binds no key to any participant and a folded bill
  /// reports no identity binding — which is a different claim from reporting
  /// that nothing is contested, and is the honest one for a wallet that cannot
  /// check a signature.
  @override
  splitz.SignEntry? get sign => _sign;

  @override
  splitz.VerifyEntry? get verify => _verify;
}
