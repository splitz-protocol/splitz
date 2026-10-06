#!/usr/bin/env python3
"""Generates vectors/withholdings.json from SPEC.md section 14.

What a payer is asked for, and what is held back before a request is built.
Section 14 is addressed to a host, so none of it is reachable from the wire
format alone: an implementation can keep every rule in sections 1 to 12, ask
a payer for a debt they have already paid, or name the wrong person as the
one whose confirmation it waits on. These cases are that surface.
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

    def case(name, plan, b, payer, recorded_by=None):
        out.append({
            "name": name,
            "plan": plan,
            "bill": b,
            "payer": payer,
            **({"recordedBy": recorded_by} if recorded_by is not None else {}),
            "expect": withholdings(plan, b, payer, recorded_by),
        })

    # Section 14.4 across payers. A confirmation re-plans from confirmed
    # balances and moves a debt onto ana, whom dee is already paying with a
    # record of his own: asked for again, ana is paid twice.
    EVE, DEE = who("eve", ADDRESSES[3]), who("dee", ADDRESSES[4])
    PAYERS = [ANA, EVE, DEE]
    case("a_payee_another_payer_is_already_paying_is_not_asked_for_again",
         [settle("eve", "ana", 1105)],
         bill(PAYERS, [pay("p1", "dee", "ana", 1105)]), "eve",
         {"p1": "dee"})
    # A record the payee wrote is their word, not a payment in flight.
    case("a_record_the_payee_wrote_holds_nothing_back_from_another_payer",
         [settle("eve", "ana", 1105)],
         bill(PAYERS, [pay("p1", "dee", "ana", 1105)]), "eve",
         {"p1": "ana"})
    # What others have in flight leaves room for the rest of the payee's
    # credit, and that is still asked for.
    case("another_payers_payment_leaves_room_for_the_rest",
         [settle("eve", "ana", 505), settle("dee", "ana", 600)],
         bill(PAYERS, [pay("p1", "dee", "ana", 600)]), "eve",
         {"p1": "dee"})
    case("another_payers_part_payment_holds_back_what_it_covers",
         [settle("eve", "ana", 505), settle("dee", "ana", 600)],
         bill(PAYERS, [pay("p1", "dee", "ana", 900)]), "eve",
         {"p1": "dee"})

    # Section 14.4 with section 6.3's coverage. Ana owed ben and has paid
    # him, not yet confirmed; netting reroutes the debt so the plan asks her
    # to pay cai. The settlement covers the debt already paid, so it waits.
    rerouted = dict(settle("ana", "cai", 1000),
                    covers=[{"from": "ana", "to": "ben", "amount": 1000}])
    case("a_rerouted_debt_already_paid_is_not_asked_for_again",
         [rerouted], bill(THREE, [pay("p1", "ana", "ben", 1000)]), "ana")
    # The debt held back is cai's; the payment it waits on went to ben, and
    # ben is the one who can confirm it or who it can be taken back from.
    case("a_held_debt_names_who_the_unconfirmed_money_went_to",
         [rerouted], bill(THREE, [pay("p1", "ana", "ben", 1000),
                                  pay("p2", "ana", "cai", 300)]), "ana")
    case("and_one_covering_a_debt_nobody_paid_is_carried",
         [rerouted], bill(THREE, [pay("p1", "ana", "cai", 5)],
                          confirmed=["p1"]), "ana")

    # Section 14.4: a payment counts first against the payer's own settlement
    # to that creditor. Dee owes three people; netting has each settlement
    # cover parts of the others' debts. Paying ben and cai exactly what their
    # settlements ask holds nothing else back, so ana is still asked for.
    DEE = who("dee", ADDRESSES[3])
    FOUR = [ANA, BEN, CAI, DEE]
    netted = [
        dict(settle("dee", "ana", 240),
             covers=[{"from": "dee", "to": "ana", "amount": 30},
                     {"from": "dee", "to": "cai", "amount": 195},
                     {"from": "dee", "to": "ben", "amount": 15}]),
        dict(settle("dee", "ben", 180),
             covers=[{"from": "dee", "to": "ana", "amount": 180}]),
        dict(settle("dee", "cai", 180),
             covers=[{"from": "dee", "to": "ben", "amount": 180}]),
    ]
    case("a_payment_matching_its_own_settlement_holds_no_other",
         netted, bill(FOUR, [pay("p1", "dee", "ben", 180),
                             pay("p2", "dee", "cai", 180)]), "dee")
    # Paid cai 20 beyond his own settlement: that much may be a debt netting
    # moved, so every settlement covering cai waits on it.
    case("a_payment_beyond_its_own_settlement_holds_those_covering_it",
         netted, bill(FOUR, [pay("p1", "dee", "cai", 200)]), "dee")

    # Section 14.4's bound. Netting moved the debt ana paid ben onto cai, and
    # the settlement to cai names no covers: what is pending already meets
    # what she owes, so asking for cai's 1000 overpays by exactly that.
    case("a_debt_netting_moved_is_held_when_no_covers_name_it",
         [settle("ana", "cai", 1000)],
         bill(THREE, [pay("p1", "ana", "ben", 1000)]), "ana")
    # Less pending than the settlement asks still holds the whole of it:
    # requesting 1000 over 300 pending overpays by 300.
    case("a_settlement_larger_than_what_is_left_owed_waits_whole",
         [settle("ana", "cai", 1000)],
         bill(THREE, [pay("p1", "ana", "ben", 300)]), "ana")
    # In plan order, each settlement is carried while what is left owed
    # covers it: 1200 owed less 300 pending holds cai's 1000 and carries
    # dee's 200.
    case("a_smaller_settlement_after_a_held_one_is_still_carried",
         [settle("ana", "cai", 1000), settle("ana", "dee", 200)],
         bill(FOUR, [pay("p1", "ana", "ben", 300)]), "ana")
    # Pending money within the payer's own settlement to its payee leaves the
    # rest of the debt its full room.
    case("a_payment_within_its_own_settlement_leaves_the_rest_carried",
         [settle("ana", "ben", 300), settle("ana", "cai", 1000)],
         bill(THREE, [pay("p1", "ana", "ben", 300)]), "ana")

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

    # Somebody else's pending payment to the same payee covers what the plan
    # still owes her: once it lands she is paid, so cai's debt waits on it
    # rather than paying her twice. Ana releases it by withdrawing a record
    # of money that never came (section 10.8).
    case("somebody_elses_pending_payment_to_the_payee_holds_the_debt",
         [settle("cai", "ana", 4500)],
         bill(THREE, payments=[pay("p1", "ben", "ana", 4500)]), "cai")

    # Section 8.4 still applies downstream: a carried settlement may name a
    # participant with no address.
    case("a_carried_settlement_may_still_be_unpayable",
         [settle("cai", "ana", 4500)],
         bill([who("ana"), BEN, CAI]), "cai")

    case("every_debt_held_back_carries_nothing",
         [settle("cai", "ana", 4500), settle("cai", "ben", 1000)],
         bill(THREE, payments=[pay("p1", "cai", "ana", 4500),
                               pay("p2", "cai", "ben", 1000)]), "cai")

    case("one_of_each",
         [settle("cai", "ana", 4500), settle("cai", "ben", 1000),
          settle("cai", "cai", 0)],
         bill(THREE, payments=[pay("p1", "cai", "ana", 100)]), "cai")

    return out


def main():
    root = pathlib.Path(__file__).resolve().parents[2] / "vectors"
    cs = cases()
    doc = {"description": "What a request carries and what is held back. "
                          "SPEC.md section 14.",
           "count": len(cs), "cases": cs}
    (root / "withholdings.json").write_text(json.dumps(doc, indent=2) + "\n")
    print(f"{len(cs):3} cases -> withholdings.json")
    buckets = {"carried": 0, "awaiting": 0}
    for c in cs:
        for k in buckets:
            buckets[k] += len(c["expect"][k])
    print("    " + "  ".join(f"{k}: {v}" for k, v in buckets.items()))


if __name__ == "__main__":
    main()
