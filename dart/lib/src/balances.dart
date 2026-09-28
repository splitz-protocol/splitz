/// Net positions and the debts as they arose (SPEC.md §5).
library;

import 'errors.dart';
import 'model.dart';
import 'money.dart';
import 'ordering.dart';
import 'split.dart';

/// Whether a set of balances sums to zero (§5.1), decided by cancellation.
///
/// A running total, or the sum of the positives, forms a value the set need
/// not contain: both exceed a signed 64-bit integer for sets whose residual is
/// zero and whose every member is representable. Cancelling largest against
/// largest never forms one, and the answer does not depend on the order a map
/// happens to yield.
bool residualIsZero(Iterable<int> values) {
  final owed = [
    for (final v in values)
      if (v > 0) v
  ]..sort((a, b) => b - a);
  final owes = [
    for (final v in values)
      if (v < 0) -v
  ]..sort((a, b) => b - a);
  var i = 0, j = 0;
  var carryOwed = 0, carryOwes = 0;
  while ((i < owed.length || carryOwed > 0) &&
      (j < owes.length || carryOwes > 0)) {
    final a = carryOwed > 0 ? carryOwed : owed[i++];
    final b = carryOwes > 0 ? carryOwes : owes[j++];
    carryOwed = a > b ? a - b : 0;
    carryOwes = b > a ? b - a : 0;
  }
  return i == owed.length &&
      j == owes.length &&
      carryOwed == 0 &&
      carryOwes == 0;
}

/// A participant and what the bill owes them, or they it.
class Position {
  const Position(this.id, this.amount);
  final String id;
  final int amount;
}

/// One debt, as it arose, before any netting.
class DirectDebt {
  const DirectDebt(this.from, this.to, this.amount);
  final String from;
  final String to;
  final int amount;
}

/// Net balances for every participant, including those who net to zero.
///
/// Positive means the bill owes the participant. Only a **confirmed** payment
/// (§10.5) moves a balance: a payment recorded and not yet confirmed is a
/// claim, and the person who owes the money is the one making it.
Map<String, int> netBalances(Bill bill) {
  final net = <String, int>{for (final p in bill.participants) p.id: 0};

  for (final e in bill.expenses) {
    final shares = splitExpense(e.amount, e.split);
    for (final id in shares.keys) {
      if (!net.containsKey(id)) {
        raise(SplitCode.unknownParticipant,
            'An expense splits to $id, who is not on this bill');
      }
    }
    net[e.paidBy] = checkedBalance(checkedAdd(net[e.paidBy]!, e.amount));
    shares.forEach((id, owed) {
      net[id] = checkedBalance(checkedSubtract(net[id]!, owed));
    });
  }

  for (final p in bill.payments) {
    if (!bill.confirmedPayments.contains(p.id)) continue;
    net[p.from] = checkedBalance(checkedAdd(net[p.from]!, p.amount));
    net[p.to] = checkedBalance(checkedSubtract(net[p.to]!, p.amount));
  }

  // An expense moves this sum by zero because every split sums to its total,
  // and a payment credits and debits the same amount. A non-zero residual is
  // this implementation's own arithmetic having gone wrong, not a malformed
  // input, which is why SPEC.md §12 names it as the one code with no vector.
  // The residual is a property of the set, not of an accumulation order: a
  // running total can exceed a signed 64-bit integer at some orderings of a
  // set whose total is zero, and the order a map yields is the
  // implementation's, not the document's.
  if (!residualIsZero(net.values)) {
    raise(SplitCode.balancesNonzeroResidual, 'Net balances do not sum to zero');
  }
  return net;
}

/// The positive balances, most owed first, ties by ascending id.
List<Position> creditors(Map<String, int> net) => _ranked(net, positive: true);

/// The negative balances, largest debt first, ties by ascending id.
List<Position> debtors(Map<String, int> net) => _ranked(net, positive: false);

List<Position> _ranked(Map<String, int> net, {required bool positive}) {
  final rows = [
    for (final id in sortedUtf8(net.keys))
      if (positive ? net[id]! > 0 : net[id]! < 0) Position(id, net[id]!),
  ];
  rows.sort((a, b) {
    final byMagnitude =
        positive ? b.amount.compareTo(a.amount) : a.amount.compareTo(b.amount);
    return byMagnitude != 0 ? byMagnitude : compareUtf8(a.id, b.id);
  });
  return rows;
}

/// The debts the bill created, before netting (§5.2).
///
/// Recorded payments are not subtracted: §6.3 reads these to explain a
/// rerouted payment.
List<DirectDebt> directDebts(Bill bill) {
  // Keyed by debtor then creditor. Nested rather than by a joined string, so
  // no separator can collide with a participant id.
  final pairs = <String, Map<String, int>>{};
  for (final e in bill.expenses) {
    final shares = splitExpense(e.amount, e.split);
    shares.forEach((id, owed) {
      if (id == e.paidBy || owed == 0) return;
      final row = pairs.putIfAbsent(id, () => <String, int>{});
      row[e.paidBy] = checkedAdd(row[e.paidBy] ?? 0, owed);
    });
  }

  final rows = <DirectDebt>[];
  for (final debtor in sortedUtf8(pairs.keys)) {
    for (final creditor in sortedUtf8(pairs[debtor]!.keys)) {
      final amount = pairs[debtor]![creditor]!;
      if (amount != 0) rows.add(DirectDebt(debtor, creditor, amount));
    }
  }
  return rows;
}
