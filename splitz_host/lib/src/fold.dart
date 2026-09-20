/// Folding a log whose signatures have to be checked first.
library;

import 'package:splitz_core/host.dart' as splitz;

import 'signing.dart';
import 'wallet.dart';
import 'wallet_bill_host.dart';

/// Raised when a fold asked a signature question nobody had answered.
///
/// Not an ordinary refusal. It means [SplitsSigner.prepare] no longer
/// anticipates every pair §10.7 asks about, so the identities the fold reported
/// are wrong in a direction that looks exactly like "no signature" — which is
/// the quiet failure this whole two-pass exists to avoid.
class UnansweredSignatureQuestion implements Exception {
  const UnansweredSignatureQuestion(this.pairs);

  final List<String> pairs;

  @override
  String toString() =>
      'UnansweredSignatureQuestion: the fold asked about ${pairs.length} '
      '(entry, key) pair(s) that were never verified';
}

/// Folds [entries] with signatures checked.
///
/// Two passes, because the curve operation is asynchronous and `foldLog` takes
/// a synchronous verifier. Verifying first and folding with a pure lookup is
/// also what §10.7 wants: a verifier that could answer differently on two
/// devices would fold two different bills from one log.
///
/// Pass `seed` when this device should sign what it writes; it changes nothing
/// about the fold, which only ever verifies.
Future<splitz.FoldedBill> foldVerified(
  SplitsWallet wallet,
  List<Map<String, dynamic>> entries, {
  SplitsSigner? signer,
  List<int>? seed,
}) async {
  final signing = signer ?? SplitsSigner();
  final verified = await signing.prepare(entries);

  final host = WalletBillHost(
    wallet,
    sign: seed == null ? null : signing.signerFor(seed),
    verify: verified.verify,
  );

  // The fold is attempted, and the unanswered check runs whether or not it
  // succeeded. An unanswered pair makes a *failure* as untrustworthy as a
  // result: §10.3 drops a create whose signature does not verify, so a
  // question nobody answered turns into `log_no_create` — a refusal that
  // names the log and says nothing about the verifier that caused it.
  splitz.FoldedBill? folded;
  Object? failure;
  try {
    folded = splitz.BillLog(host, entries: entries).fold();
  } on Object catch (e) {
    failure = e;
  }
  if (verified.unanswered.isNotEmpty) {
    throw UnansweredSignatureQuestion(verified.unanswered);
  }
  if (failure != null) throw failure;
  return folded!;
}

/// Folds [entries] without checking any signature.
///
/// For a device that holds no identity yet. §10.7 then binds no key and reports
/// no contest, which is a different claim from reporting that nothing is
/// contested — and is the honest one here.
splitz.FoldedBill foldUnverified(
  SplitsWallet wallet,
  List<Map<String, dynamic>> entries,
) => splitz.BillLog(WalletBillHost(wallet), entries: entries).fold();
