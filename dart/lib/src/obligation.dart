/// One payer's obligation as a payment request (SPEC.md §8.5).
library;

import 'errors.dart';
import 'model.dart';
import 'money.dart';
import 'rate.dart';
import 'settle.dart';
import 'zip321.dart';

/// A recipient the request cannot carry, and why.
class Unpayable {
  const Unpayable(this.id, this.reason, this.minorUnits);
  final String id;

  /// `no_address` when nothing is published, `payout_not_zec` when the
  /// preferred payout is a swap or cash. The two need different remedies.
  final String reason;
  final int minorUnits;
}

/// What one payer's obligation came to.
///
/// Three groups, because a request that silently covers three of a payer's
/// four debts is indistinguishable, to the payer who sends it, from one that
/// settles all four.
class Obligation {
  const Obligation({
    required this.uri,
    required this.payments,
    required this.unpayable,
    required this.carriedMinorUnits,
    required this.withheldMinorUnits,
  });

  /// Null when nothing could be carried.
  final String? uri;
  final List<Zip321Payment> payments;
  final List<Unpayable> unpayable;

  /// What the URI sends. Never present a figure pricing the whole obligation
  /// as this.
  final int carriedMinorUnits;
  final int withheldMinorUnits;

  bool get isComplete => unpayable.isEmpty;
}

/// Renders one payer's settlements as a payment request.
///
/// With [skipUnpayable] the payable outputs are rendered and the rest
/// reported; without it a recipient the request cannot carry refuses the whole
/// request. An implementation must not do neither.
Obligation renderObligation(
  List<Settlement> settlements,
  Bill bill, {
  required ExchangeRate rate,
  bool skipUnpayable = false,
  bool includeFiat = false,
}) {
  final payments = <Zip321Payment>[];
  final unpayable = <Unpayable>[];
  var carried = 0;
  var withheld = 0;

  for (final s in settlements) {
    final who = bill.participant(s.to);
    if (who == null) {
      // A merge or storage fault, needing a different remedy from a missing
      // address.
      raise(SplitCode.unknownParticipant,
          'The plan settles to ${s.to}, who is not on this bill');
    }
    final address = who.payableAddress;
    if (address == null) {
      final reason = who.payouts.isNotEmpty ? 'payout_not_zec' : 'no_address';
      if (!skipUnpayable) {
        raise(SplitCode.zip321NoAddress,
            '${s.to} has published no address this request can carry');
      }
      unpayable.add(Unpayable(s.to, reason, s.amount));
      withheld = checkedAdd(withheld, s.amount);
      continue;
    }
    payments.add(Zip321Payment(
      address: address,
      zatoshi: fiatToZatoshi(s.amount, rate, amountCurrency: bill.currency),
      fiat: FiatPrice(bill.currency, s.amount),
      label: who.name,
    ));
    carried = checkedAdd(carried, s.amount);
  }

  return Obligation(
    uri:
        payments.isEmpty ? null : renderUri(payments, includeFiat: includeFiat),
    payments: payments,
    unpayable: unpayable,
    carriedMinorUnits: carried,
    withheldMinorUnits: withheld,
  );
}
