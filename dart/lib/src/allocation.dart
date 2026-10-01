/// Largest-remainder allocation (SPEC.md §3).
library;

import 'errors.dart';
import 'money.dart';

/// Distributes [total] across [weights] so the parts sum to it exactly.
///
/// Leftover units go to the largest fractional remainders, ties to the lower
/// index. The result is a function of the inputs alone, which is what lets two
/// devices split one expense without exchanging anything.
List<int> allocate(int total, List<int> weights) {
  // 1. The weight sum must be greater than zero.
  if (weights.isEmpty) {
    raise(SplitCode.emptyWeights, 'Allocating across no weights');
  }
  for (final w in weights) {
    if (w < 0) {
      raise(SplitCode.negativeWeight, 'A weight of $w is negative');
    }
  }

  // 2. The sum is formed before any product, so it is the first thing that can
  //    wrap. A wrapped sum is negative and every part then divides to a
  //    plausible wrong number.
  final weightSum = checkedSum(weights, SplitCode.weightSumOverflow);
  if (weightSum == 0) {
    raise(SplitCode.zeroWeightSum, 'Every weight is zero');
  }

  // 3. The most negative integer has no positive counterpart.
  if (total == minAmount) {
    raise(SplitCode.allocationOverflow,
        'A total of $total has no magnitude in a 64-bit integer');
  }

  final negative = total < 0;
  final magnitude = total.abs();

  final parts = List<int>.filled(weights.length, 0);
  final remainders = List<int>.filled(weights.length, 0);
  var distributed = 0;

  for (var i = 0; i < weights.length; i++) {
    // 4. Every product is exact: each factor is below 2^63, so the product is
    //    below 2^126. The part is at most `magnitude` and the remainder below
    //    `weightSum`, so both fit back in 64 bits.
    final w = weights[i];
    if (w == 0 || magnitude <= maxAmount ~/ w) {
      final scaled = magnitude * w;
      parts[i] = scaled ~/ weightSum;
      remainders[i] = scaled % weightSum;
    } else {
      final scaled = BigInt.from(magnitude) * BigInt.from(w);
      final sum = BigInt.from(weightSum);
      parts[i] = (scaled ~/ sum).toInt();
      remainders[i] = (scaled % sum).toInt();
    }
    distributed += parts[i];
  }

  // 5 and 6. Each part discards a fraction below one, so the leftover is less
  //    than the number of weights. Checked rather than assumed: the
  //    distribution hands out one unit per index and would double-credit if it
  //    did not hold.
  final leftover = magnitude - distributed;
  if (leftover < 0 || leftover >= weights.length) {
    raise(
        SplitCode.allocationOverflow,
        'A leftover of $leftover cannot be distributed across '
        '${weights.length} weights');
  }

  final order = List<int>.generate(weights.length, (i) => i)
    ..sort((a, b) {
      final byRemainder = remainders[b].compareTo(remainders[a]);
      return byRemainder != 0 ? byRemainder : a.compareTo(b);
    });
  for (var i = 0; i < leftover; i++) {
    parts[order[i]] += 1;
  }

  // 7. Restore the sign.
  if (negative) {
    for (var i = 0; i < parts.length; i++) {
      parts[i] = -parts[i];
    }
  }
  return parts;
}

/// [total] split evenly [count] ways. Earlier indices absorb the extra units.
List<int> allocateEvenly(int total, int count) =>
    allocate(total, List<int>.filled(count, 1));
