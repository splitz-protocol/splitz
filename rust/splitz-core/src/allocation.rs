//! Largest-remainder allocation (SPEC.md §3).

use crate::error::{code, Result, SplitError};
use crate::money::{checked_sum, MIN_AMOUNT};

/// Distributes `total` across `weights` so the parts sum to it exactly.
///
/// Leftover units go to the largest fractional remainders, ties to the lower
/// index. The result is a function of the inputs alone, which is what lets two
/// devices split one expense without exchanging anything.
pub fn allocate(total: i64, weights: &[i64]) -> Result<Vec<i64>> {
    // 1. The weight sum must be greater than zero.
    if weights.is_empty() {
        return Err(SplitError::new(
            code::EMPTY_WEIGHTS,
            "Allocating across no weights",
        ));
    }
    if let Some(negative) = weights.iter().find(|w| **w < 0) {
        return Err(SplitError::new(
            code::NEGATIVE_WEIGHT,
            format!("A weight of {negative} is negative"),
        ));
    }

    // 2. The sum is formed before any product, so it is the first value that
    //    can wrap. A wrapped sum is negative and every part then divides to a
    //    plausible wrong number.
    let weight_sum = checked_sum(weights.iter().copied(), code::WEIGHT_SUM_OVERFLOW)?;
    if weight_sum == 0 {
        return Err(SplitError::new(
            code::ZERO_WEIGHT_SUM,
            "Every weight is zero",
        ));
    }

    // 3. The most negative integer has no positive counterpart.
    if total == MIN_AMOUNT {
        return Err(SplitError::new(
            code::ALLOCATION_OVERFLOW,
            format!("A total of {total} has no magnitude in a 64-bit integer"),
        ));
    }

    let negative = total < 0;
    let magnitude = total.abs();

    let mut parts = vec![0i64; weights.len()];
    let mut remainders = vec![0i64; weights.len()];
    let mut distributed: i64 = 0;

    for (i, &weight) in weights.iter().enumerate() {
        // 4. Every product is exact: each factor is below 2^63, so the
        //    product is below 2^126. The part is at most `magnitude` and the
        //    remainder below `weight_sum`, so both fit back in 64 bits.
        let scaled = i128::from(magnitude) * i128::from(weight);
        parts[i] = (scaled / i128::from(weight_sum)) as i64;
        remainders[i] = (scaled % i128::from(weight_sum)) as i64;
        distributed += parts[i];
    }

    // 5 and 6. Each part discards a fraction below one, so the leftover is
    //    less than the number of weights. Checked rather than assumed: the
    //    distribution hands out one unit per index and would double-credit if
    //    it did not hold.
    let leftover = magnitude - distributed;
    if leftover < 0 || leftover >= weights.len() as i64 {
        return Err(SplitError::new(
            code::ALLOCATION_OVERFLOW,
            format!(
                "A leftover of {leftover} cannot be distributed across {} weights",
                weights.len()
            ),
        ));
    }

    let mut order: Vec<usize> = (0..weights.len()).collect();
    order.sort_by(|&a, &b| remainders[b].cmp(&remainders[a]).then(a.cmp(&b)));
    for &i in order.iter().take(leftover as usize) {
        parts[i] += 1;
    }

    // 7. Restore the sign.
    if negative {
        for part in &mut parts {
            *part = -*part;
        }
    }
    Ok(parts)
}

/// `total` split evenly `count` ways. Earlier indices absorb the extra units.
pub fn allocate_evenly(total: i64, count: usize) -> Result<Vec<i64>> {
    allocate(total, &vec![1i64; count])
}
