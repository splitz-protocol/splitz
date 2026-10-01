/// One payer's obligation as a payment request (SPEC.md §8.5).
library;

import 'dart:convert';

import 'address.dart';
import 'errors.dart';
import 'model.dart';
import 'ordering.dart';
import 'money.dart';
import 'rate.dart';
import 'settle.dart';
import 'zip321.dart';

/// A recipient the request cannot carry, and why.
class Unpayable {
  const Unpayable(this.id, this.reason, this.minorUnits);
  final String id;

  /// `no_address` when nothing is published, `bad_address` when what is
  /// published is not an address §8.3 admits, `payout_not_zec` when the
  /// preferred payout is a swap or cash, `unpriceable` when the debt is past
  /// what one request can price at this rate. Each needs a different remedy.
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
    required this.recipients,
    required this.unpayable,
    required this.carriedMinorUnits,
    required this.withheldMinorUnits,
  });

  /// Null when nothing could be carried.
  final String? uri;
  final List<Zip321Payment> payments;

  /// The participant each of [payments] pays, in the same order.
  final List<String> recipients;
  final List<Unpayable> unpayable;

  /// What the URI sends. Never present a figure pricing the whole obligation
  /// as this.
  final int carriedMinorUnits;
  final int withheldMinorUnits;

  bool get isComplete => unpayable.isEmpty;
}

/// The refusals one output's size produces (§7.1, §8.1, §8.4).
const Set<String> _unpriceable = {
  SplitCode.rateAmountTooLarge,
  SplitCode.zip321AmountTooLarge,
  SplitCode.zip321FiatTooManyDigits,
};

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
  bool Function(String address)? readsAddress,
}) {
  final payments = <Zip321Payment>[];
  final recipients = <String>[];
  final unpayable = <Unpayable>[];
  var carried = 0;
  var withheld = 0;

  // §8.5: a request carries one payer's debts. Built from a whole plan it
  // would ask this payer to send every other payer's too.
  if (settlements.map((s) => s.from).toSet().length > 1) {
    raise(SplitCode.obligationMixedPayers,
        'These settlements are owed by more than one payer');
  }

  for (final s in settlements) {
    final who = bill.participant(s.to);
    if (who == null) {
      // A merge or storage fault, needing a different remedy from a missing
      // address.
      raise(SplitCode.unknownParticipant,
          'The plan settles to ${s.to}, who is not on this bill');
    }
    // §14.6: an address the payer's own reader cannot read is one the whole
    // request fails on, so it is reported like any other it cannot carry.
    final payable = who.payableAddress;
    final address = payable != null && (readsAddress?.call(payable) ?? true)
        ? payable
        : null;
    if (address == null) {
      final published = who.publishedAddress;
      final bad = published != null && published.isNotEmpty;
      final reason = bad
          ? 'bad_address'
          : who.payouts.isNotEmpty
              ? 'payout_not_zec'
              : 'no_address';
      if (!skipUnpayable) {
        raise(bad ? SplitCode.zip321BadAddress : SplitCode.zip321NoAddress,
            '${s.to} has published no address this request can carry');
      }
      unpayable.add(Unpayable(s.to, reason, s.amount));
      withheld = checkedAdd(withheld, s.amount);
      continue;
    }
    // Each output is priced, and checked against what §8 renders, on its own:
    // one debt past what a request can carry is that debt's to report, not a
    // reason to carry none of the others.
    final int zatoshi;
    try {
      zatoshi = fiatToZatoshi(s.amount, rate, amountCurrency: bill.currency);
      renderAmount(zatoshi);
      if (includeFiat) renderFiat(FiatPrice(bill.currency, s.amount));
    } on SplitError catch (err) {
      // Only the refusals one output's size produces. One about the rate
      // itself refuses every output alike and is raised.
      if (!skipUnpayable || !_unpriceable.contains(err.code)) rethrow;
      unpayable.add(Unpayable(s.to, 'unpriceable', s.amount));
      withheld = checkedAdd(withheld, s.amount);
      continue;
    }
    payments.add(Zip321Payment(
      address: address,
      zatoshi: zatoshi,
      fiat: FiatPrice(bill.currency, s.amount),
      // §8.5: what ties the send to this bill, where the address takes one.
      memo: _takesMemo(address) ? billMemo(bill.id) : null,
      label: who.name,
    ));
    recipients.add(s.to);
    carried = checkedAdd(carried, s.amount);
  }

  return Obligation(
    uri:
        payments.isEmpty ? null : renderUri(payments, includeFiat: includeFiat),
    payments: payments,
    recipients: recipients,
    unpayable: unpayable,
    carriedMinorUnits: carried,
    withheldMinorUnits: withheld,
  );
}

/// [bill] with each participant [via] names paid by the payout chosen for
/// them (§14.8).
///
/// [via] maps a participant id to the index of one of their declared payouts.
/// That payout moves to the front and the rest keep their order; everything
/// else on the bill is unchanged. Rendering a request from the result carries
/// a chosen `zec` payout's address. Ids are checked in §2.3's order: one not
/// on the bill is refused with `unknown_participant`, an index outside what
/// that participant declared with `payout_not_declared`.
Bill choosePayouts(Bill bill, Map<String, int> via) {
  if (via.isEmpty) return bill;
  for (final id in sortedUtf8(via.keys)) {
    final who = bill.participant(id);
    if (who == null) {
      raise(SplitCode.unknownParticipant, '$id is not on this bill');
    }
    final index = via[id]!;
    if (index < 0 || index >= who.payouts.length) {
      raise(SplitCode.payoutNotDeclared,
          '$id declared ${who.payouts.length} payouts, not one at $index');
    }
  }
  return Bill(
    id: bill.id,
    name: bill.name,
    currency: bill.currency,
    splitMode: bill.splitMode,
    participants: [
      for (final p in bill.participants)
        if (via[p.id] case final index?)
          Participant(
            id: p.id,
            name: p.name,
            payTo: p.payTo,
            identityKey: p.identityKey,
            payouts: [
              p.payouts[index],
              for (var i = 0; i < p.payouts.length; i++)
                if (i != index) p.payouts[i],
            ],
          )
        else
          p,
    ],
    expenses: bill.expenses,
    payments: bill.payments,
    confirmedPayments: bill.confirmedPayments,
    rate: bill.rate,
  );
}

/// A debt held back because a payment to that participant is unconfirmed
/// (§14.4).
class Awaiting {
  const Awaiting(this.to, this.owed, this.paid, this.paidTo);

  /// Who the plan says is owed.
  final String to;

  /// What the plan still says is owed. An unconfirmed payment does not reduce
  /// it (§10.5).
  final int owed;

  /// What this payer has already sent and is waiting to have confirmed. Less
  /// than [owed] when the payment was partial.
  final int paid;

  /// Who that unconfirmed money went to, in ascending id order. Not [to] when
  /// netting rerouted the debt (§6.3): the payment to confirm, or to take
  /// back, is theirs.
  final List<String> paidTo;
}

/// One payer's settlements, split into what a request may carry and what
/// §14 holds back.
class Withholdings {
  const Withholdings({
    required this.carried,
    required this.awaiting,
  });

  /// Safe to render. Still subject to §8.4: a participant here may have no
  /// payout address, which `renderObligation` reports as unpayable.
  final List<Settlement> carried;
  final List<Awaiting> awaiting;
}

/// Splits [payer]'s settlements into what may be requested and what §14.4
/// holds back.
///
/// Pure: it reads the bill, and decides nothing a wallet is entitled to
/// decide. Given [recordedBy] — the fold's author of each payment record —
/// only a record the payer wrote withholds anything.
Withholdings withholdings(
  List<Settlement> plan,
  Bill bill,
  String payer, {
  Map<String, String>? recordedBy,
}) {
  final mine = plan.where((s) => s.from == payer);

  // §10.5: only a confirmed payment moves a balance, so a debt this payer has
  // already paid is still in the plan. Several records to one participant sum.
  final pending = <String, int>{};
  for (final p in bill.payments) {
    if (p.from != payer) continue;
    if (bill.confirmedPayments.contains(p.id)) continue;
    // A record somebody else wrote is their word, not a payment this payer
    // has in flight.
    if (recordedBy != null && recordedBy[p.id] != payer) continue;
    pending[p.to] = checkedAdd(pending[p.to] ?? 0, p.amount);
  }

  final carried = <Settlement>[];
  final awaiting = <Awaiting>[];
  for (final s in mine) {
    // The payee, and every creditor whose debt this settlement covers (§6.3):
    // netting can reroute a debt already paid onto somebody else.
    final owedTo = {s.to, for (final c in s.covers) c.to};
    final paidTo = sortedUtf8([
      for (final t in owedTo)
        if (pending.containsKey(t)) t
    ]);
    if (paidTo.isNotEmpty) {
      awaiting.add(Awaiting(s.to, s.amount,
          checkedSum([for (final t in paidTo) pending[t]!]), paidTo));
    } else {
      carried.add(s);
    }
  }
  return Withholdings(carried: carried, awaiting: awaiting);
}

/// The memo a request carries to every output that takes one (§8.5): the
/// UTF-8 bytes of `splitz:` and the bill's id. The payee's wallet reads it
/// back to tell a payment for this bill from one sent for anything else
/// (§14.7).
List<int> billMemo(String billId) => utf8.encode('splitz:$billId');

/// Whether [address] decodes (§8.6) to one that can receive a memo. One that
/// does not decode carries none: §8.3 admits strings no reader decodes, and
/// a request to one still renders.
bool _takesMemo(String address) {
  try {
    return parseAddress(address).canReceiveMemo;
  } on SplitError {
    return false;
  }
}
