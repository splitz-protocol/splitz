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
KEY_B = b64url(b"b" * 32)
KEY_RIVAL = b64url(b"r" * 32)


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

    # Section 10.3. A payload member the decoder refuses sets ITS OWN entry
    # aside; it never makes the whole bill undecodable. Every case here folds
    # to a document decode_bill accepts, with the honest expense still on it.
    ("an_expense_whose_amount_is_a_string",
     BASE + [{"v": 1, "id": "e4", "author": "ben", "kind": "addExpense",
              "at": AT(6),
              "expense": {"id": "x4", "paidBy": "ben", "amount": "9999",
                          "at": AT(6),
                          "split": {"type": "equal", "among": ["ana", "ben"]}}}],
     C["id"]),
    ("an_expense_that_states_no_amount",
     BASE + [{"v": 1, "id": "e5", "author": "ben", "kind": "addExpense",
              "at": AT(6),
              "expense": {"id": "x5", "paidBy": "ben", "at": AT(6),
                          "split": {"type": "equal", "among": ["ana", "ben"]}}}],
     C["id"]),
    ("an_expense_whose_at_is_not_an_instant",
     BASE + [{"v": 1, "id": "e6", "author": "ben", "kind": "addExpense",
              "at": AT(6),
              "expense": {"id": "x6", "paidBy": "ben", "amount": 10,
                          "at": "soon",
                          "split": {"type": "equal", "among": ["ana", "ben"]}}}],
     C["id"]),
    ("an_expense_whose_currency_is_not_a_string",
     BASE + [{"v": 1, "id": "e7", "author": "ben", "kind": "addExpense",
              "at": AT(6),
              "expense": {"id": "x7", "paidBy": "ben", "amount": 10,
                          "currency": 5, "at": AT(6),
                          "split": {"type": "equal", "among": ["ana", "ben"]}}}],
     C["id"]),
    ("an_expense_whose_currency_is_lower_case",
     BASE + [{"v": 1, "id": "e8", "author": "ben", "kind": "addExpense",
              "at": AT(6),
              "expense": {"id": "x8", "paidBy": "ben", "amount": 10,
                          "currency": "eur", "at": AT(6),
                          "split": {"type": "equal", "among": ["ana", "ben"]}}}],
     C["id"]),
    ("a_payment_that_states_no_id",
     BASE + [{"v": 1, "id": "p3", "author": "ben", "kind": "recordPayment",
              "at": AT(6),
              "payment": {"from": "ben", "to": "ana", "amount": 500,
                          "method": "cash", "at": AT(6)}}], C["id"]),
    ("a_participant_whose_name_is_not_a_string",
     BASE + [{"v": 1, "id": "j4", "author": "ben", "kind": "joinBill",
              "at": AT(6), "participant": {"id": "ben", "name": 5}}], C["id"]),
    ("a_rejoin_whose_pay_to_is_not_a_string",
     BASE + [{"v": 1, "id": "j5", "author": "ben", "kind": "joinBill",
              "at": AT(6),
              "participant": {"id": "ben", "name": "Ben", "payTo": 5}}],
     C["id"]),

    # A payload the decoder that will read it would refuse sets ITS OWN entry
    # aside, whatever the payload kind. Before these, a rate, an amendment's
    # payload, a confirmation's reference and a payout's address each reached
    # a different pass in each implementation.
    ("a_rate_whose_price_is_a_string",
     BASE + [{"v": 1, "id": "r1", "author": "cai", "kind": "setRate", "at": AT(6),
              "rate": {"currency": "EUR", "minorUnitsPerZec": "4200000",
                       "at": AT(6)}}], C["id"]),
    ("a_rate_whose_instant_is_a_number",
     BASE + [{"v": 1, "id": "r2", "author": "ana", "kind": "setRate", "at": AT(6),
              "rate": {"currency": "EUR", "minorUnitsPerZec": 51234, "at": 7}}],
     C["id"]),
    ("a_rate_that_states_no_instant",
     BASE + [{"v": 1, "id": "r3", "author": "ana", "kind": "setRate", "at": AT(6),
              "rate": {"currency": "EUR", "minorUnitsPerZec": 51234}}], C["id"]),
    ("a_rate_whose_source_is_a_list",
     BASE + [{"v": 1, "id": "r4", "author": "ana", "kind": "setRate", "at": AT(6),
              "rate": {"currency": "EUR", "minorUnitsPerZec": 51234,
                       "at": AT(6), "source": []}}], C["id"]),
    ("a_rate_that_is_not_positive",
     BASE + [{"v": 1, "id": "r5", "author": "ana", "kind": "setRate", "at": AT(6),
              "rate": {"currency": "EUR", "minorUnitsPerZec": 0, "at": AT(6)}}],
     C["id"]),
    # An amendEntry carries the payload it replaces and no kind declares it.
    ("an_amendment_whose_expense_is_a_scalar",
     BASE + [{"v": 1, "id": "am1", "author": "ana", "kind": "amendEntry",
              "at": AT(6), "targetId": "e1", "expense": 7}], C["id"]),
    ("an_amendment_whose_participant_is_a_scalar",
     BASE + [{"v": 1, "id": "am2", "author": "ana", "kind": "amendEntry",
              "at": AT(6), "targetId": "j1", "participant": 7}], C["id"]),
    # A reference is a non-empty string; anything else names no transaction.
    ("an_on_chain_reference_that_is_a_number",
     BASE + [{"v": 1, "id": "c10", "author": "ben", "kind": "confirmPayment",
              "at": AT(6),
              "confirmation": {"paymentId": "y1", "method": "onChain",
                               "reference": 5}}], C["id"]),
    ("an_on_chain_reference_that_is_a_list",
     BASE + [{"v": 1, "id": "c11", "author": "ana", "kind": "confirmPayment",
              "at": AT(6),
              "confirmation": {"paymentId": "y1", "method": "onChain",
                               "reference": []}}], C["id"]),
    ("a_confirmation_method_that_is_a_list",
     BASE + [{"v": 1, "id": "c12", "author": "ana", "kind": "confirmPayment",
              "at": AT(6),
              "confirmation": {"paymentId": "y1", "method": []}}], C["id"]),
    # A payout names the address money is sent to.
    ("a_payout_whose_address_is_a_number",
     [C, J_ANA,
      {"v": 1, "id": "j9", "author": "ben", "kind": "joinBill", "at": AT(2),
       "participant": {"id": "ben", "name": "Ben",
                       "payouts": [{"type": "zec", "address": 9}]}},
      E1, P1], C["id"]),
    # The entry that opens the bill states its own members at ingress, so a
    # malformed one never enters the union and the log simply has no create.
    ("a_bill_whose_name_is_a_number",
     [dict(create(), name=7)] + BASE[1:], None),
    ("a_bill_whose_split_mode_is_a_number",
     [dict(create(), splitMode=7)] + BASE[1:], None),

    # §10.1 types a payload; it does not type inside one. The pass that
    # decides whether a withdrawn participant is still named reads `split`
    # before the expense is decoded, so a peer's list there reaches it first.
    ("a_split_that_is_a_list_while_a_participant_is_withdrawn",
     BASE + [{"v": 1, "id": "e9", "author": "ben", "kind": "addExpense",
              "at": AT(6),
              "expense": {"id": "x9", "paidBy": "ben", "amount": 10,
                          "at": AT(6), "split": ["ana", "ben"]}},
             void("v30", "ana", "j2", 7)], C["id"]),
    # Section 10.8 rests on the fold being unable to apply an entry naming
    # somebody who is not on the bill. `paidBy` was checked; the ids a split
    # names were not, so such an expense was kept and the refusal surfaced
    # from balances on a bill that already looked whole.
    ("an_expense_splits_to_somebody_who_never_joined",
     BASE + [{"v": 1, "id": "e11", "author": "ana", "kind": "addExpense",
              "at": AT(6),
              "expense": {"id": "x11", "paidBy": "ana", "amount": 10,
                          "at": AT(6),
                          "split": {"type": "equal",
                                    "among": ["ana", "nobody"]}}}], C["id"]),
    ("an_itemized_share_names_somebody_who_never_joined",
     BASE + [{"v": 1, "id": "e12", "author": "ana", "kind": "addExpense",
              "at": AT(6),
              "expense": {"id": "x12", "paidBy": "ana", "amount": 10,
                          "at": AT(6),
                          "split": {"type": "itemized", "items": [
                              {"minorUnits": 10,
                               "sharedBy": ["ana", "nobody"]}]}}}], C["id"]),
    ("a_split_whose_items_are_scalars",
     BASE + [{"v": 1, "id": "e10", "author": "ben", "kind": "addExpense",
              "at": AT(6),
              "expense": {"id": "x10", "paidBy": "ben", "amount": 10,
                          "at": AT(6),
                          "split": {"type": "itemized", "items": [7]}}},
             void("v31", "ana", "j2", 7)], C["id"]),

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
    # A pair of withdrawals naming each other is not expressible under §9.5:
    # each id is the digest of an entry that states the other's id, so the
    # pair has no fixed point and no log can carry it. What is expressible,
    # and is what §10.8 has to decide, is two withdrawals aimed at one target.
    ("two_withdrawals_naming_one_target",
     BASE + [void("v13", "ana", "e1", 6), void("v14", "ben", "e1", 7)], C["id"]),
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

    # §10.1 and §10.7. A host that verifies hands its verifier to the fold; one
    # that does not gets no binding and no contest, which is the honest answer
    # rather than a claim that none exists.
    ("a_verified_create_binds_the_creator",
     [C, J_ANA, J_BEN], C["id"], [0]),
    ("a_create_whose_signature_fails_opens_no_bill",
     [C, J_ANA, J_BEN], C["id"], []),
    ("a_self_claim_that_verifies_binds_a_key",
     [C, J_ANA, dict(J_BEN, participant={"id": "ben", "name": "Ben",
                                         "identityKey": KEY_B})],
     C["id"], [0, 2]),
    ("a_rival_claim_leaves_the_id_contested",
     [C, J_ANA,
      dict(J_BEN, participant={"id": "ben", "name": "Ben",
                               "identityKey": KEY_B}),
      {"v": 1, "id": "jr", "author": "ben", "kind": "joinBill", "at": AT(7),
       "participant": {"id": "ben", "name": "Ben",
                       "identityKey": KEY_RIVAL}}],
     C["id"], [0, 2, 3]),
    ("no_verifier_decides_nothing",
     [C, J_ANA, dict(J_BEN, participant={"id": "ben", "name": "Ben",
                                         "identityKey": KEY_B})], C["id"]),

    ("an_empty_log", [], None),
    ("a_log_with_no_create", [J_ANA, J_BEN], None),
    ("two_create_entries",
     [C, create(name="Other"), J_ANA], None),
]

def at_depth(total):
    """An entry whose deepest value sits at level `total` (§10.1).

    The entry is level 1, its `expense` level 2 and the `note` level 3, so the
    scalar inside `total - 3` list wrappers is at `total`. The id is derived,
    because an entry that fails §9.5 first never reaches the depth check and
    the case would assert nothing about it.
    """
    value = 1
    for _ in range(total - 3):
        value = [value]
    e = {"v": 1, "author": "ana", "kind": "addExpense", "at": AT(3),
         "expense": {"id": "x1", "paidBy": "ana", "amount": 1, "at": AT(3),
                     "split": {"type": "equal", "among": ["ana"]},
                     "note": value}}
    # Too deep to encode is too deep to derive an id for, so the over-limit
    # cases keep a written id: §10.1 refuses them before §9.5 is reached.
    try:
        e["id"] = derive_entry_id(e)
    except RecursionError:
        e["id"] = "too-deep-to-derive"
    return e


ENTRY_CASES = [
    ("a_create_entry_derives_its_id", C),
    # §10.1's depth bound, at the boundary and one past it. A case nested far
    # past a JSON reader's own recursion limit cannot live here: the file
    # would fail to parse and take the whole corpus down rather than test one
    # rule, which is the same reason §12 gives for `bill_not_scalar_values`.
    # Each suite carries that case itself.
    ("an_entry_at_the_depth_limit", at_depth(64)),
    ("an_entry_one_level_too_deep", at_depth(65)),
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
    # §9.5 derives an id from the entry with `id`, `sig` and `v` removed, so
    # two copies that rule 2 cannot separate differ in exactly those members.
    # Copies differing anywhere else are different entries with different ids
    # and never meet under one id at all.
    ("two_signed_copies_resolve_by_canonical_order",
     [dict(J_ANA, sig="A" * 86)], [dict(J_ANA, sig="B" * 86)]),
    ("disjoint_logs_union", [J_ANA], [J_BEN]),

    # Removing a payload member makes an entry sort higher under §9.3, so
    # without §10.1 at ingress the stripped copy would win rule 2 and take the
    # expense off the bill on every device. The copy has to keep the genuine
    # entry's id to meet it under one id, which is what the callable form
    # builds: §9.5 fixes that id first.
    ("a_stripped_entry_never_enters_the_union",
     [E1], lambda left: [{k: v for k, v in left[0].items() if k != "expense"}]),
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
    """Section 9.5 ids, derived.

    Raises rather than falling back to the entries as written. A fixture that
    cannot be sealed is a fixture whose subject the corpus never reaches: the
    unsealed copy is refused whole at ingress, so the case goes green while
    asserting nothing about the rule it is named for.
    """
    out = seal_log(entries)
    if out is None:
        raise AssertionError(
            "seal_log cannot build this log: every id is a digest of the entry "
            "that carries it, so a pair of entries naming each other has no "
            "fixed point")
    return out


def main():
    out = []

    for spec in FOLD_CASES:
        name, raw, bill_id = spec[0], spec[1], spec[2]
        # A fourth element lists the entry ids the host is taken to have
        # verified. Absent, the fold is driven with no verifier at all, which
        # is the shape every other case uses.
        verifies = spec[3] if len(spec) > 3 else None
        entries = sealed(raw)
        case = {"name": name, "log": entries}
        if bill_id:
            case["billId"] = bill_id
        verify = None
        if verifies is not None:
            ok = {entries[i]["id"] for i in verifies}
            case["verifies"] = sorted(ok)
            def verify(e, key, ok=ok):
                return e["id"] in ok
        try:
            r = fold(entries, bill_id, verify)
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
