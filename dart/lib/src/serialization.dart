/// Decoding a bill (SPEC.md §9).
///
/// A field of the wrong type is refused, optional or not: a wrong-typed
/// `payTo` is the address money is sent to, and an `extraMinorUnits` silently
/// defaulted to zero makes §4.5's total check pass on a bill whose tax has
/// vanished.
library;

import 'errors.dart';
import 'instant.dart';
import 'model.dart';
import 'money.dart';
import 'split.dart';
import 'ordering.dart';
import 'rate.dart';

/// The wire format version this library writes and the highest it reads.
const int billVersion = 1;

/// The split modes a bill may declare (§9.1).
const Set<String> billSplitModes = {'equal', 'percentage'};
const Set<String> _payoutTypes = {'zec', 'swap', 'cash'};
const Set<String> _settlementMethods = {'shieldedZec', 'swap', 'cash'};

/// Decodes a bill document.
Bill decodeBill(Object? doc) {
  if (doc is! Map) {
    raise(SplitCode.billTypeError, 'A bill is an object, got $doc');
  }
  // §2.3: every string the ordering is defined over is a sequence of Unicode
  // scalar values, checked before any of them is compared or encoded.
  checkScalarValues(doc);
  final map = doc.cast<String, dynamic>();

  // A version written as a string is not a version: a reader that skips the
  // check when the type is wrong lets a future format present itself as this
  // one.
  if (!map.containsKey('v')) {
    raise(SplitCode.billMissingVersion, 'A bill states its format version');
  }
  final version = map['v'];
  if (version is! int) {
    raise(SplitCode.billMissingVersion,
        'A version is an integer, got ${version.runtimeType}');
  }
  if (version < 1) {
    raise(SplitCode.billTypeError, 'A version is at least 1, got $version');
  }
  if (version > billVersion) {
    raise(SplitCode.billFutureVersion,
        'This bill is version $version; this reader implements $billVersion');
  }

  final currency = map['currency'];
  if (currency == null || currency == '') {
    raise(SplitCode.billMissingCurrency, 'A bill states its currency');
  }
  checkCurrency(currency);
  final billCurrency = currency as String;

  // §9.1. An optional scalar does not read `null` as absent.
  final mode = map.containsKey('splitMode') ? map['splitMode'] : 'equal';
  if (mode is! String) {
    raise(SplitCode.billTypeError, 'A split mode is a string, got $mode');
  }
  if (!billSplitModes.contains(mode)) {
    raise(SplitCode.billUnknownSplitMode, 'No such split mode: "$mode"');
  }

  final participants = <Participant>[];
  final ids = <String>{};
  for (final raw in _list(map['participants'])) {
    final participant = decodeParticipant(raw);
    if (!ids.add(participant.id)) {
      raise(SplitCode.duplicateParticipant,
          'Two participants share the id "${participant.id}"');
    }
    participants.add(participant);
  }

  final expenses = <Expense>[];
  for (final raw in _list(map['expenses'])) {
    expenses.add(decodeExpense(raw, billCurrency, ids));
  }

  final payments = <PaymentRecord>[];
  for (final raw in _list(map['payments'])) {
    payments.add(decodePayment(raw, billCurrency, ids));
  }

  ExchangeRate? rate;
  if (map.containsKey('rate')) {
    final r = map['rate'];
    if (r is! Map) {
      raise(SplitCode.billTypeError, 'A rate is an object, got $r');
    }
    rate = decodeRate(r);
  }

  final rawConfirmed = map['confirmedPayments'];
  if (rawConfirmed != null && rawConfirmed is! List) {
    raise(SplitCode.billTypeError, 'confirmedPayments is a list of ids');
  }
  final confirmedPayments = <String>{
    for (final id in (rawConfirmed as List?) ?? const [])
      if (id is String)
        id
      else
        raise(SplitCode.billTypeError, 'A confirmed payment id is a string'),
  };

  return Bill(
    id: map.containsKey('id') ? _string(map['id']) : '',
    name: map.containsKey('name') ? _string(map['name']) : '',
    currency: billCurrency,
    splitMode: mode,
    participants: participants,
    expenses: expenses,
    payments: payments,
    // §9.1. Absent means nothing is confirmed, never everything: reading it
    // the other way settles a debt on the debtor's own unconfirmed claim.
    confirmedPayments: confirmedPayments,
    rate: rate,
  );
}

/// Decodes one participant payload (§9.1).
///
/// Shared with the fold, which applies it to each `joinBill` before the
/// participant reaches a bill document: a member the decoder would refuse must
/// set its entry aside (§10.3), never make the whole document undecodable.
Participant decodeParticipant(Object? raw) {
  final p = _object(raw);
  final id = _string(p['id']);
  // §9.1. An empty id is not a name anyone can be settled to, and two
  // readers disagreeing about it fold different bills from one log.
  if (id.isEmpty) {
    raise(SplitCode.billBadParticipantId, 'A participant states an id');
  }
  return Participant(
    id: id,
    name: p.containsKey('name') ? _string(p['name']) : '',
    payTo: p.containsKey('payTo') ? _string(p['payTo']) : null,
    identityKey:
        p.containsKey('identityKey') ? _string(p['identityKey']) : null,
    payouts: [
      for (final rawPayout in _list(p['payouts'])) _payout(rawPayout),
    ],
  );
}

/// Decodes one expense payload (§9.1), against the ids already on the bill.
Expense decodeExpense(Object? raw, String billCurrency, Set<String> ids) {
  final e = _object(raw);
  final own = _currencyOf(e, billCurrency);
  final paidBy = e['paidBy'];
  if (paidBy is! String || !ids.contains(paidBy)) {
    raise(SplitCode.unknownParticipant,
        'An expense is paid by $paidBy, who is not on this bill');
  }
  final split = e['split'];
  if (split is! Map) {
    raise(SplitCode.billTypeError, 'An expense states how it splits');
  }
  // Every id a split names must be on the bill, not just `paidBy`. Without
  // this an expense splitting to a stranger is decoded, folded and kept, and
  // the refusal surfaces from `netBalances` on a bill that already looks
  // whole — §10.8 rests on the fold being unable to apply such an entry.
  for (final id in splitParticipants(split.cast<String, dynamic>())) {
    if (!ids.contains(id)) {
      raise(SplitCode.unknownParticipant,
          'An expense splits to $id, who is not on this bill');
    }
  }
  final expense = Expense(
    id: _string(e['id']),
    description: e.containsKey('description') ? _string(e['description']) : '',
    paidBy: paidBy,
    amount: _integer(e['amount']),
    currency: own,
    at: canonicalInstant(e['at']),
    split: split.cast<String, dynamic>(),
  );
  // §2.2. Checked once the payload has decoded, so an entry wrong in two ways
  // is refused for the same one everywhere.
  if (expense.amount > maxEntryAmount || expense.amount < -maxEntryAmount) {
    raise(SplitCode.amountTooLarge,
        'An expense of ${expense.amount} is past the $maxEntryAmount cap');
  }
  return expense;
}

/// Decodes one payment payload (§9.2), against the ids already on the bill.
PaymentRecord decodePayment(Object? raw, String billCurrency, Set<String> ids) {
  final p = _object(raw);
  final own = _currencyOf(p, billCurrency);
  final from = p['from'];
  final to = p['to'];
  if (from is! String ||
      !ids.contains(from) ||
      to is! String ||
      !ids.contains(to)) {
    raise(SplitCode.unknownParticipant,
        'A payment names somebody who is not on this bill');
  }
  if (from == to) {
    raise(SplitCode.selfPayment, '$from cannot pay themselves');
  }
  final method = p['method'];
  if (method is! String || !_settlementMethods.contains(method)) {
    raise(SplitCode.billUnknownSettlementMethod,
        'No such settlement method: $method');
  }
  final amount = _integer(p['amount']);
  if (amount < 0) {
    raise(SplitCode.negativeAmount, 'A payment of $amount is negative');
  }

  int? zatoshi;
  if (p.containsKey('zatoshi')) {
    zatoshi = _integer(p['zatoshi']);
    if (zatoshi <= 0) {
      raise(SplitCode.negativeAmount,
          'A payment sends more than nothing, got $zatoshi');
    }
  }

  ExchangeRate? paidAt;
  if (p.containsKey('paidAtRate')) {
    final r = p['paidAtRate'];
    if (r is! Map) {
      raise(SplitCode.billTypeError, 'A rate is an object, got $r');
    }
    paidAt = decodeRate(r);
    // Checked against the currency the payment states, never one it
    // inherited: a rate in another currency restates the debt at an
    // unrelated number rather than pricing it.
    if (paidAt.currency != own) {
      raise(SplitCode.rateCurrencyMismatch,
          'A rate in ${paidAt.currency} does not price a payment in $own');
    }
  }

  return PaymentRecord(
    id: _string(p['id']),
    from: from,
    to: to,
    amount: amount,
    currency: own,
    method: method,
    at: canonicalInstant(p['at']),
    zatoshi: zatoshi,
    paidAtRate: paidAt,
    reference: p.containsKey('reference') ? _string(p['reference']) : null,
    note: p.containsKey('note') ? _string(p['note']) : null,
  );
}

/// Every payload carrying an amount states its own currency, so it decodes on
/// its own. A reader falls back to the enclosing bill's when the field is
/// absent (§9.1).
String _currencyOf(Map<String, dynamic> payload, String fallback) {
  if (!payload.containsKey('currency')) return fallback;
  final own = payload['currency'];
  checkCurrency(own);
  if (own != fallback) {
    raise(SplitCode.currencyMismatch,
        'An amount in $own is on a bill denominated in $fallback');
  }
  return own as String;
}

Payout _payout(Object? raw) {
  final p = _object(raw);
  final type = p['type'];
  // Refused rather than skipped: skipping settles to the next preference
  // down, which is a different address.
  if (type is! String || !_payoutTypes.contains(type)) {
    raise(SplitCode.billUnknownPayoutMethod, 'No such payout method: $type');
  }
  return Payout(
    type: type,
    address: p.containsKey('address') ? _string(p['address']) : null,
    asset: p.containsKey('asset') ? _string(p['asset']) : null,
    chain: p.containsKey('chain') ? _string(p['chain']) : null,
  );
}

/// Decodes one exchange rate (§7).
///
/// Shared with the fold, which applies it to each `setRate` before the rate
/// reaches a bill document: a member the decoder would refuse sets its entry
/// aside (§10.3) rather than making the whole document undecodable.
///
/// §7 makes only `source` optional, so `at` is required here. A rate with no
/// instant prices an expense at a moment nobody stated.
ExchangeRate decodeRate(Object? raw) {
  final r = _object(raw);
  checkCurrency(r['currency']);
  final per = _integer(r['minorUnitsPerZec']);
  if (per <= 0) {
    raise(SplitCode.rateNotPositive, 'A rate of $per is not positive');
  }
  return ExchangeRate(
    currency: r['currency'] as String,
    minorUnitsPerZec: per,
    at: canonicalInstant(r['at']),
    source: r.containsKey('source') ? _string(r['source']) : null,
  );
}

List<Object?> _list(Object? value) {
  if (value == null) return const [];
  if (value is! List) {
    raise(SplitCode.billTypeError, 'Expected a list, got $value');
  }
  return value;
}

Map<String, dynamic> _object(Object? value) {
  if (value is! Map) {
    raise(SplitCode.billTypeError, 'Expected an object, got $value');
  }
  return value.cast<String, dynamic>();
}

String _string(Object? value) {
  if (value is! String) {
    raise(SplitCode.billTypeError, 'Expected a string, got $value');
  }
  return value;
}

int _integer(Object? value) {
  if (value is! int) {
    raise(SplitCode.billTypeError, 'Expected an integer, got $value');
  }
  return value;
}

// --- the encoder ------------------------------------------------------------

/// Writes [bill] as a §9 document.
///
/// **Every field [decodeBill] reads, this writes.** A field the decoder
/// admits and the encoder drops is a value that survives one hop and vanishes
/// on the next — a swap's `reference` becoming unreadable after a re-share, a
/// snapshotted rate silently re-looked-up per device.
///
/// An absent optional is left out rather than written as null: §9.3's
/// canonical form has no null, and a reader that admitted one would be
/// admitting a shape this never emits.
Map<String, dynamic> billToJson(Bill bill) => <String, dynamic>{
      'v': billVersion,
      'id': bill.id,
      'name': bill.name,
      'currency': bill.currency,
      'splitMode': bill.splitMode,
      'participants': [
        for (final p in bill.participants) participantToJson(p),
      ],
      'expenses': [for (final e in bill.expenses) expenseToJson(e)],
      'payments': [for (final p in bill.payments) paymentToJson(p)],
      'confirmedPayments': bill.confirmedPayments.toList(),
      if (bill.rate != null) 'rate': rateToJson(bill.rate!),
    };

/// Writes a participant, including the payout preferences in their order:
/// §9.1 makes the order the preference order.
Map<String, dynamic> participantToJson(Participant p) => <String, dynamic>{
      'id': p.id,
      'name': p.name,
      if (p.payTo != null) 'payTo': p.payTo,
      if (p.identityKey != null) 'identityKey': p.identityKey,
      if (p.payouts.isNotEmpty)
        'payouts': [for (final o in p.payouts) payoutToJson(o)],
    };

/// Writes one payout preference.
Map<String, dynamic> payoutToJson(Payout p) => <String, dynamic>{
      'type': p.type,
      if (p.address != null) 'address': p.address,
      if (p.asset != null) 'asset': p.asset,
      if (p.chain != null) 'chain': p.chain,
    };

/// Writes an expense. `split` is §4's own shape and is passed through
/// untouched.
Map<String, dynamic> expenseToJson(Expense e) => <String, dynamic>{
      'id': e.id,
      'description': e.description,
      'paidBy': e.paidBy,
      'amount': e.amount,
      'currency': e.currency,
      'at': e.at,
      'split': e.split,
    };

/// Writes a payment record, including the advisory halves §9.2 allows:
/// `zatoshi`, `paidAtRate`, the swap `reference` and a `note`.
Map<String, dynamic> paymentToJson(PaymentRecord p) => <String, dynamic>{
      'id': p.id,
      'from': p.from,
      'to': p.to,
      'amount': p.amount,
      'currency': p.currency,
      'method': p.method,
      'at': p.at,
      if (p.zatoshi != null) 'zatoshi': p.zatoshi,
      if (p.paidAtRate != null) 'paidAtRate': rateToJson(p.paidAtRate!),
      if (p.reference != null) 'reference': p.reference,
      if (p.note != null) 'note': p.note,
    };

/// Writes a rate. `source` is the only optional §7 leaves.
Map<String, dynamic> rateToJson(ExchangeRate r) => <String, dynamic>{
      'currency': r.currency,
      'minorUnitsPerZec': r.minorUnitsPerZec,
      'at': r.at,
      if (r.source != null) 'source': r.source,
    };
