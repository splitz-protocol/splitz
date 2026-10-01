#!/usr/bin/env python3
"""Generates vectors/log.json and vectors/authority.json.

SPEC.md sections 9.4, 10 and 10.5.
"""
import json, pathlib, sys
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from _spec import (MAX_ENTRY_AMOUNT, ADDRESSES, check_entry, derive_bill_id, derive_entry_id,
                   payment_digest, participant_id,
                   seal_log, merge, order, fold, balances,
                   canonical_json, b64url, Refused, stand_in,
                   non_canonical)

def AT(m):
    return f"2026-10-28T19:{m:02d}:00.000Z"

KEY = b64url(b"k" * 32)
NONCE = b64url(b"n" * 16)
SIG = "S" * 86
KEY_B = b64url(b"b" * 32)
KEY_RIVAL = b64url(b"r" * 32)
# A participant who publishes a key is named by the id that key derives
# (section 10.7).
BEN_K = participant_id(KEY_B)


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
# A payment of one cent under the id the bill's payment uses.
P_CENT = {"v": 1, "id": "p9", "author": "ben", "kind": "recordPayment",
          "at": AT(4),
          "payment": {"id": "y1", "from": "ben", "to": "ana", "amount": 1,
                      "method": "cash", "at": AT(4)}}
J_DEE = {"v": 1, "id": "j3", "author": "dee", "kind": "joinBill", "at": AT(2),
         "participant": {"id": "dee", "name": "Dee"}}
BASE = [C, J_ANA, J_BEN, E1, P1]


# A confirmation written with no `record`, where `sealed` would otherwise fill
# one in from the payment it names.
NO_RECORD = object()


def conf(eid, author, method, minute, ref=None, pid="y1", record=None):
    c = {"paymentId": pid, "method": method}
    if ref:
        c["reference"] = ref
    if record is not None:
        c["record"] = record
    return {"v": 1, "id": eid, "author": author, "kind": "confirmPayment",
            "at": AT(minute), "confirmation": c}


def void(eid, author, target, minute):
    return {"v": 1, "id": eid, "author": author, "kind": "voidEntry",
            "at": AT(minute), "targetId": target}


# --- §9.2's three lanes, and the payments that settle them -------------------
#
# Every other payment fixture here is `cash`, which is the one method that
# carries no reference and no zatoshi. A corpus that never sees `shieldedZec`
# or `swap` leaves the two members those methods bring — a transaction and a
# ZEC leg — to be read only by code checking itself.

USDC_ON_BASE = {"type": "swap", "asset": "USDC", "chain": "base",
                "address": "0xcara"}

J_BEN_ZEC = {"v": 1, "id": "l2", "author": "ben", "kind": "joinBill",
             "at": AT(2),
             "participant": {"id": "ben", "name": "Ben",
                             "payouts": [{"type": "zec",
                                          "address": ADDRESSES[1]}]}}
J_CARA_SWAP = {"v": 1, "id": "l3", "author": "cara", "kind": "joinBill",
               "at": AT(3),
               "participant": {"id": "cara", "name": "Cara",
                               "payouts": [USDC_ON_BASE]}}
J_DAN_CASH = {"v": 1, "id": "l4", "author": "dan", "kind": "joinBill",
              "at": AT(4),
              "participant": {"id": "dan", "name": "Dan",
                              "payouts": [{"type": "cash"}]}}


def lane_expense(eid, who, minute):
    """`who` covers 20.00 shared with ana, so ana owes them 10.00."""
    return {"v": 1, "id": eid, "author": who, "kind": "addExpense",
            "at": AT(minute),
            "expense": {"id": f"x-{who}", "paidBy": who, "amount": 2000,
                        "at": AT(minute),
                        "split": {"type": "equal",
                                  "among": sorted(["ana", who])}}}


LANES = [C, J_ANA, J_BEN_ZEC, J_CARA_SWAP, J_DAN_CASH,
         lane_expense("l5", "ben", 5),
         lane_expense("l6", "cara", 6),
         lane_expense("l7", "dan", 7)]


def paid(eid, pid, to, minute, method, amount=1000, ref=None, zatoshi=None):
    payment = {"id": pid, "from": "ana", "to": to, "amount": amount,
               "method": method, "at": AT(minute)}
    if ref is not None:
        payment["reference"] = ref
    if zatoshi is not None:
        payment["zatoshi"] = zatoshi
    return {"v": 1, "id": eid, "author": "ana", "kind": "recordPayment",
            "at": AT(minute), "payment": payment}


LANE_CASES = [
    ("three_payout_lanes_on_one_bill", LANES, C["id"]),
    # A payout type a reader does not define is refused rather than skipped:
    # skipping settles to the next preference down, which is a different
    # address.
    ("a_payout_type_nobody_defines",
     [C, J_ANA,
      {"v": 1, "id": "l8", "author": "ben", "kind": "joinBill", "at": AT(2),
       "participant": {"id": "ben", "name": "Ben",
                       "payouts": [{"type": "giftCard", "address": "g1"}]}}],
     C["id"]),
    ("a_swap_payout_states_the_asset_and_the_chain",
     LANES + [paid("l9", "s1", "cara", 8, "swap",
                   ref="near-intent-7f3a", zatoshi=100000)],
     C["id"]),
    ("a_shielded_payment_carries_its_transaction",
     LANES + [paid("l10", "tx-9:ben", "ben", 8, "shieldedZec", ref="tx-9")],
     C["id"]),
    # §10.5: one transaction paying two people is two records, each with its
    # own id. Under one id the second is set aside and the payment it records
    # is lost, so its payee is still owed and cannot confirm.
    ("one_transaction_paying_two_people",
     LANES + [paid("l11", "tx-9:ben", "ben", 8, "shieldedZec", ref="tx-9"),
              paid("l12", "tx-9:cara", "cara", 8, "shieldedZec", ref="tx-9")],
     C["id"]),
    ("two_recipients_of_one_transaction_under_one_id",
     LANES + [paid("l13", "tx-9", "ben", 8, "shieldedZec", ref="tx-9"),
              paid("l14", "tx-9", "cara", 8, "shieldedZec", ref="tx-9")],
     C["id"]),
    # Each payee vouches for the record addressed to them, in a method §10.5
    # lets them author. The three lanes clear independently.
    ("each_payee_confirms_the_record_addressed_to_them",
     LANES + [paid("l15", "tx-9:ben", "ben", 8, "shieldedZec", ref="tx-9"),
              paid("l16", "s2", "cara", 8, "swap",
                   ref="near-intent-7f3a", zatoshi=100000),
              paid("l17", "c-dan-1", "dan", 8, "cash"),
              conf("l18", "ben", "walletReceived", 9, pid="tx-9:ben"),
              conf("l19", "cara", "recipientConfirmed", 9, pid="s2"),
              conf("l20", "dan", "recipientConfirmed", 9, pid="c-dan-1")],
     C["id"]),
    # A confirmation's whole weight is in who gave it, so one payee cannot
    # clear another's debt.
    ("a_payee_confirms_a_record_addressed_to_somebody_else",
     LANES + [paid("l21", "tx-9:ben", "ben", 8, "shieldedZec", ref="tx-9"),
              conf("l22", "cara", "recipientConfirmed", 9, pid="tx-9:ben")],
     C["id"]),
    # `onChain` is the recipient's, like every method that settles: a
    # shielded payment is visible to nobody else, and a payer or a third party
    # naming a transaction proves nothing about it.
    ("a_third_party_may_not_say_a_transaction_landed",
     LANES + [paid("l23", "tx-9:ben", "ben", 8, "shieldedZec", ref="tx-9"),
              conf("l24", "dan", "onChain", 9, "tx-9", pid="tx-9:ben")],
     C["id"]),
    ("the_recipient_says_a_transaction_landed",
     LANES + [paid("l23", "tx-9:ben", "ben", 8, "shieldedZec", ref="tx-9"),
              conf("l24", "ben", "onChain", 9, "tx-9", pid="tx-9:ben")],
     C["id"]),
    # A swap's ZEC leg is advisory and must still be a real amount.
    ("a_swap_payment_whose_zec_leg_is_zero",
     LANES + [paid("l25", "s3", "cara", 8, "swap",
                   ref="near-intent-7f3a", zatoshi=0)],
     C["id"]),
    ("a_settlement_method_nobody_defines",
     LANES + [paid("l26", "g1", "dan", 8, "giftCard")],
     C["id"]),
]

FOLD_CASES = [
    ("a_bill_with_one_expense", BASE, C["id"]),
    ("a_recorded_payment_is_not_yet_confirmed", BASE, C["id"]),
    ("the_recipient_confirms", BASE + [conf("c1", "ana", "recipientConfirmed", 5)], C["id"]),
    ("a_wallet_saw_it_land", BASE + [conf("c2", "ana", "walletReceived", 5)], C["id"]),
    ("on_chain_with_a_reference",
     BASE + [conf("c3", "ana", "onChain", 5, "tx:abc")], C["id"]),
    ("the_payer_attests_and_it_settles_nothing",
     BASE + [conf("c4", "ben", "payerAttested", 5)], C["id"]),

    # §10.5. A confirmation binds the record it was given for, by digest.
    ("a_confirmation_naming_no_record",
     BASE + [conf("c13", "ana", "recipientConfirmed", 5, record=NO_RECORD)],
     C["id"]),
    # Ben records a cent, Ana confirms it, and Ben amends the record to the
    # whole debt: the confirmation was for a cent.
    ("a_confirmation_of_a_record_amended_since",
     [C, J_ANA, J_BEN, E1, P_CENT,
      conf("c14", "ana", "recipientConfirmed", 5),
      {"v": 1, "id": "a7", "author": "ben", "kind": "amendEntry",
       "at": AT(6), "targetId": "p9",
       "payment": {"id": "y1", "from": "ben", "to": "ana", "amount": 4500,
                   "method": "cash", "at": AT(4)}}],
     C["id"]),
    # The same, by withdrawing the record and writing a larger one under the
    # same payment id.
    ("a_confirmation_reused_after_its_record_is_rewritten",
     [C, J_ANA, J_BEN, E1, P_CENT,
      conf("c15", "ana", "recipientConfirmed", 5),
      void("v10", "ben", "p9", 6),
      {"v": 1, "id": "p10", "author": "ben", "kind": "recordPayment",
       "at": AT(7),
       "payment": {"id": "y1", "from": "ben", "to": "ana", "amount": 4500,
                   "method": "cash", "at": AT(7)}}],
     C["id"]),
    ("the_payer_may_not_confirm_his_own_debt",
     BASE + [conf("c5", "ben", "recipientConfirmed", 5)], C["id"]),
    ("on_chain_without_a_reference",
     BASE + [conf("c6", "ana", "onChain", 5)], C["id"]),
    # One copy of an honest entry with `v` changed keeps the honest id
    # (§9.5). Refused at ingress, it leaves the bill opening as before.
    ("a_copy_whose_version_is_a_fraction", BASE + [dict(E1, v=1.5)], C["id"]),
    ("the_payer_may_not_say_his_own_transaction_landed",
     BASE + [conf("c6b", "ben", "onChain", 5, "not-a-transaction")], C["id"]),
    ("a_confirmation_method_nobody_defines",
     BASE + [conf("c7", "ana", "sawItOnTheNews", 5)], C["id"]),
    ("a_confirmation_for_a_payment_the_bill_lacks",
     BASE + [conf("c8", "ana", "recipientConfirmed", 5, pid="nope")], C["id"]),
    ("a_confirmation_arriving_before_its_payment",
     [C, J_ANA, J_BEN, E1, conf("c9", "ana", "recipientConfirmed", 1), P1], C["id"]),

    # One transaction paying two people, recorded as the same id twice. The
    # second record is refused: a confirmation names one record, and two under
    # one id would let Dee's word settle what Ana is owed.
    ("two_payments_sharing_one_id",
     [C, J_ANA, J_BEN, J_DEE, E1,
      {"v": 1, "id": "p4", "author": "ben", "kind": "recordPayment",
       "at": AT(4),
       "payment": {"id": "tx9", "from": "ben", "to": "ana", "amount": 4500,
                   "method": "cash", "at": AT(4)}},
      {"v": 1, "id": "p5", "author": "ben", "kind": "recordPayment",
       "at": AT(5),
       "payment": {"id": "tx9", "from": "ben", "to": "dee", "amount": 10,
                   "method": "cash", "at": AT(5)}}], C["id"]),

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
     BASE + [{"v": 1, "id": "r1", "author": "ana", "kind": "setRate", "at": AT(6),
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
     BASE + [{"v": 1, "id": "c10", "author": "ana", "kind": "confirmPayment",
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
    # §10.4. An amendment keeps the id its target is about. Ben renaming his
    # own join would take him off the bill and his share of the dinner with
    # him; renaming it and then withdrawing the old name would get past the
    # §10.8 check that reads the target.
    ("a_join_amended_to_another_id",
     [C, J_ANA, J_BEN, E1,
      {"v": 1, "id": "a4", "author": "ben", "kind": "amendEntry",
       "at": AT(6), "targetId": "j2",
       "participant": {"id": "ben-gone", "name": "Ben",
                       "payTo": ADDRESSES[1]}}],
     C["id"]),
    ("a_join_amended_to_another_id_then_withdrawn",
     [C, J_ANA, J_BEN, E1,
      {"v": 1, "id": "a5", "author": "ben", "kind": "amendEntry",
       "at": AT(6), "targetId": "j2",
       "participant": {"id": "ben-gone", "name": "Ben",
                       "payTo": ADDRESSES[1]}},
      void("v9", "ben", "j2", 7)],
     C["id"]),
    ("a_payment_amended_to_another_id",
     BASE + [{"v": 1, "id": "a6", "author": "ben", "kind": "amendEntry",
              "at": AT(6), "targetId": "p1",
              "payment": {"id": "y2", "from": "ben", "to": "ana",
                          "amount": 4500, "method": "cash", "at": AT(4)}}],
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
             {"v": 1, "id": "r2", "author": "ana", "kind": "setRate",
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
     [C, J_ANA, J_BEN], C["id"], [0, 1]),
    ("a_create_whose_signature_fails_opens_no_bill",
     [C, J_ANA, J_BEN], C["id"], []),
    ("a_self_claim_that_verifies_binds_a_key",
     [C, J_ANA, dict(J_BEN, author=BEN_K,
                     participant={"id": BEN_K, "name": "Ben",
                                  "identityKey": KEY_B})],
     C["id"], [0, 1, 2]),
    # §10.7. A key names the id it derives, so a second key's claim to a bound
    # participant is set aside rather than contesting them.
    ("a_rival_claim_binds_nothing",
     [C, J_ANA,
      dict(J_BEN, author=BEN_K, participant={"id": BEN_K, "name": "Ben",
                                             "identityKey": KEY_B}),
      {"v": 1, "id": "jr", "author": BEN_K, "kind": "joinBill", "at": AT(7),
       "participant": {"id": BEN_K, "name": "Ben",
                       "identityKey": KEY_RIVAL}}],
     C["id"], [0, 1, 2, 3]),
    ("a_key_stated_under_an_id_it_does_not_derive_is_set_aside",
     [C, J_ANA, dict(J_BEN, participant={"id": "ben", "name": "Ben",
                                         "identityKey": KEY_B})], C["id"]),
    ("the_creators_record_may_state_any_key",
     [C, dict(J_ANA, participant={"id": "ana", "name": "Ana",
                                  "identityKey": KEY_RIVAL})], C["id"]),
    ("no_verifier_decides_nothing",
     [C, J_ANA, dict(J_BEN, author=BEN_K,
                     participant={"id": BEN_K, "name": "Ben",
                                  "identityKey": KEY_B})], C["id"]),

    ("an_empty_log", [], None),
    ("a_log_with_no_create", [J_ANA, J_BEN], None),
    ("two_create_entries",
     [C, create(name="Other"), J_ANA], None),
]

FOLD_CASES += LANE_CASES

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
    # §10.1. A signature is a string or absent: `null` would sort above every
    # string and win every merge it entered.
    ("a_signature_that_is_not_a_string_is_refused", dict(J_ANA, sig=None)),
    ("a_numeric_signature_is_refused", dict(J_ANA, sig=7)),
    # §10.1. `v` is outside the id, so any value keeps the honest id; one that
    # is not an integer would stop the merge's canonical comparison.
    ("an_entry_whose_version_is_a_fraction", dict(J_ANA, v=1.5)),
    ("an_entry_whose_version_is_a_string", dict(J_ANA, v="1")),
    ("an_entry_whose_version_is_zero", dict(J_ANA, v=0)),
    # §10.1's depth bound, at the boundary and one past it. A case nested far
    # past a JSON reader's own recursion limit cannot live here: the file
    # would fail to parse and take the whole corpus down rather than test one
    # rule, which is the same reason §12 gives for `bill_not_scalar_values`.
    # Each suite carries that case itself.
    ("an_entry_at_the_depth_limit", at_depth(62)),
    ("an_entry_one_level_too_deep", at_depth(63)),
    ("a_create_entry_with_no_key", {k: v for k, v in C.items() if k != "creatorKey"}),
    ("a_create_entry_with_a_short_nonce", dict(C, nonce=b64url(b"n" * 8))),
    # §9.4. A key that is not its bytes' canonical encoding is refused, so one
    # bill has one creatorKey spelling on every reader.
    ("a_create_entry_whose_key_is_not_canonical",
     (lambda e: dict(e, id=derive_bill_id(e)))(
         dict({k: v for k, v in C.items() if k != "id"},
              creatorKey=non_canonical(KEY)))),
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
                       ("confirmPayment", "confirmation")):
    ENTRY_CASES.append((f"a_scalar_{_member}_is_not_a_payload",
                        {"v": 1, "id": "s1", "author": "ana", "kind": _kind,
                         "at": AT(6), _member: 5}))

ENTRY_CASES += [
    # §10.1's order among a create's members: its currency is decided before
    # whether it is bound, so a create wrong in both ways is refused the same
    # way by every reader.
    ("a_create_with_a_bad_currency_and_no_nonce",
     (lambda e: dict(e, id=derive_bill_id(e)))(
         {k: v for k, v in dict(create(), currency="eur").items() if k not in ("id", "nonce")})),
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

    # §10.1. `v` is bounded as every other integer is: 2^63 is past it, and a
    # reader whose parser holds it as a double must refuse it as one that
    # holds it exactly does.
    ("an_entry_whose_version_is_past_64_bits", dict(J_ANA, v=2**63)),

    # §2.2 at ingress. A number past 64 bits is `amount_overflow` however it
    # was written; any other non-integer is refused as §9.3's encoding
    # refuses it.
    ("a_payment_of_two_to_the_sixty_third",
     {"v": 1, "id": "s6", "author": "ben", "kind": "recordPayment", "at": AT(6),
      "payment": {"id": "y6", "from": "ben", "to": "ana", "amount": 2**63,
                  "method": "cash", "at": AT(6)}}),
    ("a_payment_whose_amount_is_a_fraction",
     {"v": 1, "id": "s7", "author": "ben", "kind": "recordPayment", "at": AT(6),
      "payment": {"id": "y7", "from": "ben", "to": "ana", "amount": 1.5,
                  "method": "cash", "at": AT(6)}}),

    # §10.1's checks run in the order it states, so an entry wrong in two
    # ways is refused for the same one everywhere.
    ("a_join_carrying_an_expense_and_no_participant",
     {"v": 1, "id": "s8", "author": "ana", "kind": "joinBill", "at": AT(6),
      "expense": 5}),
    ("a_void_whose_target_is_null",
     {"v": 1, "id": "s9", "author": "ana", "kind": "voidEntry", "at": AT(6),
      "targetId": None}),
]

# A non-string entry id is refused at ingress, and the refusal must carry an
# id without casting one: the refusal path has to be total over every value
# the check refuses.
MERGE_CASES_EXTRA = [
    ("a_non_string_entry_id_is_refused_not_thrown",
     [dict(J_ANA, id=7)], []),
]


# §10.2 and §10.3. Two signed copies of one entry are both kept, and a fold
# that verifies applies one whose signature checks against the author's key.
SIG_A = "A" * 86
SIG_Z = "z" * 86
J_BEN_KEYED = dict(J_BEN, author=BEN_K,
                   participant={"id": BEN_K, "name": "Ben",
                                "payTo": ADDRESSES[1],
                                "identityKey": KEY_B})
E1_K = dict(E1, expense=dict(E1["expense"], split={"type": "equal",
                                                   "among": ["ana", BEN_K]}))
CONFIRM_AS_ANA = {"v": 1, "id": "c1", "author": "ana",
                  "kind": "confirmPayment", "at": AT(5),
                  "confirmation": {"paymentId": "y1",
                                   "method": "recipientConfirmed"}}

FOLD_CASES += [
    ("a_create_whose_signature_was_swapped_still_opens",
     [dict(C, sig=SIG_A), dict(C, sig=SIG_Z), J_ANA, J_BEN],
     C["id"], [("copy", 0), 2]),
    ("a_create_with_no_copy_that_verifies_opens_no_bill",
     [dict(C, sig=SIG_A), dict(C, sig=SIG_Z), J_ANA, J_BEN],
     C["id"], [2]),
    ("unverified_the_copy_that_sorts_higher_applies",
     [dict(C, sig=SIG_A), dict(C, sig=SIG_Z), J_ANA, J_BEN], C["id"]),
    ("an_entry_authored_as_the_bound_creator_that_does_not_verify_is_set_aside",
     [C, J_ANA, J_BEN, E1, P1, CONFIRM_AS_ANA], C["id"], [0, 1, 3]),
    ("and_one_that_verifies_applies",
     [C, J_ANA, J_BEN, E1, P1, CONFIRM_AS_ANA], C["id"], [0, 1, 3, 5]),
    ("an_unsigned_join_written_as_a_bound_participant_is_set_aside",
     [C, J_ANA, J_BEN_KEYED, E1_K,
      {"v": 1, "id": "jf", "author": BEN_K, "kind": "joinBill", "at": AT(8),
       "participant": {"id": BEN_K, "name": "Ben", "payTo": ADDRESSES[2]}}],
     C["id"], [0, 1, 2, 3]),
    ("a_join_for_a_bound_creator_by_anyone_else_is_set_aside",
     [C, J_BEN,
      {"v": 1, "id": "ja", "author": "ben", "kind": "joinBill", "at": AT(3),
       "participant": {"id": "ana", "name": "Ana", "payTo": ADDRESSES[2]}}],
     C["id"], [0]),
    ("an_amendment_written_as_a_bound_participant_is_set_aside",
     [C, J_ANA, J_BEN_KEYED,
      {"v": 1, "id": "am", "author": BEN_K, "kind": "amendEntry",
       "at": AT(8), "targetId": "j2",
       "participant": {"id": BEN_K, "name": "Ben", "payTo": ADDRESSES[2],
                       "identityKey": KEY_B}}],
     C["id"], [0, 1, 2]),
    ("an_amendment_that_changes_an_address_is_reported",
     [C, J_ANA, J_BEN,
      {"v": 1, "id": "am", "author": "ben", "kind": "amendEntry",
       "at": AT(8), "targetId": "j2",
       "participant": {"id": "ben", "name": "Ben", "payTo": ADDRESSES[2]}}],
     C["id"]),
    ("a_swapped_signature_on_a_claim_keeps_its_binding",
     [C, J_ANA, dict(J_BEN_KEYED, sig=SIG_A),
      {"v": 1, "id": "jr", "author": BEN_K, "kind": "joinBill", "at": AT(7),
       "participant": {"id": BEN_K, "name": "Ben",
                       "identityKey": KEY_RIVAL}},
      dict(J_BEN_KEYED, sig=SIG_Z)],
     C["id"], [0, 1, ("copy", 2), 3]),

    # §10.7. A rival claim to a bound participant takes nothing from them:
    # an unsigned confirmation written in their name is still set aside, so
    # the debt it would have cleared stands.
    ("a_rival_claim_does_not_let_an_unsigned_entry_speak_for_a_participant",
     [C, J_ANA, J_BEN_KEYED,
      {"v": 1, "id": "jm", "author": "mal", "kind": "joinBill", "at": AT(2),
       "participant": {"id": "mal", "name": "Mal"}},
      {"v": 1, "id": "eb", "author": BEN_K, "kind": "addExpense", "at": AT(3),
       "expense": {"id": "xb", "paidBy": BEN_K, "amount": 6000, "at": AT(3),
                   "split": {"type": "equal", "among": [BEN_K, "mal"]}}},
      {"v": 1, "id": "qm", "author": "mal", "kind": "recordPayment",
       "at": AT(4),
       "payment": {"id": "ym", "from": "mal", "to": BEN_K, "amount": 3000,
                   "method": "cash", "at": AT(4)}},
      {"v": 1, "id": "jr", "author": BEN_K, "kind": "joinBill", "at": AT(5),
       "participant": {"id": BEN_K, "name": "Ben",
                       "identityKey": KEY_RIVAL}},
      conf("cf", BEN_K, "recipientConfirmed", 6, pid="ym")],
     C["id"], [0, 1, 2, 3, 4, 5, 6]),

    # §10.3. An entry by a bound participant applies only from a copy that
    # verifies against their own key: here the expense verifies against the
    # creator's key alone, which speaks for nobody else.
    ("an_entry_verifies_against_its_own_authors_key",
     [C, J_ANA, J_BEN_KEYED, E1_K,
      {"v": 1, "id": "eb", "author": BEN_K, "kind": "addExpense", "at": AT(5),
       "expense": {"id": "xb", "paidBy": BEN_K, "amount": 600, "at": AT(5),
                   "split": {"type": "equal", "among": ["ana", BEN_K]}}}],
     C["id"], [0, 1, ("key", 2, KEY_B), 3, ("key", 4, KEY_B)]),
    ("and_one_verifying_only_against_another_key_is_set_aside",
     [C, J_ANA, J_BEN_KEYED, E1_K,
      {"v": 1, "id": "eb", "author": BEN_K, "kind": "addExpense", "at": AT(5),
       "expense": {"id": "xb", "paidBy": BEN_K, "amount": 600, "at": AT(5),
                   "split": {"type": "equal", "among": ["ana", BEN_K]}}}],
     C["id"], [0, 1, ("key", 2, KEY_B), 3, ("key", 4, KEY)]),

    # §10.8. What a withdrawal names orders it after what it names, whatever
    # instant its author wrote.
    ("an_undo_dated_before_the_withdrawal_it_names_is_in_force",
     BASE + [void("v1", "ana", "e1", 9), void("v2", "ana", "v1", 8)],
     C["id"]),
    ("two_refused_removals_of_one_participant_are_both_reported",
     BASE + [void("v1", "ben", "j2", 9), void("v2", "ana", "j2", 10)],
     C["id"]),
    ("a_join_rewriting_another_record_is_refused_before_it_is_decoded",
     BASE + [{"v": 1, "id": "jm", "author": "ana", "kind": "joinBill",
              "at": AT(9),
              "participant": {"id": "ben", "name": "Ben", "payTo": 7}}],
     C["id"]),
]


# §2.2. One expense carries at most MAX_ENTRY_AMOUNT in magnitude. A payment
# carries no cap of its own, and a pair total past the 64-bit bound is set
# aside rather than wrapped.
CAP = MAX_ENTRY_AMOUNT
I64_MAX = 2**63 - 1


def expense(eid, author, paid_by, amount, amounts, minute):
    return {"v": 1, "id": eid, "author": author, "kind": "addExpense",
            "at": AT(minute),
            "expense": {"id": "x" + eid, "description": "big",
                        "paidBy": paid_by, "amount": amount, "at": AT(minute),
                        "split": {"type": "exact", "amounts": amounts}}}


def payment(eid, author, frm, to, amount, minute, pid):
    return {"v": 1, "id": eid, "author": author, "kind": "recordPayment",
            "at": AT(minute),
            "payment": {"id": pid, "from": frm, "to": to, "amount": amount,
                        "method": "cash", "at": AT(minute)}}


FOLD_CASES += [
    ("an_expense_at_the_cap_is_applied",
     BASE + [expense("big", "ben", "ana", CAP, {"ben": CAP}, 9)], C["id"]),
    ("an_expense_one_over_the_cap_is_set_aside",
     BASE + [expense("big", "ben", "ana", CAP + 1, {"ben": CAP + 1}, 9)],
     C["id"]),
    ("a_refund_at_the_cap_is_applied",
     BASE + [expense("big", "ben", "ana", -CAP, {"ben": -CAP}, 9)], C["id"]),
    ("a_refund_one_over_the_cap_is_set_aside",
     BASE + [expense("big", "ben", "ana", -CAP - 1, {"ben": -CAP - 1}, 9)],
     C["id"]),
    ("a_payment_past_the_expense_cap_is_applied",
     BASE + [payment("q1", "ben", "ben", "ana", CAP + 1, 9, "ycap")],
     C["id"]),
    ("a_payment_that_would_carry_a_pair_total_out_of_range_is_set_aside",
     [C, J_ANA, J_BEN,
      payment("q1", "ben", "ben", "ana", I64_MAX, 9, "y1"),
      payment("q2", "ben", "ben", "ana", 1, 10, "y2")], C["id"]),
    # §5.1. After E1 ana is owed 4500, so confirming a payment from her moves
    # her balance up by its amount. One that would pass the 64-bit bound stays
    # unconfirmed and its confirmation is set aside; one landing exactly on it
    # applies.
    ("a_confirmation_that_would_carry_a_balance_out_of_range_is_set_aside",
     [C, J_ANA, J_BEN, E1,
      payment("q1", "ana", "ana", "ben", I64_MAX, 9, "y2"),
      conf("k1", "ben", "recipientConfirmed", 10, pid="y2")], C["id"]),
    ("and_one_landing_on_the_bound_applies",
     [C, J_ANA, J_BEN, E1,
      payment("q1", "ana", "ana", "ben", I64_MAX - 4500, 9, "y2"),
      conf("k1", "ben", "recipientConfirmed", 10, pid="y2")], C["id"]),
]


# §10.1 and §10.3. Who may set the rate, and a destination that moves.
FOLD_CASES += [
    ("a_rate_set_by_somebody_not_on_the_bill_is_set_aside",
     BASE + [{"v": 1, "id": "rz", "author": "zed", "kind": "setRate",
              "at": AT(9),
              "rate": {"currency": "EUR", "minorUnitsPerZec": 1,
                       "at": AT(9)}}], C["id"]),
    ("and_one_set_by_a_participant_applies",
     BASE + [{"v": 1, "id": "rb", "author": "ben", "kind": "setRate",
              "at": AT(9),
              "rate": {"currency": "EUR", "minorUnitsPerZec": 1,
                       "at": AT(9)}}], C["id"]),
    ("a_rejoin_that_moves_a_payout_address_is_reported",
     [C, J_ANA,
      dict(J_BEN, participant={"id": "ben", "name": "Ben", "payouts": [
          {"type": "swap", "asset": "USDC", "chain": "base",
           "address": "0xben"}]}),
      {"v": 1, "id": "jb2", "author": "ben", "kind": "joinBill", "at": AT(8),
       "participant": {"id": "ben", "name": "Ben", "payouts": [
           {"type": "swap", "asset": "USDC", "chain": "base",
            "address": "0xmallory"}]}}], C["id"]),
]


# §10.3 step 5. An id its author minted (the author's id, `:`, anything) is
# theirs whatever `at` another entry states; a copy of it in somebody else's
# entry is set aside. An id nobody minted still goes to the first by §10.2.
def owned_expense(eid, author, xid, amount, minute):
    return {"v": 1, "id": eid, "author": author, "kind": "addExpense",
            "at": AT(minute),
            "expense": {"id": xid, "paidBy": author, "amount": amount,
                        "at": AT(minute),
                        "split": {"type": "exact", "amounts": {"ana": amount // 2,
                                                               "ben": amount - amount // 2}}}}


FOLD_CASES += [
    ("an_expense_id_its_author_minted_stands_over_a_backdated_copy",
     BASE + [owned_expense("xa", "ana", "ana:hotel", 30000, 20),
             owned_expense("xb", "ben", "ana:hotel", 2, 8)], C["id"]),
    ("and_an_expense_id_nobody_minted_goes_to_the_first",
     BASE + [owned_expense("xa", "ana", "hotel", 30000, 20),
             owned_expense("xb", "ben", "hotel", 2, 8)], C["id"]),
    ("a_payment_id_its_author_minted_stands_over_a_backdated_copy",
     BASE + [payment("q1", "ben", "ben", "ana", 4500, 20, "ben:t1:ana"),
             payment("q2", "ana", "ben", "ana", 1, 8, "ben:t1:ana")], C["id"]),
    ("a_pair_total_is_bounded_per_author",
     BASE + [payment("q1", "ana", "ben", "ana", I64_MAX, 8, "ana:big"),
             payment("q2", "ben", "ben", "ana", 4500, 20, "ben:t1:ana")], C["id"]),
]

# An author whose id holds `:` mints nothing: `ben:t1` would otherwise mint
# `ben:t1:ana`, the id of Ben's own send record, and its backdated copy would
# set Ben's aside.
def colon_join(eid, pid):
    return {"v": 1, "id": eid, "author": pid, "kind": "joinBill", "at": AT(2),
            "participant": {"id": pid, "name": "Mal"}}


FOLD_CASES += [
    ("a_participant_id_holding_a_colon_mints_no_payment_id",
     BASE + [colon_join("jm", "ben:t1"),
             payment("q1", "ben", "ben", "ana", 4500, 20, "ben:t1:ana"),
             payment("q2", "ben:t1", "ben:t1", "ana", 1, 8, "ben:t1:ana")],
     C["id"]),
    ("and_mints_no_expense_id",
     BASE + [colon_join("jm", "ana:x"),
             owned_expense("xa", "ana", "ana:x:hotel", 30000, 20),
             owned_expense("xb", "ana:x", "ana:x:hotel", 2, 8)], C["id"]),
    ("and_still_records_a_payment_under_an_id_nobody_minted",
     BASE + [colon_join("jm", "ben:t1"),
             payment("q2", "ben:t1", "ben:t1", "ana", 1, 8, "ben:t1:own")],
     C["id"]),
]


def rate(eid, author, per, minute, at=None):
    return {"v": 1, "id": eid, "author": author, "kind": "setRate",
            "at": at or AT(minute),
            "rate": {"currency": "EUR", "minorUnitsPerZec": per,
                     "at": AT(minute)}}


J_CY = {"v": 1, "id": "j5", "author": "cy", "kind": "joinBill", "at": AT(2),
        "participant": {"id": "cy", "name": "Cy"}}

FOLD_CASES += [
    # §10.1 and §10.7. A fold that verifies takes the rate only from a
    # participant whose key it has bound: an unsigned join puts anybody
    # holding the invite on the bill.
    ("a_rate_from_a_participant_with_no_bound_key_is_set_aside",
     BASE + [J_DEE, rate("rd", "dee", 1, 9)], C["id"], [0, 1, 2, 3, 4, 5, 6]),
    ("a_rate_from_a_bound_participant_applies",
     [C, J_ANA, J_BEN_KEYED, E1_K, rate("rb", BEN_K, 1, 9)],
     C["id"], [0, 1, 2, 3, 4]),

    # §10.8. A rate dated far ahead outranks every later one, so the creator
    # may withdraw it as they may an expense; its author still may.
    ("the_creator_withdraws_a_rate_dated_ahead",
     BASE + [rate("ra", "ana", 950000, 6),
             rate("rf", "ben", 1, 7, at="2036-10-28T19:07:00.000Z"),
             void("vr", "ana", "rf", 8)], C["id"]),
    # §10.1. The creator's rate stands over one anybody else dates ahead of
    # it; anybody else's decides only while the creator has set none.
    ("the_creators_rate_stands_over_one_dated_ahead_by_somebody_else",
     BASE + [rate("ra", "ana", 950000, 6),
             rate("rf", "ben", 1, 7, at="2036-10-28T19:07:00.000Z")], C["id"]),
    ("and_the_creators_next_rate_replaces_it",
     BASE + [rate("ra", "ana", 950000, 6),
             rate("rf", "ben", 1, 7, at="2036-10-28T19:07:00.000Z"),
             rate("rb", "ana", 960000, 8)], C["id"]),
    ("a_stranger_may_not_withdraw_a_rate",
     BASE + [J_DEE, rate("ra", "ana", 950000, 6), void("vr", "dee", "ra", 8)],
     C["id"]),

    # §10.2. Entries are ordered by the instant `at` names, not its text: a
    # lower-case `t` sorts after every upper-case one as bytes.
    ("an_earlier_instant_written_in_lower_case_sorts_first",
     BASE + [rate("rl", "ben", 1, 6, at="2026-10-28t19:06:00.000Z"),
             rate("ru", "ana", 950000, 7)], C["id"]),
    ("two_spellings_of_one_instant_are_ordered_by_their_text",
     BASE + [rate("rl", "ana", 1, 6, at="2026-10-28t19:06:00.000z"),
             rate("ru", "ana", 950000, 6)], C["id"]),

    # §10.4. An amendment that cannot be applied is set aside and its target
    # applies as written. Correcting a join into a record nobody can decode
    # would otherwise take its author off the bill, and every expense naming
    # them with it.
    ("a_join_amended_into_a_record_that_cannot_be_decoded_stands",
     BASE + [{"v": 1, "id": "am", "author": "ben", "kind": "amendEntry",
              "at": AT(6), "targetId": "j2",
              "participant": {"id": "ben", "name": 5}}], C["id"]),
    ("an_expense_amended_into_one_that_cannot_be_applied_stands",
     BASE + [{"v": 1, "id": "am", "author": "ana", "kind": "amendEntry",
              "at": AT(6), "targetId": "e1",
              "expense": {"id": "x1", "paidBy": "ana", "amount": "9000",
                          "at": AT(3),
                          "split": {"type": "equal", "among": ["ana", "ben"]}}}],
     C["id"]),
    # The removal check reads both: an amendment dropping somebody from a
    # split, which then cannot be applied, leaves them named by the entry it
    # corrected.
    ("a_failed_amendment_does_not_let_a_participant_go",
     BASE + [void("vp", "ben", "p1", 5),
             {"v": 1, "id": "am", "author": "ana", "kind": "amendEntry",
              "at": AT(6), "targetId": "e1",
              "expense": {"id": "x1", "paidBy": "ana", "amount": "9000",
                          "at": AT(3),
                          "split": {"type": "equal", "among": ["ana"]}}},
             void("vj", "ana", "j2", 7)], C["id"]),

    # §2.2. A balance is symmetric about zero: the most negative 64-bit value
    # has no positive counterpart, so §5.1 and §6 could not form its
    # magnitude. After E1 ben owes 4500; a payment to him he confirms himself
    # that would leave him at exactly that value stays unconfirmed.
    ("a_confirmation_that_would_leave_a_balance_at_the_most_negative_value",
     [C, J_ANA, J_BEN, J_CY, E1,
      {"v": 1, "id": "q1", "author": "ben", "kind": "recordPayment",
       "at": AT(9),
       "payment": {"id": "y2", "from": "cy", "to": "ben",
                   "amount": I64_MAX - 4499, "method": "cash", "at": AT(9)}},
      conf("k1", "ben", "recipientConfirmed", 10, pid="y2")], C["id"]),
    ("and_one_leaving_it_one_above_applies",
     [C, J_ANA, J_BEN, J_CY, E1,
      {"v": 1, "id": "q1", "author": "ben", "kind": "recordPayment",
       "at": AT(9),
       "payment": {"id": "y2", "from": "cy", "to": "ben",
                   "amount": I64_MAX - 4500, "method": "cash", "at": AT(9)}},
      conf("k1", "ben", "recipientConfirmed", 10, pid="y2")], C["id"]),

    # §10.8. A member the target leaves out names nobody. The record below was
    # written with no `from` and amended to name ben; a withdrawal authored as
    # the empty id must not be read as naming that absent member.
    ("a_withdrawal_authored_as_nobody_takes_nothing_off",
     BASE + [{"v": 1, "id": "pn", "author": "ben", "kind": "recordPayment",
              "at": AT(5),
              "payment": {"id": "y5", "to": "ana", "amount": 100,
                          "method": "cash", "at": AT(5)}},
             {"v": 1, "id": "an", "author": "ben", "kind": "amendEntry",
              "at": AT(6), "targetId": "pn",
              "payment": {"id": "y5", "from": "ben", "to": "ana", "amount": 100,
                          "method": "cash", "at": AT(5)}},
             conf("cn", "ana", "recipientConfirmed", 7, pid="y5",
                  record=payment_digest(
                      {"id": "y5", "from": "ben", "to": "ana", "amount": 100,
                       "method": "cash", "at": AT(5)})),
             void("vn", "", "pn", 8)], C["id"]),

    # One id names one expense: an amendment or a withdrawal is written
    # against the expense a reader shows, and two under one id leave it to
    # guess which.
    ("two_expenses_sharing_one_id",
     BASE + [{"v": 1, "id": "e2", "author": "ben", "kind": "addExpense",
              "at": AT(6),
              "expense": {"id": "x1", "paidBy": "ben", "amount": 100,
                          "at": AT(6),
                          "split": {"type": "equal", "among": ["ana", "ben"]}}}],
     C["id"]),
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
    # two copies under one id differ in exactly those members. Two different
    # signatures are both kept: nothing in the pair says which is genuine.
    # Copies differing anywhere else are different entries with different ids
    # and never meet under one id at all.
    ("two_signed_copies_are_both_kept",
     [dict(J_ANA, sig="A" * 86)], [dict(J_ANA, sig="B" * 86)]),
    ("a_copy_repeated_on_both_sides_is_kept_once",
     [dict(J_ANA, sig="A" * 86)], [dict(J_ANA, sig="A" * 86),
                                   dict(J_ANA, sig="B" * 86)]),
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


def with_records(entries):
    """Fills each confirmation's `record` from the first payment in the log
    carrying the id it names, as the wallet confirming it would (10.5). One
    written with NO_RECORD is left without."""
    payments = {}
    for e in entries:
        if e.get("kind") == "recordPayment" and isinstance(e.get("payment"), dict):
            payments.setdefault(e["payment"].get("id"), e["payment"])
    out = []
    for e in entries:
        c = e.get("confirmation")
        if e.get("kind") == "confirmPayment" and isinstance(c, dict):
            c = dict(c)
            if c.get("record") is NO_RECORD:
                del c["record"]
            elif "record" not in c and c.get("paymentId") in payments:
                c["record"] = payment_digest(payments[c["paymentId"]])
            e = dict(e, confirmation=c)
        out.append(e)
    return out


def sealed(entries):
    """Section 9.5 ids, derived.

    Raises rather than falling back to the entries as written. A fixture that
    cannot be sealed is a fixture whose subject the corpus never reaches: the
    unsealed copy is refused whole at ingress, so the case goes green while
    asserting nothing about the rule it is named for.
    """
    out = seal_log(with_records(entries))
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
            # An index verifies every copy of that entry; ("copy", i) only the
            # copy at i, by its signature.
            ok = set()
            for v in verifies:
                if isinstance(v, tuple) and v[0] == "key":
                    # ("key", i, key): that entry verifies against that key
                    # alone.
                    ok.add(f"{entries[v[1]]['id']}@{v[2]}")
                elif isinstance(v, tuple):
                    ok.add(f"{entries[v[1]]['id']}|{entries[v[1]]['sig']}")
                else:
                    ok.add(entries[v]["id"])
            case["verifies"] = sorted(ok)
            verify = stand_in(ok)
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
