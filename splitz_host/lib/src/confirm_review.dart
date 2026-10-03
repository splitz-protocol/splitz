/// What a payee is warned about before confirming a payment, beyond what
/// §14.2 requires be shown (§14.7).
library;

import 'package:splitz_core/host.dart' as host;
import 'package:splitz_core/splitz_core.dart' as protocol;

import 'pricing.dart';

/// Why a payment wants the payee's own look rather than a one-tap confirm.
enum PaymentConcern {
  /// The payer set the bill's rate: the figure the record's ZEC is checked
  /// against is the payer's own.
  rateSetByPayer,

  /// The record was priced at a rate other than the bill's.
  pricedAtAnotherRate,

  /// The rate the record is priced at is [rateWarningPercent] or more from a
  /// live price.
  rateFarFromLive,
}

/// The concerns [payment] on [folded] raises, in this enum's order; empty
/// when none does. [live] is a live price of one ZEC in the payment's
/// currency, or null when none could be read — not by itself a concern.
///
/// A host MAY confirm several `arrived` payments on one acceptance (§14.7),
/// and SHOULD leave out of it every one with a concern: each is checked
/// against a figure somebody with a stake in it chose.
List<PaymentConcern> concernsBeforeConfirming(
  protocol.PaymentRecord payment,
  host.FoldedBill folded, {
  int? live,
}) {
  final rate = folded.bill.rate;
  final pricedAt = payment.paidAtRate ?? rate;
  return [
    if (folded.rateAuthor != null && folded.rateAuthor == payment.from)
      PaymentConcern.rateSetByPayer,
    if (payment.paidAtRate != null &&
        (rate == null ||
            payment.paidAtRate!.currency != rate.currency ||
            payment.paidAtRate!.minorUnitsPerZec != rate.minorUnitsPerZec))
      PaymentConcern.pricedAtAnotherRate,
    if (pricedAt != null &&
        live != null &&
        pricedAt.currency == payment.currency &&
        rateFarFromLive(pricedAt.minorUnitsPerZec, live))
      PaymentConcern.rateFarFromLive,
  ];
}
