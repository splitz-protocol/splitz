#!/usr/bin/env python3
"""Generates vectors/rate.json from SPEC.md section 7."""
import json, pathlib, sys
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from _spec import fiat_to_zatoshi, zatoshi_to_fiat, Refused, I64_MAX, ZAT_PER_ZEC

MXN = {"currency": "MXN", "minorUnitsPerZec": 950000, "at": "2026-10-28T19:30:00.000Z"}
USD = {"currency": "USD", "minorUnitsPerZec": 51234, "at": "2026-10-28T19:30:00.000Z"}
BOUND = I64_MAX // ZAT_PER_ZEC          # 92233720368, computed not quoted
# The highest price a rate can carry. A remainder under it can reach 2^62.
HIGHEST = {"currency": "MXN", "minorUnitsPerZec": I64_MAX, "at": MXN["at"]}

TO_ZAT = [
    ("exact_division_needs_no_rounding", 950000, MXN, None, "up"),
    ("rounds_up_by_default",                350, MXN, None, "up"),
    ("the_same_amount_rounded_down",        350, MXN, None, "down"),
    ("nearest_rounds_up_at_half",             1, {"currency": "USD", "minorUnitsPerZec": 2, "at": MXN["at"]}, None, "nearest"),
    ("nearest_rounds_down_below_half",        1, {"currency": "USD", "minorUnitsPerZec": 3, "at": MXN["at"]}, None, "nearest"),
    ("a_small_amount_at_a_high_price",        1, USD, None, "up"),
    ("zero_converts_to_zero",                 0, MXN, None, "up"),
    ("matching_currency_is_accepted",       350, MXN, "MXN", "up"),
    ("the_largest_amount_that_fits",      BOUND, MXN, None, "down"),

    ("a_currency_the_rate_does_not_price",  350, MXN, "EUR", "up"),
    ("a_price_of_zero",                     350, {"currency": "MXN", "minorUnitsPerZec": 0, "at": MXN["at"]}, None, "up"),
    ("a_negative_price",                    350, {"currency": "MXN", "minorUnitsPerZec": -1, "at": MXN["at"]}, None, "up"),
    ("a_negative_amount",                  -350, MXN, None, "up"),
    ("one_unit_past_the_bound",         BOUND+1, MXN, None, "up"),
    ("a_lower_case_currency",               350, {"currency": "mxn", "minorUnitsPerZec": 950000, "at": MXN["at"]}, None, "up"),

    # The half-way test must not be written as `remainder x 2`. At a price of
    # i64::MAX the remainder passes 2^62, and doubling it wraps negative, which
    # reads as below half and rounds the wrong way. PEAK is the largest
    # remainder that can arise, so it is the most this can be off by.
    ("nearest_at_a_remainder_past_two_to_the_sixty_two",
     BOUND, HIGHEST, None, "nearest"),
    ("nearest_just_below_half_at_the_same_price",
     BOUND // 2, HIGHEST, None, "nearest"),
]

TO_FIAT = [
    ("one_zec_is_the_price",        ZAT_PER_ZEC, MXN),
    ("half_a_zec",              ZAT_PER_ZEC // 2, MXN),
    ("display_rounds_half_up",                 1, {"currency": "USD", "minorUnitsPerZec": ZAT_PER_ZEC * 2, "at": MXN["at"]}),
    ("zero_zatoshi",                           0, MXN),
    ("a_negative_zatoshi_count",              -1, MXN),
    ("the_product_overflows",            I64_MAX, MXN),
]


def main():
    out = []
    for name, minor, rate, cur, rounding in TO_ZAT:
        case = {"name": name, "direction": "fiatToZatoshi", "minorUnits": minor,
                "rate": rate, "rounding": rounding}
        if cur is not None:
            case["amountCurrency"] = cur
        try:
            case["expect"] = fiat_to_zatoshi(minor, rate, cur, rounding)
        except Refused as r:
            case["error"] = r.code
        out.append(case)

    for name, zat, rate in TO_FIAT:
        case = {"name": name, "direction": "zatoshiToFiat", "zatoshi": zat, "rate": rate}
        try:
            case["expect"] = zatoshi_to_fiat(zat, rate)
        except Refused as r:
            case["error"] = r.code
        out.append(case)

    # Every conversion that succeeds must be reproducible by the stated formula.
    for c in out:
        if "expect" not in c or c["direction"] != "fiatToZatoshi":
            continue
        n = c["minorUnits"] * ZAT_PER_ZEC
        per = c["rate"]["minorUnitsPerZec"]
        q, r = divmod(n, per)
        want = q if r == 0 else {"up": q + 1, "down": q,
                                 "nearest": q + 1 if r >= per - r else q}[c["rounding"]]
        assert c["expect"] == want, f"{c['name']}: {c['expect']} != {want}"

    doc = {"description": "Fiat to zatoshi and back. SPEC.md section 7.",
           "count": len(out), "cases": out}
    p = pathlib.Path(__file__).resolve().parents[2] / "vectors" / "rate.json"
    p.write_text(json.dumps(doc, indent=2) + "\n")
    codes = sorted({c["error"] for c in out if "error" in c})
    print(f"{len(out)} cases -> {p.name}")
    print(f"codes covered: {' '.join(codes)}")
    print(f"bound used: {BOUND} (computed as i64::MAX // {ZAT_PER_ZEC})")


if __name__ == "__main__":
    main()
