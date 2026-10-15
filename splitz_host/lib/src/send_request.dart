/// The order every wallet's send of a payment request keeps (§14.3, §14.6).
library;

import 'package:splitz_core/host.dart' as host;
import 'package:splitz_core/splitz_core.dart' as splitz;

import 'wallet.dart';

/// Why the payments a wallet read from [uri] are not the ones it asks for, in
/// words for the payer, or null when they are (§14.6).
///
/// [read] is what the wallet's own ZIP 321 reader produced: the reading its
/// proposal is built from. A request this protocol did not write is refused
/// with its code's sentence.
String? proposalProblem(String uri, List<host.ProposedOutput> read) {
  final host.ProposalCheck check;
  try {
    check = host.checkProposal(uri, read);
  } on splitz.SplitError catch (e) {
    return splitz.describeCode(e.code) ?? e.code;
  }
  if (check.matches) return null;
  return 'Your wallet read this payment differently. Nothing was sent.';
}

/// Why a proposal whose fee is [fee] may not be sent to a payer who reviewed
/// [reviewedFee], in words for the payer, or null when it may (§14.2). Both
/// are zatoshi.
///
/// A fee above the reviewed one is refused; one at or below it is not. Notes
/// can change between the review and the send, and a person confirms the
/// figure they saw, not one chosen after.
String? feeRaisedProblem({required int reviewedFee, required int fee}) {
  if (fee <= reviewedFee) return null;
  return 'The fee went up from ${_zec(reviewedFee)} to ${_zec(fee)} since '
      'you reviewed this payment. Nothing was sent; review it again.';
}

/// [zatoshi] as ZEC in §8.1's form, or as zatoshi when it is not an amount
/// §8.1 renders.
String _zec(int zatoshi) => zatoshi > 0 && zatoshi <= splitz.maxZatoshi
    ? '${splitz.renderAmount(zatoshi)} ZEC'
    : '$zatoshi zatoshi';

/// Sends [uri] in the order §14.3 and §14.6 require, as a
/// [WalletSender.send] does.
///
/// [read] is the wallet's own reading of the request, held against it with
/// [proposalProblem] before anything is built; [propose] builds and signs;
/// [broadcast] hands the transaction to the network and says which of the
/// three outcomes occurred.
///
/// A refusal before a transaction is built — a request read differently, too
/// little to spend, a reader or builder that raised — is
/// [WalletSendPhase.failed], never a send that may still land: nothing was
/// built, so nothing can arrive. [broadcast]'s own outcome is returned as it
/// is.
Future<WalletSendOutcome> sendPaymentRequest<P>({
  required String uri,
  required Future<List<host.ProposedOutput>> Function() read,
  required Future<P> Function() propose,
  required Future<WalletSendOutcome> Function(P proposal) broadcast,
}) => _send<P>(uri: uri, read: read, propose: propose, broadcast: broadcast);

/// [sendPaymentRequest] for a payer shown [reviewedFee] zatoshi as the fee.
///
/// [propose] answers the proposal and its fee. A proposal whose fee
/// [feeRaisedProblem] refuses is handed to [discard] and never broadcast, and
/// the send is [WalletSendPhase.failed] with that sentence; a [discard] that
/// raises does not change the outcome, since nothing was broadcast.
Future<WalletSendOutcome> sendReviewedPaymentRequest<P>({
  required String uri,
  required int reviewedFee,
  required Future<List<host.ProposedOutput>> Function() read,
  required Future<(P, int)> Function() propose,
  required Future<void> Function(P proposal) discard,
  required Future<WalletSendOutcome> Function(P proposal) broadcast,
}) {
  late int fee;
  return _send<P>(
    uri: uri,
    read: read,
    propose: () async {
      final P proposal;
      (proposal, fee) = await propose();
      return proposal;
    },
    refusal: (proposal) async {
      final why = feeRaisedProblem(reviewedFee: reviewedFee, fee: fee);
      if (why == null) return null;
      try {
        await discard(proposal);
      } on Object {
        // Nothing was broadcast; the refusal stands either way.
      }
      return why;
    },
    broadcast: broadcast,
  );
}

Future<WalletSendOutcome> _send<P>({
  required String uri,
  required Future<List<host.ProposedOutput>> Function() read,
  required Future<P> Function() propose,
  required Future<WalletSendOutcome> Function(P proposal) broadcast,
  Future<String?> Function(P proposal)? refusal,
}) async {
  WalletSendOutcome failed(String error) =>
      WalletSendOutcome(phase: WalletSendPhase.failed, error: error);
  final String? why;
  try {
    why = proposalProblem(uri, await read());
  } on Object catch (error) {
    return failed('The wallet could not read this payment: $error');
  }
  if (why != null) return failed(why);
  final P proposal;
  try {
    proposal = await propose();
  } on Object catch (error) {
    return failed('The wallet could not build this payment: $error');
  }
  final refused = await refusal?.call(proposal);
  if (refused != null) return failed(refused);
  return broadcast(proposal);
}
