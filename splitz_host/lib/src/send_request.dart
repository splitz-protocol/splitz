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
}) async {
  final String? why;
  try {
    why = proposalProblem(uri, await read());
  } on Object catch (error) {
    return WalletSendOutcome(
      phase: WalletSendPhase.failed,
      error: 'The wallet could not read this payment: $error',
    );
  }
  if (why != null) {
    return WalletSendOutcome(phase: WalletSendPhase.failed, error: why);
  }
  final P proposal;
  try {
    proposal = await propose();
  } on Object catch (error) {
    return WalletSendOutcome(
      phase: WalletSendPhase.failed,
      error: 'The wallet could not build this payment: $error',
    );
  }
  return broadcast(proposal);
}
