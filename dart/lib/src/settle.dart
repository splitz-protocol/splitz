/// Settlement (SPEC.md §6).
///
/// Balances are netted, then partitioned into as many zero-sum groups as
/// possible. A group of `k` needs exactly `k−1` payments, so maximising groups
/// minimises the total.
library;

import 'balances.dart';
import 'errors.dart';
import 'model.dart';
import 'money.dart';
import 'ordering.dart';

/// The default number of non-zero participants solved exactly.
const int defaultExactLimit = 14;

/// The ceiling on the exact limit.
///
/// The partition search allocates `2^n` entries and costs `3^n`. The ceiling
/// is normative because the failure past it is silent: at 26 the search does
/// not finish, and at 64 the shift overflows and the search returns an empty
/// plan that still reports itself optimal.
const int maxExactLimit = 20;

/// One payment in a plan.
class Settlement {
  const Settlement(this.from, this.to, this.amount, {this.covers = const []});
  final String from;
  final String to;
  final int amount;

  /// The original debts this payment discharges (§6.3).
  ///
  /// Netting reroutes payments, so a payer is often asked to pay somebody they
  /// never transacted with, and a bill showing only the result cannot explain
  /// it.
  ///
  /// Empty for a plan computed from balances: net balances do not carry the
  /// debts that produced them. [settleBill] populates it.
  final List<DirectDebt> covers;

  /// The part of this payment no direct debt explains (§6.3).
  ///
  /// Anybody holding the invite may write an expense, and §4 admits a negative
  /// total, so a peer can attribute a refund to somebody who never agreed to
  /// it. The victim's settlement then exceeds every debt the bill records for
  /// them, and coverage goes quiet precisely when it is needed.
  ///
  /// Refused with `amount_overflow` when the covers, or the amount less them,
  /// leave the signed 64-bit range: a settlement handed in across a boundary
  /// is not one §6 produced.
  int get unexplained {
    final covered = checkedSum([for (final c in covers) c.amount]);
    // For a plan from balances, where §6.3 requires coverage to be empty,
    // this is the whole amount — the true answer, since no debts were given.
    final rest = checkedSubtract(amount, covered);
    return rest > 0 ? rest : 0;
  }

  /// Whether any part of this payment discharges a debt owed to somebody other
  /// than [to] (§6.3).
  ///
  /// Membership is not the test. A payment covering a little of what the payee
  /// lent and a great deal of what two other people lent is still a payment the
  /// payer cannot account for by looking at the payee.
  bool get isRerouted {
    var elsewhere = 0;
    for (final c in covers) {
      if (c.to != to) elsewhere += c.amount;
    }
    return elsewhere > 0;
  }
}

/// A whole plan.
class SettlementPlan {
  const SettlementPlan(this.settlements, {required this.isOptimal});
  final List<Settlement> settlements;

  /// False when the partition search was not run and the whole set was treated
  /// as one group. A plan that overshoots by a payment is acceptable; one that
  /// claims minimality it has not established is not.
  final bool isOptimal;

  int get paymentCount => settlements.length;
}

/// Plans the fewest payments that clear [net].
SettlementPlan settleBalances(Map<String, int> net,
    {int exactLimit = defaultExactLimit}) {
  if (exactLimit > maxExactLimit) {
    raise(SplitCode.exactLimitTooLarge,
        'An exact limit of $exactLimit exceeds $maxExactLimit');
  }

  // A balance of minAmount has no positive counterpart, so no settlement
  // amount can carry it (§2.2, §8.1).
  for (final v in net.values) {
    if (v == minAmount) {
      raise(SplitCode.amountOverflow,
          'A balance of $v cannot be settled: no payment can carry it');
    }
  }

  // §5.1, decided by cancellation so no value the set does not contain is
  // formed.
  if (!residualIsZero(net.values)) {
    raise(SplitCode.balancesNonzeroResidual, 'Net balances do not sum to zero');
  }

  final ids = [
    for (final id in sortedUtf8(net.keys))
      if (net[id] != 0) id
  ];
  final values = [for (final id in ids) net[id]!];
  final optimal = ids.length <= exactLimit;
  final groups = _zeroSumGroups(values, exactLimit);

  final settlements = <Settlement>[];
  for (final group in groups) {
    final balance = <String, int>{for (final i in group) ids[i]: values[i]};
    while (true) {
      final owing = [
        for (final e in balance.entries)
          if (e.value < 0) e
      ]..sort((a, b) {
          final byMagnitude = a.value.compareTo(b.value);
          return byMagnitude != 0 ? byMagnitude : compareUtf8(a.key, b.key);
        });
      final owed = [
        for (final e in balance.entries)
          if (e.value > 0) e
      ]..sort((a, b) {
          final byMagnitude = b.value.compareTo(a.value);
          return byMagnitude != 0 ? byMagnitude : compareUtf8(a.key, b.key);
        });
      if (owing.isEmpty || owed.isEmpty) break;

      final debtor = owing.first;
      final creditor = owed.first;
      final amount =
          -debtor.value < creditor.value ? -debtor.value : creditor.value;
      settlements.add(Settlement(debtor.key, creditor.key, amount));
      balance[debtor.key] = balance[debtor.key]! + amount;
      balance[creditor.key] = balance[creditor.key]! - amount;
    }
  }

  settlements.sort((a, b) {
    final byPayer = compareUtf8(a.from, b.from);
    return byPayer != 0 ? byPayer : compareUtf8(a.to, b.to);
  });
  return SettlementPlan(settlements, isOptimal: optimal);
}

/// Partitions indices into as many zero-sum groups as possible.
List<List<int>> _zeroSumGroups(List<int> values, int exactLimit) {
  final n = values.length;
  if (n == 0) return const [];
  if (n > exactLimit) {
    return [List<int>.generate(n, (i) => i)];
  }

  final full = (1 << n) - 1;
  // A subset whose sum cannot be formed in a signed 64-bit integer is not
  // zero and not a group — but the bill around it may settle perfectly, so it
  // is skipped rather than refused. Tracked per subset: a bound over the whole
  // set depends on an order whoever joined chose.
  final sums = List<int>.filled(1 << n, 0);
  final exact = List<bool>.filled(1 << n, true);
  for (var mask = 1; mask <= full; mask++) {
    final low = mask & -mask;
    final rest = mask ^ low;
    if (!exact[rest]) {
      exact[mask] = false;
      continue;
    }
    final value = values[low.bitLength - 1];
    final a = sums[rest];
    final total = a + value;
    if ((a > 0 && value > 0 && total < 0) ||
        (a < 0 && value < 0 && total >= 0)) {
      exact[mask] = false;
    } else {
      sums[mask] = total;
    }
  }

  final best = List<int>.filled(1 << n, 0);
  final pick = List<int>.filled(1 << n, 0);
  for (var mask = 1; mask <= full; mask++) {
    final lowest = mask & -mask;
    var sub = mask;
    while (sub != 0) {
      if ((sub & lowest) != 0 && exact[sub] && sums[sub] == 0) {
        final candidate = best[mask ^ sub] + 1;
        if (candidate > best[mask]) {
          best[mask] = candidate;
          pick[mask] = sub;
        }
      }
      sub = (sub - 1) & mask;
    }
    if (pick[mask] == 0) pick[mask] = mask;
  }

  final groups = <List<int>>[];
  var mask = full;
  while (mask != 0) {
    final sub = pick[mask];
    groups.add([
      for (var i = 0; i < n; i++)
        if ((sub >> i) & 1 == 1) i
    ]);
    mask ^= sub;
  }
  return groups;
}

/// Plans the fewest payments that clear [bill], each carrying the original
/// debts it discharges (§6.3).
SettlementPlan settleBill(Bill bill, {int exactLimit = defaultExactLimit}) {
  final plan = settleBalances(netBalances(bill), exactLimit: exactLimit);
  return SettlementPlan(
    attributeCoverage(plan.settlements, directDebts(bill)),
    isOptimal: plan.isOptimal,
  );
}

/// Attributes each settlement to the original debts it discharges (§6.3).
///
/// A payer's direct debts, in their §5.2 order, are consumed against that
/// payer's settlements in plan order, each settlement taking as much of each
/// remaining debt as it needs.
List<Settlement> attributeCoverage(
    List<Settlement> settlements, List<DirectDebt> debts) {
  // Rows are addressed by index into `debts` so that two identical (debtor,
  // creditor, amount) rows stay distinct and are drawn down separately.
  final rowsOf = <String, List<int>>{};
  for (var i = 0; i < debts.length; i++) {
    (rowsOf[debts[i].from] ??= []).add(i);
  }
  final left = [for (final d in debts) d.amount];

  final out = <Settlement>[];
  for (final s in settlements) {
    var need = s.amount;
    final covers = <DirectDebt>[];
    for (final i in rowsOf[s.from] ?? const <int>[]) {
      if (need <= 0) break;
      // A pair aggregating to a negative amount is a credit on that pair.
      if (left[i] <= 0) continue;
      final take = need < left[i] ? need : left[i];
      covers.add(DirectDebt(s.from, debts[i].to, take));
      left[i] -= take;
      need -= take;
    }
    out.add(Settlement(s.from, s.to, s.amount, covers: covers));
  }
  return out;
}
