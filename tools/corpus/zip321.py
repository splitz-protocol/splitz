#!/usr/bin/env python3
"""Generates vectors/zip321.json from SPEC.md section 8."""
import json, pathlib, sys
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from _spec import render_uri, Refused, ZAT_PER_ZEC, MAX_ZATOSHI, ADDRESSES

A = ADDRESSES[0]
B = ADDRESSES[1]
# Addresses of the other kinds section 8.6 answers for, from zcash_address
# 0.13 `src/encoding.rs` and ZIP 320.
SAPLING = "zs1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqpq6d8g"
P2PKH = "t1Hsc1LR8yKnbbe3twRp88p6vFfC5t7DLbs"
TEX = "tex1s2rt77ggv6q989lr49rkgzmh5slsksa9khdgte"

CASES = [
    ("one_payment_puts_the_address_in_the_path",
     [{"address": A, "zatoshi": 36843}], False),
    ("a_whole_zec_has_no_decimal_point",
     [{"address": A, "zatoshi": ZAT_PER_ZEC}], False),
    ("trailing_zeros_are_removed",
     [{"address": A, "zatoshi": 50_000_000}], False),
    ("one_zatoshi_keeps_eight_places",
     [{"address": A, "zatoshi": 1}], False),
    ("the_largest_amount_there_is",
     [{"address": A, "zatoshi": MAX_ZATOSHI}], False),

    ("two_payments_index_the_second",
     [{"address": A, "zatoshi": 7004}, {"address": B, "zatoshi": 9246}], False),
    ("parameters_keep_their_order",
     [{"address": A, "zatoshi": 7004, "memo": "thanks", "label": "Ana",
       "message": "dinner"}], False),
    ("a_label_is_percent_escaped",
     [{"address": A, "zatoshi": 7004, "label": "Zcon7 dîner ✨"}], False),
    ("a_long_label_is_cut_on_a_character_boundary",
     [{"address": A, "zatoshi": 7004, "label": "é" * 60}], False),
    ("a_memo_at_the_cap",
     [{"address": A, "zatoshi": 7004, "memo": "m" * 512}], False),

    ("fiat_prices_this_payment",
     [{"address": A, "zatoshi": 6_000_000, "fiat": ("EUR", 3000)}], True),
    ("each_payment_carries_its_own_fiat",
     [{"address": A, "zatoshi": 7004, "fiat": ("EUR", 1000)},
      {"address": B, "zatoshi": 9246, "fiat": ("EUR", 1500)}], True),
    ("fiat_is_omitted_unless_asked_for",
     [{"address": A, "zatoshi": 7004, "fiat": ("EUR", 1000)}], False),
    ("fiat_at_the_digit_cap",
     [{"address": A, "zatoshi": 7004, "fiat": ("EUR", 10**18 - 1)}], True),

    # A memo goes only where section 8.6 says one can be delivered; an
    # address carrying no memo is checked for syntax alone.
    ("a_memo_to_a_sapling_address",
     [{"address": SAPLING, "zatoshi": 7004, "memo": "thanks"}], False),
    ("a_transparent_address_without_a_memo",
     [{"address": P2PKH, "zatoshi": 7004, "label": "Ana"}], False),
    ("a_memo_beside_a_transparent_output",
     [{"address": A, "zatoshi": 7004, "memo": "thanks"},
      {"address": P2PKH, "zatoshi": 9246}], False),

    # --- refusals, one per code section 8 names ---
    ("a_memo_to_a_transparent_address",
     [{"address": P2PKH, "zatoshi": 7004, "memo": "thanks"}], False),
    ("a_memo_to_a_tex_address",
     [{"address": TEX, "zatoshi": 7004, "memo": "thanks"}], False),
    ("an_empty_memo_to_a_transparent_address",
     [{"address": P2PKH, "zatoshi": 7004, "memo": ""}], False),
    ("a_memo_on_the_second_output_to_a_transparent_address",
     [{"address": A, "zatoshi": 7004},
      {"address": P2PKH, "zatoshi": 9246, "memo": "thanks"}], False),
    ("a_memo_to_something_that_is_no_address",
     [{"address": "u1abc", "zatoshi": 7004, "memo": "thanks"}], False),
    ("a_memo_to_a_transparent_address_is_checked_before_the_amount",
     [{"address": P2PKH, "zatoshi": 0, "memo": "thanks"}], False),
    ("no_payments_at_all", [], False),
    ("an_amount_of_zero", [{"address": A, "zatoshi": 0}], False),
    ("a_negative_amount", [{"address": A, "zatoshi": -1}], False),
    ("more_than_there_will_ever_be",
     [{"address": A, "zatoshi": MAX_ZATOSHI + 1}], False),
    ("a_memo_one_byte_over",
     [{"address": A, "zatoshi": 7004, "memo": "m" * 513}], False),
    ("an_address_the_grammar_forbids",
     [{"address": ADDRESSES[0][:20] + "-" + ADDRESSES[0][21:], "zatoshi": 7004}], False),
    ("no_address_at_all", [{"address": "", "zatoshi": 7004}], False),
    ("the_address_is_checked_first",
     [{"address": "", "zatoshi": 0}], False),
    ("a_fiat_code_that_is_not_three_letters",
     [{"address": A, "zatoshi": 7004, "fiat": ("EURO", 3000)}], True),
    ("a_fiat_count_of_zero",
     [{"address": A, "zatoshi": 7004, "fiat": ("EUR", 0)}], True),
    ("a_fiat_count_of_nineteen_digits",
     [{"address": A, "zatoshi": 7004, "fiat": ("EUR", 10**18)}], True),
]

# Indices run to 9999, so 10001 payments is one past the cap. The case is
# carried as a count rather than ten thousand literal payments: a suite builds
# the list and the file stays readable.
BULK = [
    ("the_largest_index_there_is", 10000, False),
    ("one_payment_past_the_cap",   10001, True),
]


def main():
    out = []
    for name, payments, fiat in CASES:
        case = {"name": name, "payments": payments, "includeFiat": fiat}
        try:
            case["expect"] = render_uri(payments, fiat)
        except Refused as r:
            case["error"] = r.code
        out.append(case)

    # Every rendered amount must parse back to the zatoshi it came from.
    for c in out:
        if "expect" not in c:
            continue
        import re
        for amt, p in zip(re.findall(r"amount(?:\.\d+)?=([0-9.]+)", c["expect"]),
                          c["payments"]):
            whole, _, frac = amt.partition(".")
            back = int(whole) * ZAT_PER_ZEC + int(frac.ljust(8, "0") or 0)
            assert back == p["zatoshi"], f"{c['name']}: {amt} -> {back} != {p['zatoshi']}"

    for name, count, expect_refusal in BULK:
        payments = [{"address": A, "zatoshi": 1000 + i} for i in range(count)]
        case = {"name": name,
                "repeatPayment": {"address": A, "zatoshiFrom": 1000},
                "paymentCount": count}
        try:
            uri = render_uri(payments, False)
            assert not expect_refusal, f"{name}: expected a refusal"
            case["expectLength"] = len(uri)
            case["expectLastIndex"] = count - 1
        except Refused as r:
            case["error"] = r.code
        out.append(case)

    doc = {"description": "Payment request URIs. SPEC.md section 8.",
           "count": len(out), "cases": out}
    p = pathlib.Path(__file__).resolve().parents[2] / "vectors" / "zip321.json"
    p.write_text(json.dumps(doc, indent=2) + "\n")
    codes = sorted({c["error"] for c in out if "error" in c})
    print(f"{len(out)} cases -> {p.name}")
    print(f"codes covered: {' '.join(codes)}")


if __name__ == "__main__":
    main()
