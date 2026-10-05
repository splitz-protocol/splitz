/// Whether a refund on the bill accounts for what a payment carries beyond
/// the debts it covers (§6.3).
library;

import 'package:splitz_core/host.dart';
import 'package:splitz_core/splitz_core.dart' as protocol;

/// The refunds behind a settlement's unexplained part: how much the bill's
/// negative expenses move onto its payer, and who wrote them.
class RefundsBehind {
  const RefundsBehind({required this.refunded, required this.authors});

  /// What the refunds move onto the payer, in the bill's minor units: at
  /// least the settlement's unexplained part.
  final int refunded;

  /// Who wrote them, sorted; an expense with no known author is the empty
  /// string, so it never reads as the payer's own doing.
  final List<String> authors;
}

/// The refunds that account for [settlement]'s unexplained part, or null when
/// none do, or it has none (§6.3).
///
/// A refund is an expense below zero the payer is down as having paid: the
/// shares it gives everybody else move onto the payer. Only when those cover
/// the whole unexplained part may a host call it a refund; otherwise the bill
/// holds no refund that explains it — a confirmed payment above what was owed
/// leaves the same figure — and a host says only that no debt explains it.
RefundsBehind? refundsBehind(
  protocol.Settlement settlement,
  FoldedBill folded,
) {
  final unexplained = settlement.unexplained;
  if (unexplained <= 0) return null;
  var refunded = 0;
  final authors = <String>{};
  try {
    for (final e in folded.bill.expenses) {
      if (e.amount >= 0 || e.paidBy != settlement.from) continue;
      var moved = 0;
      protocol.splitExpense(e.amount, e.split).forEach((id, share) {
        if (id != settlement.from) {
          moved = protocol.checkedSubtract(moved, share);
        }
      });
      if (moved <= 0) continue;
      refunded = protocol.checkedAdd(refunded, moved);
      authors.add(folded.expenseAuthors[e.id] ?? '');
    }
  } on protocol.SplitError {
    return null;
  }
  if (refunded < unexplained) return null;
  return RefundsBehind(
    refunded: refunded,
    authors: protocol.sortedUtf8(authors),
  );
}
