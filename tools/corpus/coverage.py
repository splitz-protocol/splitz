#!/usr/bin/env python3
"""Generates vectors/coverage.json from SPEC.md section 6.3.

Netting reroutes payments, so a participant is asked to pay somebody they never
transacted with. These cases pin what each payment discharges, and which
payments a wallet must explain rather than present as a debt between two
people.
"""
import json
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from _spec import (attribute_coverage, decode_bill, direct_debts,  # noqa: E402
                   settle, settle_bill, Refused)

AT = "2026-10-28T19:30:00.000Z"


def who(*ids):
    return [{"id": i, "name": i.title()} for i in ids]


def spend(eid, payer, amount, among):
    return {"id": eid, "paidBy": payer, "amount": amount, "currency": "MXN",
            "at": AT, "split": {"type": "equal", "among": among}}


def bill(participants, expenses):
    return {"v": 1, "id": "b", "name": "Trip", "currency": "MXN",
            "participants": who(*participants), "expenses": expenses}


# Ana covers dinner three ways; Ben covers a taxi he shared with Cai. Cai is
# then asked to pay Ana more than Cai ever owed Ana.
DINNER = bill(["ana", "ben", "cai"], [
    spend("e1", "ana", 4800, ["ana", "ben", "cai"]),
    spend("e2", "ben", 300, ["ben", "cai"]),
])

# Cai's payment to Ana covers a little of what Ana lent and a great deal of
# what Ben and Dee lent. Testing only whether Ana appears in the coverage
# reports this payment as direct, which is section 6.3's stated trap.
MEMBERSHIP = bill(["ana", "ben", "cai", "dee"], [
    spend("e1", "ana", 900, ["ana", "ben", "dee"]),
    spend("e2", "ben", 610, ["ben", "cai"]),
    spend("e3", "dee", 610, ["dee", "cai"]),
    spend("e4", "ana", 20, ["ana", "cai"]),
])

# A refund (section 10.4) leaves the pair (ana, ben) aggregating to -100: a
# credit on that pair, not a debt, so it discharges nothing.
REFUND = bill(["ana", "ben", "cai"], [
    spend("e1", "ben", 1000, ["ana", "ben"]),
    spend("e2", "ben", -1200, ["ana", "ben"]),
    spend("e3", "cai", 400, ["ana", "cai"]),
])

NOBODY = bill(["ana", "ben"], [spend("e1", "ana", 3000, ["ana"])])

# §10.4 lets anybody holding the invite write an expense and §4 admits a
# negative total, so a peer can attribute a refund to somebody who never agreed
# to it. Ben's settlement then exceeds every debt the bill records for him, and
# coverage — the line a payer reads to answer "why do I owe this" — falls
# short. The shortfall is stated rather than left silent.
FABRICATED_REFUND = bill(["ana", "ben"], [
    spend("e1", "ana", 10000, ["ana", "ben"]),
    spend("e2", "ben", -10000, ["ana", "ben"]),
])

CASES = [
    ("a_fabricated_refund_leaves_a_settlement_unexplained", FABRICATED_REFUND),
    ("a_rerouted_payment_names_the_debts_it_discharges", DINNER),
    ("membership_is_not_the_test", MEMBERSHIP),
    ("a_credit_on_a_pair_is_not_consumed", REFUND),
    ("nobody_owes_anything", NOBODY),
]


def main():
    out = []
    for name, doc in CASES:
        case = {"name": name, "bill": doc, "exactLimit": 14}
        try:
            t = decode_bill(doc)
            plan = settle_bill(t)
            case["expect"] = plan

            # Coverage never invents value: what a payment covers sums to at
            # most what it carries, and every cover names a real debt.
            rows = direct_debts(t)
            owed = {}
            for d in rows:
                owed[(d["from"], d["to"])] = owed.get((d["from"], d["to"]), 0) \
                    + d["amount"]
            drawn = {}
            for s in plan["settlements"]:
                assert sum(c["amount"] for c in s["covers"]) <= s["amount"], \
                    f"{name}: {s} covers more than it carries"
                for c in s["covers"]:
                    assert c["amount"] > 0, f"{name}: a cover of {c['amount']}"
                    assert c["from"] == s["from"], f"{name}: {c} is not the payer's"
                    key = (c["from"], c["to"])
                    assert key in owed, f"{name}: {key} is not a debt on the bill"
                    drawn[key] = drawn.get(key, 0) + c["amount"]
            for key, taken in drawn.items():
                assert taken <= owed[key], \
                    f"{name}: {key} drawn {taken} of {owed[key]}"

            # A payer who is also owed money settles less than they directly
            # owe, and the shortfall is exactly the smaller of what they owe
            # and what they are owed. These bills carry no recorded payment,
            # so a net balance is owed-to minus owed-by and nothing else.
            for pid in {d["from"] for d in rows}:
                by = sum(a for (f, _), a in owed.items() if f == pid)
                to = sum(a for (_, c), a in owed.items() if c == pid)
                covered = sum(a for (f, _), a in drawn.items() if f == pid)
                assert by - covered == min(by, max(to, 0)), \
                    f"{name}: {pid} leaves {by - covered} of {by} uncovered, " \
                    f"is owed {to}"
        except Refused as r:
            case["error"] = r.code
        out.append(case)

    # Net balances do not carry the debts that produced them, so a plan
    # computed from balances carries no coverage and reports nothing rerouted.
    net_only = {"ana": 5000, "ben": -1000, "cai": -4000}
    plan = settle(dict(net_only))
    out.append({
        "name": "a_plan_from_balances_carries_no_coverage",
        "balances": net_only,
        "exactLimit": 14,
        "expect": {**plan,
                   "settlements": attribute_coverage(plan["settlements"], [])},
    })

    doc = {"description": "What each payment discharges. SPEC.md section 6.3.",
           "count": len(out), "cases": out}
    p = pathlib.Path(__file__).resolve().parents[2] / "vectors" / "coverage.json"
    p.write_text(json.dumps(doc, indent=2) + "\n")
    rerouted = sum(1 for c in out for s in c.get("expect", {}).get("settlements", [])
                   if s["rerouted"])
    print(f"{len(out)} cases -> {p.name} ({rerouted} rerouted payments)")


if __name__ == "__main__":
    main()
