#!/usr/bin/env python3
"""Generates vectors/allocation.json from SPEC.md section 3.

Written from the specification, not from either shipped implementation, so a
case cannot inherit a mistake one of them is self-consistently making.
"""
import json, pathlib

I64_MAX = 2**63 - 1
I64_MIN = -(2**63)


class Refused(Exception):
    def __init__(self, code):
        self.code = code


def allocate(total, weights):
    """SPEC.md section 3, steps 1 to 7."""
    # 1. W must be greater than zero.
    if not weights:
        raise Refused("empty_weights")
    if any(w < 0 for w in weights):
        raise Refused("negative_weight")

    # 2. W accumulated without overflow.
    W = 0
    for w in weights:
        W += w
        if W > I64_MAX:
            raise Refused("weight_sum_overflow")
    if W == 0:
        raise Refused("zero_weight_sum")

    # 3. total is not the most negative 64-bit integer.
    if total == I64_MIN:
        raise Refused("allocation_overflow")

    s = -1 if total < 0 else 1
    m = abs(total)

    # 4. every product fits.
    for w in weights:
        if m * w > I64_MAX:
            raise Refused("allocation_overflow")

    # 5. floor division and remainders.
    parts = [(m * w) // W for w in weights]
    rems = [(m * w) % W for w in weights]

    # 6. leftover, descending remainder, ties to the lower index.
    leftover = m - sum(parts)
    if not (0 <= leftover < len(weights)):
        raise Refused("allocation_overflow")
    order = sorted(range(len(weights)), key=lambda i: (-rems[i], i))
    for i in order[:leftover]:
        parts[i] += 1

    # 7. restore the sign.
    return [p * s for p in parts] if s < 0 else parts


CASES = [
    # --- exact division, nothing left over ---
    ("splits_evenly",                 900, [1, 1, 1]),
    ("one_weight_takes_everything",   500, [7]),
    ("zero_weight_gets_nothing",      100, [1, 0, 1]),

    # --- leftover units land by descending remainder, ties to lower index ---
    ("one_unit_left_over",            100, [1, 1, 1]),
    ("two_units_left_over",           101, [1, 1, 1]),
    ("unequal_weights_round",        1000, [1, 2, 4]),
    ("tie_goes_to_the_lower_index",     2, [1, 1, 1]),

    # --- sign ---
    ("a_refund_divides",             -900, [1, 1, 1]),
    ("a_refund_with_a_leftover",     -100, [1, 1, 1]),
    ("zero_total_allocates_zero",       0, [1, 1, 1]),

    # --- conservation at awkward magnitudes ---
    ("large_total_conserves",  I64_MAX // 2, [1, 1, 1]),
    ("many_participants",             997, [1] * 13),

    # --- refusals, one per code section 3 names ---
    ("no_weights_at_all",             100, []),
    ("a_negative_weight",             100, [1, -1]),
    ("every_weight_is_zero",          100, [0, 0]),
    ("weight_sum_overflows",          100, [I64_MAX, 1]),
    ("product_overflows",         I64_MAX, [2, 1]),
    ("most_negative_total",       I64_MIN, [1, 1]),
]


def main():
    out = []
    for name, total, weights in CASES:
        case = {"name": name, "total": total, "weights": weights}
        try:
            parts = allocate(total, weights)
            assert sum(parts) == total, f"{name}: parts sum to {sum(parts)}, not {total}"
            case["expect"] = parts
        except Refused as r:
            case["error"] = r.code
        out.append(case)

    doc = {
        "description": "Largest-remainder allocation. SPEC.md section 3.",
        "count": len(out),
        "cases": out,
    }
    path = pathlib.Path(__file__).resolve().parents[2] / "vectors" / "allocation.json"
    path.write_text(json.dumps(doc, indent=2) + "\n")

    codes = sorted({c["error"] for c in out if "error" in c})
    print(f"{len(out)} cases -> {path}")
    print(f"codes covered: {' '.join(codes)}")


if __name__ == "__main__":
    main()
