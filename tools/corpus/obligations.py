#!/usr/bin/env python3
"""Generates vectors/obligations.json from SPEC.md section 8.5.

One payer's whole obligation, from a settlement plan to a payment request.
The cases that matter are the ones where the URI cannot carry a recipient: a
request that silently covers three of a payer's four debts is
indistinguishable, to the payer who sends it, from one that settles all four.
"""
import json
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from _spec import ADDRESSES, Refused, render_obligation  # noqa: E402

AT = "2026-10-28T19:30:00.000Z"
RATE = {"currency": "MXN", "minorUnitsPerZec": 950000, "at": AT}
LOW_RATE = {"currency": "MXN", "minorUnitsPerZec": 3000, "at": AT}


def who(pid, name, address=None, payouts=None):
    p = {"id": pid, "name": name}
    if address is not None:
        p["payTo"] = address
    if payouts is not None:
        p["payouts"] = payouts
    return p


ANA = who("ana", "Ana", ADDRESSES[0])
BEN = who("ben", "Ben", ADDRESSES[1])
CAI = who("cai", "Cai", ADDRESSES[2])
DEE_NO_ADDRESS = who("dee", "Dee")
ELI_CASH = who("eli", "Eli", ADDRESSES[3], [{"type": "cash"}])
FAY_SWAP = who("fay", "Fay", ADDRESSES[4],
               [{"type": "swap", "asset": "USDC", "chain": "near",
                 "address": "fay.near"}])
GUS_ZEC_PAYOUT = who("gus", "Gus", None,
                     [{"type": "zec", "address": ADDRESSES[5]}])


def pay(to, amount):
    return {"from": "cai", "to": to, "amount": amount}


CASES = [
    ("one_recipient", [pay("ana", 7004)], [ANA, CAI], False, True),
    ("two_recipients_one_transaction",
     [pay("ana", 7004), pay("ben", 9246)], [ANA, BEN, CAI], False, True),
    ("fiat_is_off_unless_asked_for",
     [pay("ana", 7004)], [ANA, CAI], False, False),
    ("a_payout_preference_overrides_pay_to",
     [pay("gus", 7004)], [GUS_ZEC_PAYOUT, CAI], False, True),

    # The cases section 8.5 exists for.
    ("a_recipient_with_no_address_refuses_the_whole_request",
     [pay("ana", 7004), pay("dee", 9246)], [ANA, DEE_NO_ADDRESS, CAI],
     False, True),
    ("or_is_reported_alongside_the_uri",
     [pay("ana", 7004), pay("dee", 9246)], [ANA, DEE_NO_ADDRESS, CAI],
     True, True),
    ("a_cash_payout_cannot_become_an_output",
     [pay("ana", 7004), pay("eli", 9246)], [ANA, ELI_CASH, CAI], True, True),
    ("a_swap_payout_cannot_either",
     [pay("ana", 7004), pay("fay", 9246)], [ANA, FAY_SWAP, CAI], True, True),
    ("every_recipient_unpayable_carries_no_uri",
     [pay("dee", 7004), pay("eli", 9246)], [DEE_NO_ADDRESS, ELI_CASH, CAI],
     True, True),
    ("an_empty_pay_to_is_not_an_address",
     [pay("ana", 7004), pay("dee", 9246)],
     [ANA, who("dee", "Dee", ""), CAI], True, True),
    ("and_refuses_the_request_when_not_skipping",
     [pay("ana", 7004), pay("dee", 9246)],
     [ANA, who("dee", "Dee", ""), CAI], False, True),
    # A published address section 8.3 does not admit is reported, not
    # carried: anyone may publish any string as their own address, and the
    # renderer would refuse every other output over it.
    ("an_address_no_request_can_carry_is_reported",
     [pay("ana", 7004), pay("dee", 9246)],
     [ANA, who("dee", "Dee", "u1dee-not-an-address"), CAI], True, True),
    ("and_refuses_the_request_as_a_bad_address_when_not_skipping",
     [pay("ana", 7004), pay("dee", 9246)],
     [ANA, who("dee", "Dee", "u1dee-not-an-address"), CAI], False, True),
    ("a_zec_payout_whose_address_no_request_can_carry",
     [pay("ana", 7004), pay("gus", 9246)],
     [ANA, who("gus", "Gus", None, [{"type": "zec", "address": "u1 gus"}]),
      CAI], True, True),
    ("a_zec_payout_with_no_address",
     [pay("ana", 7004), pay("gus", 9246)],
     [ANA, who("gus", "Gus", None, [{"type": "zec"}]), CAI], True, True),

    ("a_recipient_who_is_not_on_the_bill",
     [pay("zed", 7004)], [ANA, CAI], True, True),

    # An unpayable amount never passes through section 7, so nothing bounds it
    # before it reaches the withheld total. Two of them sum past i64, and both
    # totals section 8.5 makes normative must be formed with section 2.2's
    # checked arithmetic or the render must refuse. Unwrapped, `withheld`
    # reads as the most negative 64-bit integer while nothing is withheld.
    ("a_withheld_total_that_overflows",
     [pay("dee", 9223372036854775807), pay("eli", 1)],
     [DEE_NO_ADDRESS, ELI_CASH, CAI], True, False),
    ("two_withheld_amounts_that_still_fit",
     [pay("dee", 9223372036854775806), pay("eli", 1)],
     [DEE_NO_ADDRESS, ELI_CASH, CAI], True, False),

    # Section 8.5. One debt past what a request can price is reported beside
    # the others, never a reason to carry none of them. 92233720369 is past
    # what section 7.1 prices; 92233720368 at 3000 minor units a ZEC prices to
    # 3074457345600000 zatoshi, past the 21000000 ZEC section 8.1 renders.
    ("a_debt_section_7_cannot_price_is_reported_alongside",
     [pay("ana", 7004), pay("ben", 92233720369)], [ANA, BEN, CAI], True, True),
    ("and_an_unpriceable_debt_refuses_the_request_when_not_skipping",
     [pay("ana", 7004), pay("ben", 92233720369)], [ANA, BEN, CAI], False, True),
    ("a_debt_past_the_largest_request_is_reported_alongside",
     [pay("ana", 7004), pay("ben", 92233720368)], [ANA, BEN, CAI], True, False,
     LOW_RATE),

    # Pricing.
    ("an_amount_that_rounds_up", [pay("ana", 350)], [ANA, CAI], False, True),
    ("an_amount_that_divides_exactly",
     [pay("ana", 950000)], [ANA, CAI], False, True),
]


def main():
    out = []
    for name, settlements, participants, skip, fiat, *rest in CASES:
        rate = rest[0] if rest else RATE
        case = {"name": name, "settlements": settlements,
                "participants": participants, "rate": rate,
                "currency": "MXN", "skipUnpayable": skip, "includeFiat": fiat}
        try:
            result = render_obligation(settlements, participants, rate, "MXN",
                                       skip_unpayable=skip, include_fiat=fiat)
            # What the URI carries plus what it withholds must account for the
            # whole obligation. A figure that prices the whole of it must never
            # be presented as what the URI sends.
            total = sum(s["amount"] for s in settlements)
            assert result["carriedMinorUnits"] + result["withheldMinorUnits"] \
                == total, f"{name}: {result} does not account for {total}"
            case["expect"] = result
        except Refused as r:
            case["error"] = r.code
        out.append(case)

    doc = {"description": "One payer's obligation as a payment request. "
                          "SPEC.md section 8.5.",
           "count": len(out), "cases": out}
    p = pathlib.Path(__file__).resolve().parents[2] / "vectors" / "obligations.json"
    p.write_text(json.dumps(doc, indent=2) + "\n")
    codes = sorted({c["error"] for c in out if "error" in c})
    print(f"{len(out)} cases -> {p.name}")
    print(f"codes: {' '.join(codes)}")


if __name__ == "__main__":
    main()
