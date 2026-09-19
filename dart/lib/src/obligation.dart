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

/// A debt held back because a payment to that participant is unconfirmed
/// (§14.4).
class Awaiting {
  const Awaiting(this.to, this.owed, this.paid);

  final String to;

  /// What the plan still says is owed. An unconfirmed payment does not reduce
  /// it (§10.5).
  final int owed;

  /// What this payer has already sent and is waiting to have confirmed. Less
  /// than [owed] when the payment was partial.
  final int paid;
}

/// A debt held back because two keys each claim that participant's id
/// (§10.7).
class Contested {
  const Contested(this.to, this.amount, this.address);

  final String to;
  final int amount;

  /// The payout address standing on the bill, which may be an impostor's.
  final String? address;
}

/// One payer's settlements, split into what a request may carry and what
/// §14 holds back.
class Withholdings {
  const Withholdings({
    required this.carried,
    required this.awaiting,
    required this.contested,
  });

  /// Safe to render. Still subject to §8.4: a participant here may have no
  /// payout address, which `renderObligation` reports as unpayable.
  final List<Settlement> carried;
  final List<Awaiting> awaiting;
  final List<Contested> contested;
}

/// Splits [payer]'s settlements into what may be requested and what may not.
///
/// Pure: it reads the bill and the identities the fold resolved, and decides
/// nothing a wallet is entitled to decide. [payAnyway] names the contested
/// ids a payer has accepted after being shown them, which §10.7 permits and
/// which is the only way through a contest — anyone may mint a rival claim,
/// so a refusal with no exit is a denial of payment.
Withholdings withholdings(
  List<Settlement> plan,
  Bill bill,
  String payer, {
  Set<String> contestedIds = const {},
  Set<String> payAnyway = const {},
}) {
  final mine = plan.where((s) => s.from == payer);

  // §10.5: only a confirmed payment moves a balance, so a debt this payer has
  // already paid is still in the plan. Several records to one participant sum.
  final pending = <String, int>{};
  for (final p in bill.payments) {
    if (p.from != payer) continue;
    if (bill.confirmedPayments.contains(p.id)) continue;
    pending[p.to] = (pending[p.to] ?? 0) + p.amount;
  }

  final payTo = <String, String?>{
    for (final p in bill.participants) p.id: p.payTo,
  };

  final carried = <Settlement>[];
  final awaiting = <Awaiting>[];
  final contested = <Contested>[];
  for (final s in mine) {
    if (pending.containsKey(s.to)) {
      awaiting.add(Awaiting(s.to, s.amount, pending[s.to]!));
    } else if (contestedIds.contains(s.to) && !payAnyway.contains(s.to)) {
      contested.add(Contested(s.to, s.amount, payTo[s.to]));
    } else {
      carried.add(s);
    }
  }
  return Withholdings(
      carried: carried, awaiting: awaiting, contested: contested);
}
