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
import 'ordering.dart';
import 'rate.dart';

/// The wire format version this library writes and the highest it reads.
const int billVersion = 1;

const Set<String> _splitModes = {'equal', 'percentage'};
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

  final mode = map['splitMode'] ?? 'equal';
  if (mode is! String) {
    raise(SplitCode.billTypeError, 'A split mode is a string, got $mode');
  }
  if (!_splitModes.contains(mode)) {
    raise(SplitCode.billUnknownSplitMode, 'No such split mode: "$mode"');
  }

  final participants = <Participant>[];
  final ids = <String>{};
  for (final raw in _list(map['participants'])) {
    final p = _object(raw);
    final id = _string(p['id']);
    // §9.1. An empty id is not a name anyone can be settled to, and two
    // readers disagreeing about it fold different bills from one log.
    if (id.isEmpty) {
      raise(SplitCode.billBadParticipantId, 'A participant states an id');
    }
    if (!ids.add(id)) {
      raise(SplitCode.duplicateParticipant,
          'Two participants share the id "$id"');
    }
    participants.add(Participant(
      id: id,
      name: p.containsKey('name') ? _string(p['name']) : '',
      payTo: p.containsKey('payTo') ? _string(p['payTo']) : null,
      identityKey:
          p.containsKey('identityKey') ? _string(p['identityKey']) : null,
      payouts: [
        for (final rawPayout in _list(p['payouts'])) _payout(rawPayout),
      ],
    ));
  }

  final expenses = <Expense>[];
  for (final raw in _list(map['expenses'])) {
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
    expenses.add(Expense(
      id: _string(e['id']),
      description:
          e.containsKey('description') ? _string(e['description']) : '',
      paidBy: paidBy,
      amount: _integer(e['amount']),
      currency: own,
      at: canonicalInstant(e['at']),
      split: split.cast<String, dynamic>(),
    ));
  }

  final payments = <PaymentRecord>[];
  for (final raw in _list(map['payments'])) {
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
      paidAt = _rate(r.cast<String, dynamic>());
      // Checked against the currency the payment states, never one it
      // inherited: a rate in another currency restates the debt at an
      // unrelated number rather than pricing it.
      if (paidAt.currency != own) {
        raise(SplitCode.rateCurrencyMismatch,
            'A rate in ${paidAt.currency} does not price a payment in $own');
      }
    }

    payments.add(PaymentRecord(
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
    ));
  }

  ExchangeRate? rate;
  if (map.containsKey('rate')) {
    final r = map['rate'];
    if (r is! Map) {
      raise(SplitCode.billTypeError, 'A rate is an object, got $r');
    }
    rate = _rate(r.cast<String, dynamic>());
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

ExchangeRate _rate(Map<String, dynamic> raw) {
  checkCurrency(raw['currency']);
  return ExchangeRate(
    currency: raw['currency'] as String,
    minorUnitsPerZec: _integer(raw['minorUnitsPerZec']),
    at: raw.containsKey('at') ? _string(raw['at']) : '',
    source: raw.containsKey('source') ? _string(raw['source']) : null,
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
