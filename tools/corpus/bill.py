#!/usr/bin/env python3
"""Generates vectors/bill-json.json, balances.json and settlement.json.

SPEC.md sections 5, 6 and 9.
"""
import copy, json, pathlib, sys
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from _spec import (ADDRESSES, decode_bill, balances, creditors_debtors, direct_debts,
                   settle, canonical_json, Refused, MAX_EXACT_LIMIT)

AT = "2026-10-28T19:30:00.000Z"
AT2 = "2026-10-28T19:31:00.000Z"


def bill(**over):
    base = {
        "v": 1, "id": "weekend", "name": "Zcon7 weekend", "currency": "EUR",
        "splitMode": "equal",
        "participants": [{"id": "ana", "name": "Ana", "payTo": ADDRESSES[0]},
                         {"id": "ben", "name": "Ben", "payTo": ADDRESSES[1]},
                         {"id": "cai", "name": "Cai", "payTo": ADDRESSES[2]}],
        "expenses": [{"id": "e1", "description": "dinner", "paidBy": "ana",
                      "amount": 9000, "currency": "EUR", "at": AT,
                      "split": {"type": "equal", "among": ["ana", "ben", "cai"]}}],
        "payments": [],
    }
    base.update(over)
    return base


def mutate(fn):
    d = bill()
    fn(d)
    return d


BILL_CASES = [
    ("a_plain_bill", bill()),

    # §9.1. An empty id is not a name anyone can be settled to: §8.5 would
    # render its payTo into a payment request like any other.
    ("an_empty_participant_id",
     mutate(lambda d: d["participants"][0].update({"id": ""}))),

    # §9.3. DIGIT is %x30-39. A reader whose \d matches every Unicode decimal
    # digit emits a "canonical" instant that is not ASCII, and it sorts after
    # every ASCII instant on the bill.
    ("an_instant_with_arabic_indic_digits",
     mutate(lambda d: d["expenses"][0].update(
         {"at": "2026-\u0661\u0660-28T19:30:00.000Z"}))),
    ("an_instant_with_fullwidth_digits",
     mutate(lambda d: d["expenses"][0].update(
         {"at": "2026-10-28T19:30:00.\uff10\uff10\uff10Z"}))),
    ("no_payments_is_not_malformed", {k: v for k, v in bill().items() if k != "payments"}),
    ("an_expense_may_state_its_own_currency", bill()),
    ("a_participant_may_publish_payouts",
     mutate(lambda d: d["participants"][0].update(
         {"payouts": [{"type": "zec", "address": ADDRESSES[0]}, {"type": "cash"}]}))),
    ("a_payment_may_carry_what_it_sent",
     mutate(lambda d: d.__setitem__("payments", [
         {"id": "p1", "from": "ben", "to": "ana", "amount": 3000,
          "currency": "EUR", "method": "shieldedZec", "at": AT2,
          "zatoshi": 36843,
          "paidAtRate": {"currency": "EUR", "minorUnitsPerZec": 950000, "at": AT}}]))),

    ("no_version",            mutate(lambda d: d.pop("v"))),
    ("a_version_as_a_string", mutate(lambda d: d.__setitem__("v", "2"))),
    ("a_version_of_zero",     mutate(lambda d: d.__setitem__("v", 0))),
    ("a_version_from_the_future", mutate(lambda d: d.__setitem__("v", 99))),
    ("no_currency",           mutate(lambda d: d.pop("currency"))),
    ("an_empty_currency",     mutate(lambda d: d.__setitem__("currency", ""))),
    ("a_lower_case_currency", mutate(lambda d: d.__setitem__("currency", "eur"))),
    ("a_four_letter_currency",mutate(lambda d: d.__setitem__("currency", "EURO"))),
    ("an_unknown_split_mode", mutate(lambda d: d.__setitem__("splitMode", "byHeight"))),
    ("a_split_mode_of_the_wrong_type", mutate(lambda d: d.__setitem__("splitMode", 7))),
    ("two_participants_with_one_id",
     mutate(lambda d: d["participants"].append({"id": "ana", "name": "Impostor"}))),
    ("a_pay_to_of_the_wrong_type",
     mutate(lambda d: d["participants"][0].__setitem__("payTo", 5))),
    ("an_unknown_payout_type",
     mutate(lambda d: d["participants"][0].__setitem__("payouts", [{"type": "gold"}]))),
    ("an_expense_in_another_currency",
     mutate(lambda d: d["expenses"][0].__setitem__("currency", "USD"))),
    ("a_payer_who_is_not_on_the_bill",
     mutate(lambda d: d["expenses"][0].__setitem__("paidBy", "zed"))),
    ("an_amount_of_the_wrong_type",
     mutate(lambda d: d["expenses"][0].__setitem__("amount", "9000"))),
    ("an_amount_that_is_a_float",
     mutate(lambda d: d["expenses"][0].__setitem__("amount", 90.0))),
    ("a_leap_second",
     mutate(lambda d: d["expenses"][0].__setitem__("at", "2026-12-31T23:59:60.000Z"))),
    ("a_day_the_month_does_not_have",
     mutate(lambda d: d["expenses"][0].__setitem__("at", "2026-09-31T00:00:00.000Z"))),
    ("a_numeric_offset",
     mutate(lambda d: d["expenses"][0].__setitem__("at", "2026-10-28T19:30:00.000+01:00"))),
    ("a_space_separator",
     mutate(lambda d: d["expenses"][0].__setitem__("at", "2026-10-28 19:30:00.000Z"))),
    ("a_bare_date",
     mutate(lambda d: d["expenses"][0].__setitem__("at", "2026-10-28"))),
    ("fractional_digits_are_truncated",
     mutate(lambda d: d["expenses"][0].__setitem__("at", "2026-10-28T19:30:00.9999Z"))),
    ("a_payment_to_oneself",
     mutate(lambda d: d.__setitem__("payments", [
         {"id": "p1", "from": "ana", "to": "ana", "amount": 100,
          "currency": "EUR", "method": "cash", "at": AT2}]))),
    ("an_unknown_settlement_method",
     mutate(lambda d: d.__setitem__("payments", [
         {"id": "p1", "from": "ben", "to": "ana", "amount": 100,
          "currency": "EUR", "method": "barter", "at": AT2}]))),
    ("a_rate_pricing_another_currency",
     mutate(lambda d: d.__setitem__("payments", [
         {"id": "p1", "from": "ben", "to": "ana", "amount": 100,
          "currency": "EUR", "method": "shieldedZec", "at": AT2,
          "paidAtRate": {"currency": "USD", "minorUnitsPerZec": 51234, "at": AT}}]))),

    # A bill's rate is snapshotted onto it (§7), and a document that carries
    # one must read back carrying it: a reader that dropped it would price
    # every device's settlement by whatever it looked up instead.
    ("a_bill_carrying_a_rate",
     mutate(lambda d: d.__setitem__("rate",
         {"currency": "EUR", "minorUnitsPerZec": 51234, "at": AT}))),
    ("a_rate_naming_its_source",
     mutate(lambda d: d.__setitem__("rate",
         {"currency": "EUR", "minorUnitsPerZec": 51234, "at": AT,
          "source": "a named feed"}))),

    # §9.2's advisory halves. `reference` identifies a swap off this chain and
    # is not a Zcash txid; a document that dropped it would leave the swap
    # unidentifiable after one re-share.
    ("a_swap_payment_with_its_reference",
     mutate(lambda d: d.__setitem__("payments", [
         {"id": "near-intent-7f3a", "from": "ana", "to": "ben", "amount": 100,
          "currency": "EUR", "method": "swap", "at": AT2,
          "reference": "near-intent-7f3a", "zatoshi": 1000000,
          "note": "USDC on base"}]))),
    ("a_cash_payment_with_a_note",
     mutate(lambda d: d.__setitem__("payments", [
         {"id": "c1", "from": "ana", "to": "ben", "amount": 100,
          "currency": "EUR", "method": "cash", "at": AT2,
          "note": "handed over at the table"}]))),
    ("a_payment_priced_at_the_rate_it_was_paid_at",
     mutate(lambda d: d.__setitem__("payments", [
         {"id": "p1", "from": "ana", "to": "ben", "amount": 100,
          "currency": "EUR", "method": "shieldedZec", "at": AT2,
          "zatoshi": 195234,
          "paidAtRate": {"currency": "EUR", "minorUnitsPerZec": 51234,
                         "at": AT, "source": "a named feed"}}]))),

    # §9. A list member that is not a list is refused, not read as empty: an
    # empty string, an object and `false` would each otherwise stand for "no
    # participants" on one reader and fail on another.
    ("participants_that_are_a_string",
     mutate(lambda d: d.__setitem__("participants", ""))),
    ("expenses_that_are_an_object",
     mutate(lambda d: d.__setitem__("expenses", {}))),
    ("payments_that_are_false",
     mutate(lambda d: d.__setitem__("payments", False))),
    # §7. A rate on a bill, and one a payment was paid at, is decoded as a
    # rate: every member checked, not only its currency.
    ("a_bill_rate_that_is_not_positive",
     mutate(lambda d: d.__setitem__("rate",
         {"currency": "EUR", "minorUnitsPerZec": 0, "at": AT}))),
    ("a_bill_rate_that_states_no_instant",
     mutate(lambda d: d.__setitem__("rate",
         {"currency": "EUR", "minorUnitsPerZec": 51234}))),
    ("a_paid_at_rate_that_is_not_positive",
     mutate(lambda d: d.__setitem__("payments", [
         {"id": "p1", "from": "ben", "to": "ana", "amount": 100,
          "currency": "EUR", "method": "shieldedZec", "at": AT2,
          "paidAtRate": {"currency": "EUR", "minorUnitsPerZec": -1,
                         "at": AT}}]))),
    ("a_paid_at_rate_whose_instant_is_a_number",
     mutate(lambda d: d.__setitem__("payments", [
         {"id": "p1", "from": "ben", "to": "ana", "amount": 100,
          "currency": "EUR", "method": "shieldedZec", "at": AT2,
          "paidAtRate": {"currency": "EUR", "minorUnitsPerZec": 51234,
                         "at": 7}}]))),
    # §10.7. An identity key is what a participant id is derived from, so it
    # is a 32-byte key in canonical unpadded base64url.
    ("an_identity_key_that_is_not_a_key",
     mutate(lambda d: d["participants"][1].__setitem__("identityKey", "abc"))),

    # §9.1's payout preferences, in their order: the order IS the preference,
    # and a reader that reordered them settles to a different address.
    ("participants_declaring_every_payout_type",
     mutate(lambda d: d.__setitem__("participants", [
         {"id": "ana", "name": "Ana",
          "payouts": [{"type": "zec", "address": ADDRESSES[0]}]},
         {"id": "ben", "name": "Ben",
          "payouts": [{"type": "swap", "asset": "USDC", "chain": "base",
                       "address": "0xben"},
                      {"type": "cash"}]},
         {"id": "cai", "name": "Cai", "payouts": [{"type": "cash"}]}]))),
]


def balance_cases():
    out = []
    for name, doc in [
        ("one_expense_three_ways", bill()),
        ("two_expenses_net_off", bill(expenses=[
            {"id": "e1", "paidBy": "ana", "amount": 9000, "currency": "EUR", "at": AT,
             "split": {"type": "equal", "among": ["ana", "ben", "cai"]}},
            {"id": "e2", "paidBy": "ben", "amount": 3000, "currency": "EUR", "at": AT2,
             "split": {"type": "equal", "among": ["ana", "ben", "cai"]}}])),
        ("everybody_nets_to_zero", bill(expenses=[
            {"id": "e1", "paidBy": "ana", "amount": 3000, "currency": "EUR", "at": AT,
             "split": {"type": "exact", "amounts": {"ana": 3000}}}])),
        # §9.1: the id must be in `confirmedPayments` for the payment to
        # move anything. Without it the document has confirmed nothing, and a
        # reader that settles anyway clears a debt on the debtor's own claim.
        ("a_confirmed_payment_moves_a_balance", bill(payments=[
            {"id": "p1", "from": "ben", "to": "ana", "amount": 3000,
             "currency": "EUR", "method": "cash", "at": AT2}],
            confirmedPayments=["p1"])),
        ("a_recorded_payment_that_nobody_confirmed_moves_nothing", bill(payments=[
            {"id": "p1", "from": "ben", "to": "ana", "amount": 3000,
             "currency": "EUR", "method": "cash", "at": AT2}])),
        ("a_confirmed_payments_list_that_is_not_a_list", bill(payments=[
            {"id": "p1", "from": "ben", "to": "ana", "amount": 3000,
             "currency": "EUR", "method": "cash", "at": AT2}],
            confirmedPayments="p1")),
    ]:
        case = {"name": name, "bill": doc}
        try:
            t = decode_bill(doc)
            net = balances(t)
            cred, debt = creditors_debtors(net)
            case["expect"] = {"net": net,
                              "creditors": [{"id": i, "amount": a} for i, a in cred],
                              "debtors": [{"id": i, "amount": a} for i, a in debt],
                              "directDebts": direct_debts(t)}
            assert sum(net.values()) == 0, f"{name}: residual {sum(net.values())}"
        except Refused as r:
            case["error"] = r.code
        out.append(case)
    return out


def settlement_cases():
    out = []
    for name, net, limit in [
        ("three_people_two_payments", {"ana": 5000, "ben": -1000, "cai": -4000}, 14),
        ("two_independent_pairs", {"a": -100, "b": 100, "c": -50, "d": 50}, 14),
        ("a_group_of_five_needs_four",
         {"a": -10, "b": -20, "c": -30, "d": -40, "e": 100}, 14),
        ("nobody_owes_anything", {"ana": 0, "ben": 0}, 14),
        ("a_tie_goes_to_the_lower_id", {"a": -100, "b": 50, "c": 50}, 14),
        ("past_the_exact_limit_reports_itself",
         dict([("a", 140)] + [(chr(98 + i), -10) for i in range(14)]), 2),
        # A running total in ascending id order never overflows here, but the
        # subset {a, c} sums to 2^64. Only a check inside the partition search
        # catches it; a bound on the whole set does not.
        # A3: every balance is inside i64 and the residual is zero, but the
        # positives sum past it. Deciding the residual with a running total or
        # with the sum of the positives refuses this legitimate bill, and
        # refuses it in one participant order and not another.
        ("positives_that_exceed_i64_still_settle",
         {"a": 5000000000000000000, "b": -5000000000000000000,
          "c": 5000000000000000000, "d": -5000000000000000000}, 14),
        # A2: no settlement amount can carry a balance of the most negative
        # 64-bit integer.
        ("a_balance_of_the_most_negative_integer",
         {"a": -9223372036854775808, "b": 9223372036854775807, "c": 1}, 14),

        ("a_subset_wraps_although_the_total_does_not",
         {"a": 9223372036854775807, "b": -9223372036854775807,
          "c": 9223372036854775807, "d": -9223372036854775807}, 14),
        ("balances_that_do_not_sum_to_zero", {"a": 100, "b": -50}, 14),
        ("a_running_total_that_overflows",
         {"a": 9223372036854775807, "b": 9223372036854775807,
          "c": 2, "d": -2}, 14),

        ("an_exact_limit_over_the_ceiling",
         {"a": -100, "b": 100}, MAX_EXACT_LIMIT + 1),
    ]:
        case = {"name": name, "balances": net, "exactLimit": limit}
        try:
            plan = settle(dict(net), limit)
            case["expect"] = plan
            if plan["settlements"]:
                moved = {}
                for s in plan["settlements"]:
                    moved[s["from"]] = moved.get(s["from"], 0) + s["amount"]
                    moved[s["to"]] = moved.get(s["to"], 0) - s["amount"]
                for pid, delta in moved.items():
                    assert net.get(pid, 0) + delta == 0, \
                        f"{name}: {pid} left with {net.get(pid,0)+delta}"
        except Refused as r:
            case["error"] = r.code
        out.append(case)
    return out


def main():
    root = pathlib.Path(__file__).resolve().parents[2] / "vectors"

    bill_out = []
    for name, doc in BILL_CASES:
        case = {"name": name, "json": doc}
        try:
            case["expect"] = decode_bill(doc)
        except Refused as r:
            case["error"] = r.code
        bill_out.append(case)

    for fname, desc, cases in [
        ("bill-json.json", "Decoding a bill. SPEC.md sections 2 and 9.", bill_out),
        ("balances.json", "Net positions and pre-netting debts. SPEC.md section 5.", balance_cases()),
        ("settlement.json", "Settlement plans. SPEC.md section 6.", settlement_cases()),
    ]:
        doc = {"description": desc, "count": len(cases), "cases": cases}
        (root / fname).write_text(json.dumps(doc, indent=2) + "\n")
        codes = sorted({c["error"] for c in cases if "error" in c})
        print(f"{len(cases):3} -> {fname:18} codes: {' '.join(codes)}")


if __name__ == "__main__":
    main()
