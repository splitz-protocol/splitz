#!/usr/bin/env python3
"""Generates vectors/signing.json from SPEC.md section 10.6.

The message is stated as text, not as a verdict. A case asserting that a
signature verified would pass in two implementations that disagree about which
bytes they signed, each checking its own perfectly.
"""
import json
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from _spec import ADDRESSES, signing_message  # noqa: E402

AT = "2026-10-28T19:30:00.000Z"
KEY = "k" * 43
SIG = "S" * 86

CASES = [
    ("a_join_with_no_signature", {
        "v": 1, "id": "j1", "author": "ana", "kind": "joinBill", "at": AT,
        "participant": {"id": "ana", "name": "Ana"}}),

    ("the_signature_is_not_covered", {
        "v": 1, "id": "j1", "author": "ana", "kind": "joinBill", "at": AT,
        "participant": {"id": "ana", "name": "Ana"}, "sig": SIG}),

    ("the_version_is_not_covered", {
        "v": 99, "id": "j1", "author": "ana", "kind": "joinBill", "at": AT,
        "participant": {"id": "ana", "name": "Ana"}}),

    ("keys_are_ordered_by_utf8_bytes", {
        "kind": "joinBill", "at": AT, "author": "ana", "id": "j1",
        "participant": {"name": "Ana", "id": "ana"}}),

    # Section 9.3's string rule: controls escaped, shorthand where RFC 8785
    # has one, and `/`, `&`, `<`, `>`, U+007F and U+2028 written as they are.
    ("strings_are_escaped_as_rfc_8785_writes_them", {
        "v": 1, "id": "j1", "author": "ana", "kind": "joinBill", "at": AT,
        "participant": {"id": "ana",
                        "name": "Fish & chips 1/2 <ok>\t\n\x01\x7f\u2028\"q\\"}}),

    ("the_id_is_covered", {
        "v": 1, "id": "j2", "author": "ana", "kind": "joinBill", "at": AT,
        "participant": {"id": "ana", "name": "Ana"}}),

    ("an_expense", {
        "v": 1, "id": "e1", "author": "ana", "kind": "addExpense", "at": AT,
        "expense": {"id": "x1", "description": "dinner", "paidBy": "ana",
                    "amount": 480000, "currency": "MXN", "at": AT,
                    "split": {"type": "equal", "among": ["ana", "ben"]}}}),

    ("a_payment_naming_an_address", {
        "v": 1, "id": "p1", "author": "ben", "kind": "recordPayment", "at": AT,
        "payment": {"id": "y1", "from": "ben", "to": "ana", "amount": 4500,
                    "currency": "MXN", "method": "shieldedZec", "at": AT,
                    "reference": ADDRESSES[0]}}),

    ("a_name_outside_the_basic_multilingual_plane", {
        "v": 1, "id": "j3", "author": "ana", "kind": "joinBill", "at": AT,
        "participant": {"id": "ana", "name": "Ana \U0001F600"}}),

    ("a_name_that_needs_escaping", {
        "v": 1, "id": "j4", "author": "ana", "kind": "joinBill", "at": AT,
        "participant": {"id": "ana", "name": "Zcon7 dîner ✨"}}),

    ("the_create_entry_covers_its_key_and_nonce", {
        "v": 1, "id": "b1", "author": "ana", "kind": "createBill", "at": AT,
        "name": "Dinner", "currency": "MXN", "splitMode": "equal",
        "creatorKey": KEY, "nonce": "n" * 22}),

    ("a_confirmation", {
        "v": 1, "id": "c1", "author": "ana", "kind": "confirmPayment", "at": AT,
        "confirmation": {"paymentId": "y1", "method": "onChain",
                         "reference": "tx:abc",
                         "record": "cmVjb3JkZGlnZXN0MDAwMA"}}),

    ("a_withdrawal", {
        "v": 1, "id": "v1", "author": "ana", "kind": "voidEntry", "at": AT,
        "targetId": "e1"}),
]


# The bill every case is signed on. An entry does not name its bill, so the
# message carries it: the same entry signed on another bill is another message.
BILL = "g0a5mrH6D5nx5bJ7KrgwVA"


def main():
    out = []
    for name, entry in CASES:
        out.append({"name": name, "billId": BILL, "entry": entry,
                    "expect": signing_message(entry, BILL)})
    # One entry on two bills: the message differs, so a signature made on one
    # bill does not verify on the other.
    out.append({"name": "the_same_entry_on_another_bill",
                "billId": "AAAAAAAAAAAAAAAAAAAAAA", "entry": CASES[0][1],
                "expect": signing_message(CASES[0][1], "AAAAAAAAAAAAAAAAAAAAAA")})

    # The two exclusions, checked rather than asserted: adding a signature or
    # changing the version must leave the message identical.
    base = dict(CASES[0][1])
    unsigned = signing_message(base, BILL)
    assert signing_message(dict(base, sig=SIG), BILL) == unsigned, "sig is covered"
    assert signing_message(dict(base, v=99), BILL) == unsigned, "v is covered"
    # Changing anything else must change it.
    assert signing_message(dict(base, id="other"), BILL) != unsigned, "id is not covered"
    assert signing_message(base, "AAAAAAAAAAAAAAAAAAAAAA") != unsigned, "bill is not covered"

    doc = {"description": "The message an entry's signature covers. "
                          "SPEC.md section 10.6.",
           "count": len(out), "cases": out}
    p = pathlib.Path(__file__).resolve().parents[2] / "vectors" / "signing.json"
    p.write_text(json.dumps(doc, indent=2, ensure_ascii=False) + "\n")
    print(f"{len(out)} cases -> {p.name}")


if __name__ == "__main__":
    main()
