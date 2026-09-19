/// The bill and the things on it (SPEC.md §9).
library;

import 'rate.dart';

/// How a participant wants to be paid, most preferred first.
///
/// A preference takes no part in any arithmetic; §6 decides who owes what.
/// Honouring one is the wallet's.
class Payout {
  const Payout({required this.type, this.address, this.asset, this.chain});

  /// `zec`, `swap` or `cash`. A reader refuses a type it does not define
  /// rather than skipping it: skipping settles to the next preference down,
  /// which is a different address.
  final String type;
  final String? address;
  final String? asset;
  final String? chain;
}

/// Somebody on the bill.
class Participant {
  const Participant({
    required this.id,
    required this.name,
    this.payTo,
    this.identityKey,
    this.payouts = const [],
  });

  final String id;
  final String name;

  /// Where money is sent. Absent means this participant cannot be paid by a
  /// payment request, which §8.5 requires be reported rather than dropped.
  final String? payTo;

  /// The Ed25519 public key that alone may write as this participant (§10.7).
  /// Absent means the identity is unclaimed.
  final String? identityKey;

  final List<Payout> payouts;

  /// Whether a payment request can carry an output for this participant.
  ///
  /// Returns the address, so a caller cannot reach for one that is not there.
  String? get payableAddress {
    final String? address;
    if (payouts.isNotEmpty) {
      final first = payouts.first;
      address = first.type == 'zec' ? first.address : null;
    } else {
      address = payTo;
    }
    // An empty string is not an address. Returning one sends it into the
    // renderer, which refuses the whole request — past the caller's choice to
    // report an unpayable recipient instead of refusing.
    return (address == null || address.isEmpty) ? null : address;
  }
}

/// A cost somebody covered.
class Expense {
  const Expense({
    required this.id,
    required this.description,
    required this.paidBy,
    required this.amount,
    required this.currency,
    required this.at,
    required this.split,
  });

  final String id;
  final String description;
  final String paidBy;
  final int amount;
  final String currency;
  final String at;
  final Map<String, dynamic> split;
}

/// A claim that a debt was discharged.
///
/// Recording one is a claim; §10.5 decides when it settles anything.
class PaymentRecord {
  const PaymentRecord({
    required this.id,
    required this.from,
    required this.to,
    required this.amount,
    required this.currency,
    required this.method,
    required this.at,
    this.zatoshi,
    this.paidAtRate,
    this.reference,
    this.note,
  });

  final String id;
  final String from;
  final String to;
  final int amount;
  final String currency;

  /// `shieldedZec`, `swap` or `cash`. A label, not a branch: the ledger
  /// arithmetic is identical whichever happened.
  final String method;
  final String at;

  /// What left the payer's wallet. Advisory: the fiat [amount] settles the
  /// debt and this takes no part in §5 or §6.
  final int? zatoshi;
  final ExchangeRate? paidAtRate;

  /// Identifies a swap — the provider's intent id, or the transaction on the
  /// destination chain. Not a Zcash transaction id.
  final String? reference;
  final String? note;
}

/// A bill.
class Bill {
  const Bill({
    required this.id,
    required this.name,
    required this.currency,
    this.splitMode = 'equal',
    this.participants = const [],
    this.expenses = const [],
    this.payments = const [],
    this.confirmedPayments = const {},
    this.rate,
  });

  final String id;
  final String name;

  /// A bill has exactly one currency. Every expense and payment on it is
  /// denominated in that currency (§2.4).
  final String currency;

  /// The default a UI reaches for. It constrains nothing.
  final String splitMode;

  final List<Participant> participants;
  final List<Expense> expenses;
  final List<PaymentRecord> payments;

  /// The ids of the payments §10.5 says are confirmed. A payment not in this
  /// set is a claim and moves no balance.
  final Set<String> confirmedPayments;

  /// Snapshotted, not looked up per device (§7).
  final ExchangeRate? rate;

  Participant? participant(String id) {
    for (final p in participants) {
      if (p.id == id) return p;
    }
    return null;
  }
}
