/// §14.9: a bill is settled only once its creator has closed it.
///
/// The close is an entry (§10.9), so every device reads one answer from the
/// log. These are the host's refusals and builders around it; the fold
/// itself stays permissive, because a payment someone already made is a fact
/// whatever state the bill was in.
library;

import 'package:splitz_core/splitz_core.dart' as splitz;

import 'bill_log.dart';
import 'entries.dart';
import 'host.dart';

/// Why no payment may start on [folded] now, or null when one may: the bill
/// is open, and its creator has not closed it for settling (§14.9).
///
/// Every way of paying asks this — the request, a swap, a cash record — so a
/// debt is never paid while the expenses that make it can still change.
String? settleRefusal(FoldedBill folded) =>
    folded.closed ? null : splitz.SplitCode.billNotClosed;

/// Why no expense may be added or corrected on [folded] now, or null when
/// one may: the bill is closed, and its debts are being paid (§14.9).
///
/// The fold would accept one and reopen the bill (§10.9); a host refuses to
/// write it so that a person changes what is owed only by asking the creator
/// to reopen.
String? expenseRefusal(FoldedBill folded) =>
    folded.closed ? splitz.SplitCode.billClosed : null;

/// The entry that closes [folded] for settling (§10.9).
///
/// Refused unless [host] is the bill's creator: a close by anybody else is
/// set aside by every fold, and writing one would tell its author the bill
/// was closed when it is not.
Map<String, dynamic> closeFor(BillHost host, FoldedBill folded) {
  if (host.me != folded.creatorId) {
    throw splitz.SplitError(
      splitz.SplitCode.unauthorizedEntry,
      'Only the creator closes a bill',
    );
  }
  return closeBill(
    host: host,
    covers: folded.closedOver,
    notBefore: folded.lastCloseAt,
  );
}

/// The entry that reopens [folded], withdrawing the close it is closed by
/// (§10.8), or null when it is open.
Map<String, dynamic>? reopenFor(BillHost host, FoldedBill folded) {
  final close = folded.closeEntry;
  if (close == null) return null;
  if (host.me != folded.creatorId) {
    throw splitz.SplitError(
      splitz.SplitCode.unauthorizedEntry,
      'Only the creator reopens a bill',
    );
  }
  return voidEntry(host: host, targetId: close, notBefore: folded.lastCloseAt);
}
