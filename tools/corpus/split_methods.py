#!/usr/bin/env python3
"""Generates vectors/split-methods.json from SPEC.md section 4."""
import json, pathlib, sys
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from _spec import split, Refused, I64_MAX

CASES = [
    # 4.1 equal
    ("equal_two_ways",            700,  {"type": "equal", "among": ["ana", "ben"]}),
    ("equal_three_with_leftover", 100,  {"type": "equal", "among": ["ana", "ben", "cai"]}),
    ("equal_collapses_duplicates",100,  {"type": "equal", "among": ["ana", "ana", "ben"]}),
    ("equal_sorts_before_splitting", 100, {"type": "equal", "among": ["cai", "ana", "ben"]}),
    ("equal_needs_somebody",      100,  {"type": "equal", "among": []}),

    # 4.2 exact
    ("exact_amounts_stand",       700,  {"type": "exact", "amounts": {"ana": 400, "ben": 300}}),
    ("exact_allows_a_zero_share", 700,  {"type": "exact", "amounts": {"ana": 700, "ben": 0}}),
    ("exact_must_reach_the_total",700,  {"type": "exact", "amounts": {"ana": 400, "ben": 200}}),
    ("exact_refuses_a_negative",  700,  {"type": "exact", "amounts": {"ana": 900, "ben": -200}}),
    ("exact_sum_overflows",       700,  {"type": "exact", "amounts": {"ana": I64_MAX, "ben": I64_MAX}}),

    # 4.3 percentage
    ("percentage_full_scale",    1000,  {"type": "percentage", "basisPoints": {"ana": 2500, "ben": 7500}}),
    ("percentage_rounds_by_remainder", 100, {"type": "percentage", "basisPoints": {"ana": 3333, "ben": 3333, "cai": 3334}}),
    ("percentage_short_of_scale", 1000, {"type": "percentage", "basisPoints": {"ana": 2500, "ben": 7000}}),
    ("percentage_overflow_before_scale", 1000,
     {"type": "percentage", "basisPoints": {"a": 2**62, "b": 2**62, "c": 2**62, "d": 2**62, "e": 10000}}),

    # 4.4 shares
    ("shares_by_count",          1000,  {"type": "shares", "shareCounts": {"ana": 1, "ben": 3}}),
    ("shares_zero_owes_nothing",  900,  {"type": "shares", "shareCounts": {"ana": 2, "ben": 0, "cai": 1}}),
    ("shares_all_zero",           900,  {"type": "shares", "shareCounts": {"ana": 0, "ben": 0}}),

    # 4.5 itemized
    ("itemized_no_extra",       10400,  {"type": "itemized", "extraMinorUnits": 0, "items": [
        {"description": "tacos", "minorUnits": 5200, "sharedBy": ["ben", "eli"]},
        {"description": "mole",  "minorUnits": 5200, "sharedBy": ["ana"]}]}),
    ("itemized_tax_follows_what_was_eaten", 12000, {"type": "itemized", "extraMinorUnits": 2000, "items": [
        {"description": "tacos", "minorUnits": 2000, "sharedBy": ["ben"]},
        {"description": "steak", "minorUnits": 8000, "sharedBy": ["ana"]}]}),
    ("itemized_all_zero_items_share_extra_evenly", 1000, {"type": "itemized", "extraMinorUnits": 1000, "items": [
        {"description": "water", "minorUnits": 0, "sharedBy": ["ana"]},
        {"description": "bread", "minorUnits": 0, "sharedBy": ["ben"]}]}),
    ("itemized_needs_items",    1000,  {"type": "itemized", "extraMinorUnits": 1000, "items": []}),
    ("itemized_item_needs_an_eater", 1000, {"type": "itemized", "extraMinorUnits": 0, "items": [
        {"description": "tacos", "minorUnits": 1000, "sharedBy": []}]}),
    ("itemized_must_reach_the_total", 9999, {"type": "itemized", "extraMinorUnits": 0, "items": [
        {"description": "tacos", "minorUnits": 1000, "sharedBy": ["ana"]}]}),
    ("itemized_refuses_a_negative_extra", 1000, {"type": "itemized", "extraMinorUnits": -100, "items": [
        {"description": "tacos", "minorUnits": 1100, "sharedBy": ["ana"]}]}),
    ("itemized_refuses_a_negative_item", 1000, {"type": "itemized", "extraMinorUnits": 2000, "items": [
        {"description": "refund", "minorUnits": -1000, "sharedBy": ["ana"]}]}),

    # A refund is an expense with a negative total (section 10.4), and all five
    # methods divide one. A rule refusing every negative share leaves `exact`
    # and `itemized` unable to express what the other three split.
    ("a_refund_split_equally",   -1000, {"type": "equal", "among": ["ana", "ben"]}),
    ("a_refund_by_shares",       -1000, {"type": "shares", "shareCounts": {"ana": 1, "ben": 1}}),
    ("a_refund_by_percentage",   -1000, {"type": "percentage", "basisPoints": {"ana": 5000, "ben": 5000}}),
    ("a_refund_in_exact_amounts",-1000, {"type": "exact", "amounts": {"ana": -500, "ben": -500}}),
    ("a_refund_itemized",        -1000, {"type": "itemized", "extraMinorUnits": -100, "items": [
        {"description": "tacos", "minorUnits": -600, "sharedBy": ["ana"]},
        {"description": "mole",  "minorUnits": -300, "sharedBy": ["ben"]}]}),

    # The rule is sign agreement, so it bites in both directions.
    ("exact_refuses_a_positive_share_of_a_refund", -1000,
     {"type": "exact", "amounts": {"ana": -1500, "ben": 500}}),
    ("itemized_refuses_a_positive_item_on_a_refund", -1000,
     {"type": "itemized", "extraMinorUnits": 0, "items": [
        {"description": "tacos", "minorUnits": -1100, "sharedBy": ["ana"]},
        {"description": "tip",   "minorUnits": 100,   "sharedBy": ["ben"]}]}),
    ("itemized_refuses_a_positive_extra_on_a_refund", -1000,
     {"type": "itemized", "extraMinorUnits": 100, "items": [
        {"description": "tacos", "minorUnits": -1100, "sharedBy": ["ana"]}]}),

    # §10.1: an id list names participants. Dropping a member a reader cannot
    # read reassigns that participant's share — 9000 three ways becomes two
    # paying 4500 each, silently.
    ("among_holds_a_non_string", 9000,
     {"type": "equal", "among": ["ana", 7, "ben"]}),
    ("shared_by_holds_a_non_string", 9000,
     {"type": "itemized", "extraMinorUnits": 0, "items": [
        {"description": "tacos", "minorUnits": 9000,
         "sharedBy": ["ana", 7, "ben"]}]}),

    # §4. Every share, weight and item is an integer, checked before any is
    # compared: a null or a string there is refused, not read as a number.
    ("an_exact_share_that_is_null", 700,
     {"type": "exact", "amounts": {"ana": 700, "ben": None}}),
    ("a_basis_point_weight_that_is_a_string", 700,
     {"type": "percentage", "basisPoints": {"ana": "5000", "ben": 5000}}),
    ("a_share_count_that_is_a_fraction", 700,
     {"type": "shares", "shareCounts": {"ana": 1, "ben": 2.5}}),
    ("an_item_whose_amount_is_a_string", 700,
     {"type": "itemized", "items": [
        {"description": "tacos", "minorUnits": "700", "sharedBy": ["ana"]}]}),

    # unknown type
    ("an_unknown_split_type",    1000,  {"type": "byHeight", "among": ["ana"]}),
]


def main():
    out = []
    for name, total, spec in CASES:
        case = {"name": name, "total": total, "split": spec}
        try:
            shares = split(total, spec)
            assert sum(shares.values()) == total, \
                f"{name}: shares sum to {sum(shares.values())}, not {total}"
            case["expect"] = shares
        except Refused as r:
            case["error"] = r.code
        out.append(case)

    doc = {"description": "The five split methods. SPEC.md section 4.",
           "count": len(out), "cases": out}
    p = pathlib.Path(__file__).resolve().parents[2] / "vectors" / "split-methods.json"
    p.write_text(json.dumps(doc, indent=2) + "\n")
    codes = sorted({c["error"] for c in out if "error" in c})
    print(f"{len(out)} cases -> {p.name}")
    print(f"codes covered: {' '.join(codes)}")


if __name__ == "__main__":
    main()
