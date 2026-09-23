#!/usr/bin/env python3
"""Generates vectors/withholdings.json from SPEC.md section 14.

What a payer is asked for, and what is held back before a request is built.
Section 14 is addressed to a host, so none of it is reachable from the wire
format alone: an implementation can keep every rule in sections 1 to 12, ask
a payer for a debt they have already paid, and send the balance of a bill to
whoever minted the second claim on an id. These cases are that surface.
"""
import json
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from _spec import ADDRESSES, withholdings  # noqa: E402

AT = "2026-10-28T19:30:00.000Z"


def who(pid, address=None):
    p = {"id": pid, "name": pid.capitalize()}
    if address is not None:
        p["payTo"] = address
    return p


def bill(participants, payments=(), confirmed=()):
    return {
        "v": 1,
        "id": "b1",
        "name": "Dinner",
        "currency": "EUR",
        "splitMode": "equal",
        "participants": list(participants),
        "expenses": [],
        "payments": list(payments),
        "confirmedPayments": list(confirmed),
    }


def pay(pid, frm, to, amount):
    return {"id": pid, "from": frm, "to": to, "amount": amount,
            "currency": "EUR", "method": "shieldedZec", "at": AT}


def settle(frm, to, amount):
    return {"from": frm, "to": to, "amount": amount}


ANA = who("ana", ADDRESSES[0])
BEN = who("ben", ADDRESSES[1])
CAI = who("cai", ADDRESSES[2])
THREE = [ANA, BEN, CAI]


def cases():
    out = []

    def case(name, plan, b, payer, contested=(), pay_anyway=(),
             recorded_by=None):
        out.append({
            "name": name,
            "plan": plan,
            "bill": b,
            "payer": payer,
            **({"contested": list(contested)} if contested else {}),
            **({"payAnyway": list(pay_anyway)} if pay_anyway else {}),
            **({"recordedBy": recorded_by} if recorded_by is not None else {}),
            "expect": withholdings(plan, b, payer, contested, pay_anyway,
                                   recorded_by),
        })

    # Section 14.4 with section 6.3's coverage. Ana owed ben and has paid
    # him, not yet confirmed; netting reroutes the debt so the plan asks her
    # to pay cai. The settlement covers the debt already paid, so it waits.
    rerouted = dict(settle("ana", "cai", 1000),
                    covers=[{"from": "ana", "to": "ben", "amount": 1000}])
    case("a_rerouted_debt_already_paid_is_not_asked_for_again",
         [rerouted], bill(THREE, [pay("p1", "ana", "ben", 1000)]), "ana")
    case("and_one_covering_a_debt_nobody_paid_is_carried",
         [rerouted], bill(THREE, [pay("p1", "ana", "cai", 5)],
                          confirmed=["p1"]), "ana")

    # Section 14.4 withholds for a record the payer wrote. Ben is owed on a
    # debt the settlement to cai covers; a record he wrote himself, saying ana
    # paid him, is his word and not hers, and holds nothing back.
    case("a_record_a_covered_creditor_wrote_holds_nothing_back",
         [rerouted], bill(THREE, [pay("p1", "ana", "ben", 1)]), "ana",
         recorded_by={"p1": "ben"})
    case("and_one_the_payer_wrote_still_does",
         [rerouted], bill(THREE, [pay("p1", "ana", "ben", 1)]), "ana",
         recorded_by={"p1": "ana"})

    # Nothing held back.
    case("a_plain_debt_is_carried",
         [settle("cai", "ana", 4500)], bill(THREE), "cai")

    # Only this payer's settlements. Section 14 partitions one payer's
    # obligation, not the plan.
    case("another_payers_settlements_are_not_this_payers",
         [settle("cai", "ana", 4500), settle("ben", "ana", 100)],
         bill(THREE), "cai")

    # Section 14.4. An unconfirmed payment does not move a balance (10.5), so
    # the debt is still in the plan and asking again sends it twice.
    case("a_paid_debt_awaiting_confirmation_is_held_back",
         [settle("cai", "ana", 4500)],
         bill(THREE, payments=[pay("p1", "cai", "ana", 4500)]), "cai")

    # Section 14.4. The whole debt, not the remainder: requesting 2500 would
    # overpay by 2000 if the pending payment lands.
    case("a_part_payment_holds_back_the_whole_debt",
         [settle("cai", "ana", 4500)],
         bill(THREE, payments=[pay("p1", "cai", "ana", 2000)]), "cai")

    case("several_pending_payments_to_one_participant_sum",
         [settle("cai", "ana", 4500)],
         bill(THREE, payments=[pay("p1", "cai", "ana", 2000),
                               pay("p2", "cai", "ana", 1500)]), "cai")

    # Confirmed, so it moved the balance and is nobody's business here.
    case("a_confirmed_payment_holds_nothing_back",
         [settle("cai", "ana", 4500)],
         bill(THREE, payments=[pay("p1", "cai", "ana", 4500)],
              confirmed=["p1"]), "cai")

    case("somebody_elses_pending_payment_holds_nothing_back",
         [settle("cai", "ana", 4500)],
         bill(THREE, payments=[pay("p1", "ben", "ana", 4500)]), "cai")

    # Section 10.7 and 14.2. Neither key is bound, so the payout record on the
    # bill may be the impostor's.
    case("a_contested_recipient_is_held_back",
         [settle("cai", "ana", 4500)], bill(THREE), "cai", contested=["ana"])

    # The way through. Anyone may mint a rival claim, so a refusal with no
    # exit is a denial of payment.
    case("a_payer_who_accepted_the_contest_carries_it",
         [settle("cai", "ana", 4500)], bill(THREE), "cai",
         contested=["ana"], pay_anyway=["ana"])

    case("a_contest_on_somebody_else_holds_nothing_back",
         [settle("cai", "ana", 4500)], bill(THREE), "cai", contested=["ben"])

    # A pending payment outranks a contest: the money is already in flight,
    # so accepting the contest changes nothing about this debt.
    case("a_pending_payment_outranks_a_contest",
         [settle("cai", "ana", 4500)],
         bill(THREE, payments=[pay("p1", "cai", "ana", 4500)]), "cai",
         contested=["ana"], pay_anyway=["ana"])

    # Section 8.4 still applies downstream: a carried settlement may name a
    # participant with no address.
    case("a_carried_settlement_may_still_be_unpayable",
         [settle("cai", "ana", 4500)],
         bill([who("ana"), BEN, CAI]), "cai")

    case("every_debt_held_back_carries_nothing",
         [settle("cai", "ana", 4500), settle("cai", "ben", 1000)],
         bill(THREE, payments=[pay("p1", "cai", "ana", 4500)]), "cai",
         contested=["ben"])

    case("one_of_each",
         [settle("cai", "ana", 4500), settle("cai", "ben", 1000),
          settle("cai", "cai", 0)],
         bill(THREE, payments=[pay("p1", "cai", "ana", 100)]), "cai",
         contested=["ben"])

    return out


def main():
    root = pathlib.Path(__file__).resolve().parents[2] / "vectors"
    cs = cases()
    doc = {"description": "What a request carries and what is held back. "
                          "SPEC.md section 14.",
           "count": len(cs), "cases": cs}
    (root / "withholdings.json").write_text(json.dumps(doc, indent=2) + "\n")
    print(f"{len(cs):3} cases -> withholdings.json")
    buckets = {"carried": 0, "awaiting": 0, "contested": 0}
    for c in cs:
        for k in buckets:
            buckets[k] += len(c["expect"][k])
    print("    " + "  ".join(f"{k}: {v}" for k, v in buckets.items()))


if __name__ == "__main__":
    main()
