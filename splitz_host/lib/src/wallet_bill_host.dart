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
    String? me,
    splitz.SignEntry? sign,
    splitz.VerifyEntry? verify,
    splitz.ReadsAddress? readsAddress,
  }) : _me = me,
       _sign = sign,
       _verify = verify,
       _readsAddress = readsAddress;

  final SplitsWallet _wallet;
  final String? _me;
  final splitz.SignEntry? _sign;
  final splitz.VerifyEntry? _verify;
  final splitz.ReadsAddress? _readsAddress;

  /// The participant id entries are written as: [me] when given, otherwise
  /// the account's own id.
  ///
  /// A host that signs passes the id its identity key derives (§10.7), which
  /// [SplitsSigner.participantIdFromSeed] computes. A join written under any
  /// other id with that key is set aside with `participant_id_not_derived`.
  @override
  String get me => _me ?? _wallet.account.id;

  @override
  splitz.Clock get now => _wallet.now;

  @override
  splitz.Randomness get randomBytes =>
      (int n) => Uint8List.fromList(_wallet.randomBytes(n));

  @override
  splitz.Broadcast get broadcast =>
      (String uri) async => sentOf(await _wallet.sender.send(uri));

  /// Null until an identity has been loaded for this account.
  ///
  /// Absent them, §10.7 binds no key to any participant and a folded bill
  /// reports no identity binding — which is a different claim from reporting
  /// that every key checked out, and is the honest one for a wallet that
  /// cannot check a signature.
  @override
  splitz.SignEntry? get sign => _sign;

  @override
  splitz.VerifyEntry? get verify => _verify;

  /// This wallet's own reading of an address, when it is narrower than §8.3's
  /// (§14.6). Null reads every address the protocol admits.
  @override
  splitz.ReadsAddress? get readsAddress => _readsAddress;
}

/// Maps the wallet's four send phases onto the protocol's three.
///
/// `aborted` and `failed` are one answer to a bill — nothing was spent — and
/// differ only in the message a person is shown. `pendingBroadcast` keeps its
/// own state: it is the one outcome from which nothing may be recorded and no
/// retry is safe.
splitz.Sent sentOf(WalletSendOutcome outcome) {
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
        txid: outcome.txid,
      );
    case WalletSendPhase.failed:
    case WalletSendPhase.aborted:
      return splitz.Sent.failed(
        detail: outcome.error ?? 'The transaction could not be sent.',
      );
  }
}
