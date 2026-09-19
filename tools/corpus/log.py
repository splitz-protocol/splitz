#!/usr/bin/env python3
"""Generates vectors/log.json and vectors/authority.json.

SPEC.md sections 9.4, 10 and 10.5.
"""
import json, pathlib, sys
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from _spec import (ADDRESSES, check_entry, derive_bill_id, derive_entry_id,
                   seal_log, merge, order, fold, balances,
                   canonical_json, b64url, Refused)

def AT(m):
    return f"2026-10-28T19:{m:02d}:00.000Z"

KEY = b64url(b"k" * 32)
NONCE = b64url(b"n" * 16)
SIG = "S" * 86


def create(author="ana", name="Dinner", currency="EUR", nonce=NONCE):
    e = {"v": 1, "author": author, "kind": "createBill", "at": AT(0),
         "name": name, "currency": currency, "splitMode": "equal",
         "creatorKey": KEY, "nonce": nonce}
    e["id"] = derive_bill_id(e)
    return e


C = create()
J_ANA = {"v": 1, "id": "j1", "author": "ana", "kind": "joinBill", "at": AT(1),
         "participant": {"id": "ana", "name": "Ana", "payTo": ADDRESSES[0]}}
J_BEN = {"v": 1, "id": "j2", "author": "ben", "kind": "joinBill", "at": AT(2),
         "participant": {"id": "ben", "name": "Ben", "payTo": ADDRESSES[1]}}
E1 = {"v": 1, "id": "e1", "author": "ana", "kind": "addExpense", "at": AT(3),
      "expense": {"id": "x1", "description": "dinner", "paidBy": "ana",
                  "amount": 9000, "at": AT(3),
                  "split": {"type": "equal", "among": ["ana", "ben"]}}}
P1 = {"v": 1, "id": "p1", "author": "ben", "kind": "recordPayment", "at": AT(4),
      "payment": {"id": "y1", "from": "ben", "to": "ana", "amount": 4500,
                  "method": "cash", "at": AT(4)}}
J_DEE = {"v": 1, "id": "j3", "author": "dee", "kind": "joinBill", "at": AT(2),
         "participant": {"id": "dee", "name": "Dee"}}
BASE = [C, J_ANA, J_BEN, E1, P1]


def conf(eid, author, method, minute, ref=None, pid="y1"):
    c = {"paymentId": pid, "method": method}
    if ref:
        c["reference"] = ref
    return {"v": 1, "id": eid, "author": author, "kind": "confirmPayment",
            "at": AT(minute), "confirmation": c}


def void(eid, author, target, minute):
    return {"v": 1, "id": eid, "author": author, "kind": "voidEntry",
            "at": AT(minute), "targetId": target}


FOLD_CASES = [
    ("a_bill_with_one_expense", BASE, C["id"]),
    ("a_recorded_payment_is_not_yet_confirmed", BASE, C["id"]),
    ("the_recipient_confirms", BASE + [conf("c1", "ana", "recipientConfirmed", 5)], C["id"]),
    ("a_wallet_saw_it_land", BASE + [conf("c2", "ana", "walletReceived", 5)], C["id"]),
    ("on_chain_with_a_reference",
     BASE + [conf("c3", "ana", "onChain", 5, "tx:abc")], C["id"]),
    ("the_payer_attests_and_it_settles_nothing",
     BASE + [conf("c4", "ben", "payerAttested", 5)], C["id"]),

    ("the_payer_may_not_confirm_his_own_debt",
     BASE + [conf("c5", "ben", "recipientConfirmed", 5)], C["id"]),
    ("on_chain_without_a_reference",
     BASE + [conf("c6", "ana", "onChain", 5)], C["id"]),
    ("a_confirmation_method_nobody_defines",
     BASE + [conf("c7", "ana", "sawItOnTheNews", 5)], C["id"]),
    ("a_confirmation_for_a_payment_the_bill_lacks",
     BASE + [conf("c8", "ana", "recipientConfirmed", 5, pid="nope")], C["id"]),
    ("a_confirmation_arriving_before_its_payment",
     [C, J_ANA, J_BEN, E1, conf("c9", "ana", "recipientConfirmed", 1), P1], C["id"]),

    ("a_payment_neither_party_wrote",
     BASE + [{"v": 1, "id": "p2", "author": "ana", "kind": "recordPayment",
              "at": AT(6),
              "payment": {"id": "y2", "from": "ben", "to": "cai", "amount": 10,
                          "method": "cash", "at": AT(6)}}], C["id"]),
    ("an_expense_paid_by_a_stranger",
     BASE + [{"v": 1, "id": "e2", "author": "ana", "kind": "addExpense",
              "at": AT(6),
              "expense": {"id": "x2", "paidBy": "zed", "amount": 10, "at": AT(6),
                          "split": {"type": "equal", "among": ["ana"]}}}], C["id"]),
    ("an_expense_in_another_currency",
     BASE + [{"v": 1, "id": "e3", "author": "ana", "kind": "addExpense",
              "at": AT(6),
              "expense": {"id": "x3", "paidBy": "ana", "amount": 10,
                          "currency": "USD", "at": AT(6),
                          "split": {"type": "equal", "among": ["ana"]}}}], C["id"]),
    ("an_amount_that_states_no_currency_is_denominated_by_the_fold",
     BASE, C["id"]),

    ("the_creator_withdraws_an_expense",
     BASE + [void("v1", "ana", "e1", 6)], C["id"]),
    ("a_stranger_may_not_withdraw_an_expense",
     BASE + [void("v2", "ben", "e1", 6)], C["id"]),
    ("either_party_may_withdraw_a_payment",
     BASE + [void("v3", "ana", "p1", 6)], C["id"]),
    ("the_creator_may_not_withdraw_a_payment",
     BASE + [C, void("v4", "ana", "p1", 6)], C["id"]),
    ("a_participant_still_named_cannot_be_removed",
     BASE + [void("v5", "ana", "j2", 6)], C["id"]),
    ("removing_the_expense_first_lets_the_person_go",
     BASE + [void("v6", "ana", "e1", 6), void("v7", "ben", "p1", 6),
             void("v8", "ben", "j2", 7)], C["id"]),
    ("an_amendment_by_somebody_else",
     BASE + [{"v": 1, "id": "a1", "author": "ben", "kind": "amendEntry",
              "at": AT(6), "targetId": "e1",
              "expense": {"id": "x1", "paidBy": "ana", "amount": 1, "at": AT(3),
                          "split": {"type": "equal", "among": ["ana", "ben"]}}}],
     C["id"]),
    ("an_amendment_carrying_no_payload_of_its_target_kind",
     BASE + [{"v": 1, "id": "a2", "author": "ana", "kind": "amendEntry",
              "at": AT(6), "targetId": "e1"}], C["id"]),
    ("an_amendment_replaces_its_target",
     BASE + [{"v": 1, "id": "a3", "author": "ana", "kind": "amendEntry",
              "at": AT(6), "targetId": "e1",
              "expense": {"id": "x1", "paidBy": "ana", "amount": 6000, "at": AT(3),
                          "split": {"type": "equal", "among": ["ana", "ben"]}}}],
     C["id"]),
    ("a_rejoin_replaces_the_record_and_reports_the_address",
     BASE + [{"v": 1, "id": "j3", "author": "ben", "kind": "joinBill", "at": AT(6),
              "participant": {"id": "ben", "name": "Ben", "payTo": ADDRESSES[3]}}],
     C["id"]),
    ("somebody_else_may_not_change_a_record",
     BASE + [{"v": 1, "id": "j4", "author": "ana", "kind": "joinBill", "at": AT(6),
              "participant": {"id": "ben", "name": "Ben", "payTo": ADDRESSES[4]}}],
     C["id"]),

    # Section 10.8 admits a voidEntry targeting a voidEntry. Nothing produced
    # one until these, and a withdrawn withdrawal stayed in effect.
    ("a_withdrawal_may_itself_be_withdrawn",
     BASE + [void("v9", "ana", "e1", 6), void("v10", "ana", "v9", 7)], C["id"]),
    ("and_that_withdrawal_too",
     BASE + [void("v9", "ana", "e1", 6), void("v10", "ana", "v9", 7),
             void("v11", "ana", "v10", 8)], C["id"]),
    ("a_chain_four_deep",
     BASE + [void("v9", "ana", "e1", 6), void("v10", "ana", "v9", 7),
             void("v11", "ana", "v10", 8), void("v12", "ana", "v11", 9)], C["id"]),
    ("two_withdrawals_naming_each_other",
     BASE + [void("v13", "ana", "v14", 6), void("v14", "ana", "v13", 7)], C["id"]),
    ("a_stranger_may_not_take_back_somebody_elses_withdrawal",
     BASE + [void("v17", "ana", "p1", 6), void("v18", "cai", "v17", 7)], C["id"]),
    ("an_unauthorised_link_does_not_break_a_chain",
     BASE + [void("v19", "ana", "e1", 6), void("v20", "ben", "v19", 7),
             void("v21", "ana", "v20", 8)], C["id"]),

    ("withdrawing_a_withdrawal_of_a_payment",
     BASE + [void("v15", "ben", "p1", 6), void("v16", "ben", "v15", 7)], C["id"]),

    # A withdrawn amendment must not apply: the figure a person took back
    # would otherwise be the figure the bill shows.
    ("an_amendment_may_be_withdrawn",
     BASE + [{"v": 1, "id": "a9", "author": "ana", "kind": "amendEntry",
              "at": AT(6), "targetId": "e1",
              "expense": {"id": "x1", "paidBy": "ana", "amount": 6000, "at": AT(3),
                          "split": {"type": "equal", "among": ["ana", "ben"]}}},
             void("v22", "ana", "a9", 7)], C["id"]),
    ("withdrawing_the_withdrawal_restores_the_amendment",
     BASE + [{"v": 1, "id": "a9", "author": "ana", "kind": "amendEntry",
              "at": AT(6), "targetId": "e1",
              "expense": {"id": "x1", "paidBy": "ana", "amount": 6000, "at": AT(3),
                          "split": {"type": "equal", "among": ["ana", "ben"]}}},
             void("v22", "ana", "a9", 7), void("v23", "ana", "v22", 8)], C["id"]),
    ("a_stranger_may_not_withdraw_an_amendment",
     BASE + [{"v": 1, "id": "a9", "author": "ana", "kind": "amendEntry",
              "at": AT(6), "targetId": "e1",
              "expense": {"id": "x1", "paidBy": "ana", "amount": 6000, "at": AT(3),
                          "split": {"type": "equal", "among": ["ana", "ben"]}}},
             void("v22", "ben", "a9", 7)], C["id"]),
    ("an_amendment_naming_an_entry_the_log_lacks",
     BASE + [{"v": 1, "id": "a9", "author": "ana", "kind": "amendEntry",
              "at": AT(6), "targetId": "ghost",
              "expense": {"id": "x1", "paidBy": "ana", "amount": 1, "at": AT(3),
                          "split": {"type": "equal", "among": ["ana"]}}}], C["id"]),
    ("a_withdrawal_naming_an_entry_the_log_lacks",
     BASE + [void("v24", "ana", "ghost", 6)], C["id"]),

    # The still-named check reads the amendments, so the withdrawal filter has
    # to run first. Evaluating it against a version the fold then discards
    # removes a participant a surviving entry still names. Neither feature had
    # a case in the other's company until this one.
    ("a_withdrawn_amendment_does_not_let_a_participant_go",
     [C, J_ANA, J_BEN, J_DEE,
      {"v": 1, "id": "e9", "author": "ana", "kind": "addExpense", "at": AT(3),
       "expense": {"id": "x9", "paidBy": "ana", "amount": 900, "at": AT(3),
                   "split": {"type": "equal",
                             "among": ["ana", "ben", "dee"]}}},
      {"v": 1, "id": "a1", "author": "ana", "kind": "amendEntry", "at": AT(4),
       "targetId": "e9",
       "expense": {"id": "x9", "paidBy": "ana", "amount": 900, "at": AT(3),
                   "split": {"type": "equal", "among": ["ana", "ben"]}}},
      void("v1", "ana", "a1", 5),
      void("v2", "ana", "j3", 6)], C["id"]),

    # §10.1. The rate is shared state, so it reaches the bill the way
    # everything else does. Without this every device folding one log had to
    # find a rate of its own, which is what §7 exists to prevent.
    ("a_set_rate_reaches_the_bill",
     BASE + [{"v": 1, "id": "r1", "author": "ana", "kind": "setRate",
              "at": AT(6),
              "rate": {"currency": "EUR", "minorUnitsPerZec": 950000,
                       "at": AT(6)}}], C["id"]),
    ("the_latest_set_rate_decides",
     BASE + [{"v": 1, "id": "r1", "author": "ana", "kind": "setRate",
              "at": AT(6),
              "rate": {"currency": "EUR", "minorUnitsPerZec": 950000,
                       "at": AT(6)}},
             {"v": 1, "id": "r2", "author": "ben", "kind": "setRate",
              "at": AT(7),
              "rate": {"currency": "EUR", "minorUnitsPerZec": 990000,
                       "at": AT(7)}}], C["id"]),
    ("withdrawing_the_latest_restores_the_one_before",
     BASE + [{"v": 1, "id": "r1", "author": "ana", "kind": "setRate",
              "at": AT(6),
              "rate": {"currency": "EUR", "minorUnitsPerZec": 950000,
                       "at": AT(6)}},
             {"v": 1, "id": "r2", "author": "ben", "kind": "setRate",
              "at": AT(7),
              "rate": {"currency": "EUR", "minorUnitsPerZec": 990000,
                       "at": AT(7)}},
             void("vr", "ben", "r2", 8)], C["id"]),
    ("a_set_rate_carrying_no_rate",
     BASE + [{"v": 1, "id": "r3", "author": "ana", "kind": "setRate",
              "at": AT(6)}], C["id"]),

    ("an_empty_log", [], None),
    ("a_log_with_no_create", [J_ANA, J_BEN], None),
    ("two_create_entries",
     [C, create(name="Other"), J_ANA], None),
]

ENTRY_CASES = [
    ("a_create_entry_derives_its_id", C),
    ("a_create_entry_with_no_key", {k: v for k, v in C.items() if k != "creatorKey"}),
    ("a_create_entry_with_a_short_nonce", dict(C, nonce=b64url(b"n" * 8))),
    ("a_create_entry_with_a_chosen_id", dict(C, id="weekend")),
    ("an_unknown_entry_kind", dict(J_ANA, kind="sendGift")),
    ("an_entry_carrying_two_payloads",
     dict(J_ANA, kind="addExpense", expense={}, payment={})),
    ("a_join_carrying_no_participant",
     {k: v for k, v in J_ANA.items() if k != "participant"}),
    ("a_void_naming_no_target",
     {"v": 1, "id": "v9", "author": "ana", "kind": "voidEntry", "at": AT(6)}),
]

# §10.1. A scalar payload passes a presence-only check, enters the union, and
# then reaches the fold's authorisation pass, which reads the target's payload
# to decide who may withdraw the entry — so the entry cannot be taken off the
# bill by anybody and the bill is unopenable on every device that holds it.
for _kind, _member in (("joinBill", "participant"), ("addExpense", "expense"),
                       ("recordPayment", "payment"),
                       ("confirmPayment", "confirmation"),
                       ("vouchIdentity", "vouch")):
    ENTRY_CASES.append((f"a_scalar_{_member}_is_not_a_payload",
                        {"v": 1, "id": "s1", "author": "ana", "kind": _kind,
                         "at": AT(6), _member: 5}))

ENTRY_CASES += [
    # §9.5. An id anyone may choose is an id anyone may take.
    ("an_entry_whose_id_is_chosen_rather_than_derived", dict(J_ANA, id="j1")),

    ("a_payload_id_that_is_not_a_string",
     {"v": 1, "id": "s2", "author": "ana", "kind": "joinBill", "at": AT(6),
      "participant": {"id": 7, "name": "Seven"}}),
    ("a_payment_naming_a_non_string_payer",
     {"v": 1, "id": "s3", "author": "ben", "kind": "recordPayment", "at": AT(6),
      "payment": {"id": "y9", "from": 7, "to": "ana", "amount": 10,
                  "method": "cash", "at": AT(6)}}),
    ("a_target_id_that_is_not_a_string",
     {"v": 1, "id": "s4", "author": "ana", "kind": "voidEntry", "at": AT(6),
      "targetId": 7}),
    ("an_expense_whose_among_holds_a_non_string",
     {"v": 1, "id": "s5", "author": "ana", "kind": "addExpense", "at": AT(6),
      "expense": {"id": "x9", "paidBy": "ana", "amount": 9000, "at": AT(6),
                  "split": {"type": "equal", "among": ["ana", 7, "ben"]}}}),
]

# A non-string entry id is refused at ingress, and the refusal must carry an
# id without casting one: the refusal path has to be total over every value
# the check refuses.
MERGE_CASES_EXTRA = [
    ("a_non_string_entry_id_is_refused_not_thrown",
     [dict(J_ANA, id=7)], []),
]


def forgery_of(entry, **changed):
    """A re-pushed copy of `entry` with members changed, keeping its id.

    Before §9.5 this displaced the genuine entry on every device: `author` is
    a free string sorting between `at` and the payload, so appending one high
    byte to it beat any entry under §10.2 rule 2, whatever the payload said.
    """
    return dict(entry, **changed)

MERGE_CASES = [
    ("union_is_idempotent", [J_ANA], [J_ANA]),
    ("a_signed_copy_beats_an_unsigned_one", [J_ANA], [dict(J_ANA, sig=SIG)]),
    ("and_in_the_other_direction", [dict(J_ANA, sig=SIG)], [J_ANA]),
    ("two_unsigned_copies_resolve_by_canonical_order",
     [dict(J_ANA, participant={"id": "ana", "name": "Ana", "payTo": ADDRESSES[4]})],
     [dict(J_ANA, participant={"id": "ana", "name": "Ana", "payTo": ADDRESSES[5]})]),
    ("disjoint_logs_union", [J_ANA], [J_BEN]),

    # Removing a payload member makes an entry sort higher under §9.3, so
    # without §10.1 at ingress the stripped copy wins rule 2 and takes the
    # expense off the bill on every device.
    ("a_stripped_entry_never_enters_the_union",
     [E1], [{k: v for k, v in E1.items() if k != "expense"}]),
    ("and_not_in_the_other_direction_either",
     [{k: v for k, v in E1.items() if k != "expense"}], [E1]),
    ("a_stripped_entry_alone_is_refused",
     [{k: v for k, v in E1.items() if k != "expense"}], []),
] + MERGE_CASES_EXTRA + [
    # §9.5. The expense re-pushed with its amount changed no longer belongs
    # under the genuine entry's id, so it is refused at ingress rather than
    # winning rule 2 and rewriting the bill on every device.
    ("a_forged_amount_under_an_honest_id_is_refused",
     [E1], lambda log: [forgery_of(log[0],
                                   expense=dict(log[0]["expense"],
                                                amount=999999))]),

    # `author` was the cheapest member to raise: it is unchecked and sorts
    # between `at` and the payload, so appending one high byte beat any entry
    # under §10.2 rule 2 whatever the payload said.
    ("a_forgery_that_outsorts_by_author_alone_is_refused",
     [E1], lambda log: [forgery_of(log[0], author="anb")]),
]


# Cases whose subject is the id itself. Sealing them would remove what they
# test.
TESTS_THE_ID = {"an_entry_whose_id_is_chosen_rather_than_derived"}


def sealed(entries):
    """Section 9.5 ids, derived. A log that never settles is left as written."""
    return seal_log(entries) or entries


def main():
    out = []

    for name, raw, bill_id in FOLD_CASES:
        entries = sealed(raw)
        case = {"name": name, "log": entries}
        if bill_id:
            case["billId"] = bill_id
        try:
            r = fold(entries, bill_id)
            r["balances"] = balances(r["bill"])
            case["expect"] = r
        except Refused as e:
            case["error"] = e.code
        out.append(case)

    for name, entry in ENTRY_CASES:
        # A case testing some other rule is sealed first, so section 9.5 is
        # not the reason it refuses. The create-entry id cases are left alone:
        # their id is what they test.
        if entry.get("kind") != "createBill" and name not in TESTS_THE_ID:
            try:
                check_entry(entry)
            except Refused as e:
                if e.code == "entry_id_not_derived":
                    entry = sealed([entry])[0]
        case = {"name": name, "entry": entry}
        try:
            check_entry(entry)
            case["expect"] = {"accepted": True}
        except Refused as e:
            case["error"] = e.code
        out.append(case)

    for name, raw_left, raw_right in MERGE_CASES:
        if callable(raw_right):
            # A forgery keeps the genuine entry's id: it is built from the
            # sealed left side, after §9.5 has fixed what that id is.
            left = sealed(raw_left)
            right = raw_right(left)
        else:
            # Both sides are sealed as one log so a copy appearing on both
            # keeps one id, which is what makes the union idempotent.
            both = sealed(raw_left + raw_right)
            left, right = both[:len(raw_left)], both[len(raw_left):]
        a, refused_a = merge(left, right)
        b, refused_b = merge(right, left)
        assert canonical_json(a) == canonical_json(b), \
            f"{name}: merge is not commutative"
        assert refused_a == refused_b, f"{name}: ingress is not commutative"
        out.append({"name": name, "left": left, "right": right,
                    "expect": {"merged": a, "refused": refused_a}})

    doc = {"description": "Entries, merge and folding. SPEC.md sections 9.4 and 10.",
           "count": len(out), "cases": out}
    p = pathlib.Path(__file__).resolve().parents[2] / "vectors" / "log.json"
    p.write_text(json.dumps(doc, indent=2) + "\n")
    codes = sorted({c["error"] for c in out if "error" in c})
    print(f"{len(out)} cases -> {p.name}")
    print(f"codes: {' '.join(codes)}")


if __name__ == "__main__":
    main()
