#!/usr/bin/env python3
"""Generates vectors/writers.json from SPEC.md sections 9.1 and 9.2.

What a host refuses to write, whether in a join, a payment record or an
amendment of either: a reader would take each as unpayable or as nothing, and
the person who wrote it would never learn why. Blank is Unicode White_Space,
the one set every implementation reads alike.
"""
import json
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from _spec import Refused, check_written_payment, check_written_payout  # noqa: E402

ADDRESS = "u1writer"


def cases():
    out = []

    def case(name, kind, value):
        c = {"name": name, kind: value}
        try:
            (check_written_payout if kind == "payout" else check_written_payment)(value)
            c["expect"] = {"accepted": True}
        except Refused as r:
            c["error"] = r.code
        out.append(c)

    case("a_zec_payout_with_an_address", "payout", {"type": "zec", "address": ADDRESS})
    case("a_zec_payout_with_no_address", "payout", {"type": "zec"})
    case("a_zec_payout_with_an_empty_address", "payout", {"type": "zec", "address": ""})
    case("a_zec_payout_of_white_space", "payout", {"type": "zec", "address": " 　\t"})
    case("a_byte_order_mark_is_not_white_space", "payout",
         {"type": "zec", "address": "﻿"})
    case("an_information_separator_is_not_white_space", "payout",
         {"type": "zec", "address": "\u001c"})
    case("a_swap_payout_whole", "payout",
         {"type": "swap", "asset": "usdc", "chain": "base", "address": "0xabc"})
    case("a_swap_payout_with_no_chain", "payout",
         {"type": "swap", "asset": "usdc", "address": "0xabc"})
    case("a_cash_payout_needs_nothing", "payout", {"type": "cash"})
    case("an_unknown_payout_is_left_to_the_reader", "payout", {"type": "gold"})

    case("a_payment_of_one", "payment", {"amount": 1, "method": "cash"})
    case("a_payment_of_nothing", "payment", {"amount": 0, "method": "cash"})
    case("a_negative_payment", "payment", {"amount": -5, "method": "cash"})
    case("a_swap_with_its_reference", "payment",
         {"amount": 100, "method": "swap", "reference": "intent-1"})
    case("a_swap_with_no_reference", "payment", {"amount": 100, "method": "swap"})
    case("a_swap_whose_reference_is_white_space", "payment",
         {"amount": 100, "method": "swap", "reference": " "})
    case("a_shielded_payment_needs_no_reference", "payment",
         {"amount": 100, "method": "shieldedZec"})
    return out


def main():
    root = pathlib.Path(__file__).resolve().parents[2] / "vectors"
    cs = cases()
    doc = {"description": "What a host refuses to write. SPEC.md sections "
                          "9.1 and 9.2.",
           "count": len(cs), "cases": cs}
    (root / "writers.json").write_text(json.dumps(doc, indent=2) + "\n")
    print(f"{len(cs):3} cases -> writers.json")
    print("codes: " + " ".join(sorted({c["error"] for c in cs if "error" in c})))


if __name__ == "__main__":
    main()
