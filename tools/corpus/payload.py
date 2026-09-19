#!/usr/bin/env python3
"""Generates vectors/payload.json and vectors/sealed.json.

SPEC.md sections 11.2 and 11.3.
"""
import base64, json, pathlib, sys
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from _spec import (ADDRESSES, encode_payload, decode_payload, parse_sealed_frame,
                   b64url, canonical_json, check_entry, seal_log, Refused,
                   PAYLOAD_CAP, NONCE_BYTES, TAG_BYTES)

AT = "2026-10-28T19:30:00.000Z"
ENTRY = {"v": 1, "id": "e1", "author": "ana", "kind": "addExpense", "at": AT}

KEY = b64url(b"k" * 32)          # 43 characters
NONCE = b64url(b"n" * 16)        # 22 characters
SIG = "S" * 86


def signed_bill(ids, names, addresses, expenses, bill_name="Dinner"):
    """A signed bill of `len(ids)` participants and `expenses` expenses.

    `addresses` gives every participant a real mainnet payout address, which is
    what a settlement needs and what sets the size: a bill nobody can be paid
    on is not the bill a wallet plans against.
    """
    create = {"v": 1, "author": ids[0], "kind": "createBill", "at": AT,
              "name": bill_name, "currency": "EUR", "splitMode": "equal",
              "creatorKey": KEY, "nonce": NONCE}
    entries = [create]
    for i, pid in enumerate(ids):
        p = {"id": pid}
        if names:
            p["name"] = names[i]
        if addresses:
            p["payTo"] = ADDRESSES[i % len(ADDRESSES)]
        # §10.7 binds the creator by the createBill's own creatorKey; only a
        # joiner states an identity key.
        if i:
            p["identityKey"] = KEY
        entries.append({"v": 1, "author": pid, "kind": "joinBill", "at": AT,
                        "participant": p})
    for k in range(expenses):
        entries.append({"v": 1, "author": ids[0], "kind": "addExpense", "at": AT,
                        "expense": {"id": f"x{k}", "description": "dinner",
                                    "paidBy": ids[0], "amount": 9000, "at": AT,
                                    "split": {"type": "equal", "among": ids}}})
    log = seal_log(entries)
    assert log is not None, "this fixture's entries cannot be sealed"
    for e in log:
        e["sig"] = SIG
        check_entry(e)
    return {"v": 1, "invite": {"b": log[0]["id"], "k": KEY, "v": 1}, "log": log}


def smallest_signed_bill(creator, joiner, name, creator_name, joiner_name):
    """Section 11.2's floor: one createBill, two joins, no expenses, signed.

    The body carries the invite, which is what makes a scanned payload a bill
    a joiner can open rather than a log they hold no key to.
    """
    create = {"v": 1, "author": creator, "kind": "createBill", "at": AT,
              "name": name, "currency": "EUR", "splitMode": "equal",
              "creatorKey": KEY, "nonce": NONCE}
    join_creator = {"v": 1, "author": creator, "kind": "joinBill", "at": AT,
                    "participant": {"id": creator,
                                    **({"name": creator_name} if creator_name else {})}}
    # Only the joiner states an identity key: section 10.7 binds the creator
    # by the createBill's own `creatorKey`.
    join_other = {"v": 1, "author": joiner, "kind": "joinBill", "at": AT,
                  "participant": {"id": joiner,
                                  **({"name": joiner_name} if joiner_name else {}),
                                  "identityKey": KEY}}
    log = seal_log([create, join_creator, join_other])
    assert log is not None, "this fixture's entries cannot be sealed"
    for e in log:
        e["sig"] = SIG
        check_entry(e)
    return {"v": 1, "invite": {"b": log[0]["id"], "k": KEY, "v": 1}, "log": log}


def deep(levels):
    """A `splitz1:` payload whose log holds one entry nested `levels` deep."""
    node = 1
    # levels counts the body itself, its log, and the nesting inside it.
    for _ in range(levels - 3):
        node = [node]
    body = {"v": 1, "log": [node]}
    return "splitz1:" + b64url(canonical_json(body).encode("utf-8"))


def payload_cases():
    out = []

    # --- encode ---
    for name, prefix, body in [
        ("encode_a_bill",            "splitz1:",  {"v": 1, "log": [ENTRY]}),
        ("encode_a_delta",          "splitzd1:", {"v": 1, "log": [ENTRY]}),
        ("encode_an_empty_log",     "splitz1:",  {"v": 1, "log": []}),
        ("encode_sorts_keys",       "splitz1:",  {"log": [ENTRY], "v": 1}),
        ("encode_past_the_cap",     "splitz1:",  {"v": 1, "log": [ENTRY] * 400}),
        ("encode_refuses_a_float",  "splitz1:",
         {"v": 1, "log": [dict(ENTRY, expense={"amount": 90.0})]}),
        ("encode_refuses_a_nested_float", "splitz1:",
         {"v": 1, "log": [dict(ENTRY, expense={"items": [{"minorUnits": 1.5}]})]}),
        # Section 11.2's floor. A cap below it refuses every bill there is.
        ("the_smallest_signed_bill", "splitz1:",
         smallest_signed_bill("a", "b", "D", None, None)),
        ("the_smallest_signed_bill_a_wallet_would_write", "splitz1:",
         smallest_signed_bill("ana", "ben", "Dinner", "Ana", "Ben")),
        # Section 11.2's ceiling. A bill carrying no payout address cannot be
        # settled from, so this is the size a wallet actually plans against.
        ("two_payable_participants_and_one_expense", "splitz1:",
         signed_bill(["ana", "ben"], ["Ana", "Ben"], True, 1)),
        ("three_payable_participants_and_no_expenses", "splitz1:",
         signed_bill(["ana", "ben", "cai"], ["Ana", "Ben", "Cai"], True, 0)),
        ("three_payable_participants_and_one_expense", "splitz1:",
         signed_bill(["ana", "ben", "cai"], ["Ana", "Ben", "Cai"], True, 1)),
        ("four_payable_participants_and_no_expenses", "splitz1:",
         signed_bill(["ana", "ben", "cai", "dee"],
                     ["Ana", "Ben", "Cai", "Dee"], True, 0)),
    ]:
        case = {"name": name, "encode": {"prefix": prefix, "body": body}}
        try:
            uri = encode_payload(prefix, body)
            # Anything encoded must decode back to the log it carried.
            back = decode_payload(uri)
            assert back["log"] == body["log"], f"{name}: round trip lost the log"
            case["expect"] = uri
        except Refused as r:
            case["error"] = r.code
        out.append(case)

    good = encode_payload("splitz1:", {"v": 1, "log": [ENTRY]})

    # --- decode ---
    oversize = "splitz1:" + "A" * (PAYLOAD_CAP + 1)
    for name, text in [
        ("decode_a_bill",                 good),
        ("decode_a_delta",               encode_payload("splitzd1:", {"v": 1, "log": []})),
        ("decode_strips_scan_padding",   "﻿  " + good + "\n"),
        ("padding_does_not_count_toward_the_cap",
         " " * 50 + good + " " * 50),
        ("not_a_payload_at_all",         "zcash:u1abc?amount=1"),
        ("a_prefix_with_nothing_after",  "splitz1:"),
        ("base64_that_does_not_decode",  "splitz1:!!!!"),
        ("a_body_that_is_not_json",      "splitz1:" + b64url(b"not json")),
        ("a_body_that_is_not_an_object", "splitz1:" + b64url(b"[1,2,3]")),
        ("no_version",                   "splitz1:" + b64url(b'{"log":[]}')),
        ("a_version_written_as_a_string","splitz1:" + b64url(b'{"v":"1","log":[]}')),
        ("a_version_from_the_future",    "splitz1:" + b64url(b'{"v":2,"log":[]}')),
        ("no_log_at_all",                "splitz1:" + b64url(b'{"v":1}')),
        ("a_log_that_is_not_a_list",     "splitz1:" + b64url(b'{"v":1,"log":{}}')),
        ("one_byte_past_the_cap",        oversize),
        # Section 11.2. `v` is bounded as section 11.1 bounds the invite's:
        # without it each reader's integer type decides what one QR means.
        ("a_version_one_past_the_bound",
         "splitz1:" + b64url(b'{"v":9223372036854775808,"log":[]}')),
        ("a_version_of_twenty_six_digits",
         "splitz1:" + b64url(b'{"v":99999999999999999999999999,"log":[]}')),
        # Section 11.2. Only the bill prefix carries an invite, and only an
        # object is one.
        ("an_invite_that_is_a_string",
         "splitz1:" + b64url(b'{"v":1,"log":[],"invite":"a string"}')),
        ("an_invite_that_is_a_number",
         "splitz1:" + b64url(b'{"v":1,"log":[],"invite":5}')),
        ("an_invite_that_is_a_list",
         "splitz1:" + b64url(b'{"v":1,"log":[],"invite":[1,2]}')),
        ("an_invite_on_a_bill_payload",
         "splitz1:" + b64url(b'{"v":1,"log":[],"invite":{"b":"Ab3","k":"Kk","v":1}}')),
        # Section 11.2's depth limit, at the boundary and past it, and past
        # the depth a JSON library gives up at on its own.
        ("a_body_at_the_depth_limit",    deep(64)),
        ("a_body_one_level_too_deep",    deep(65)),
        ("a_body_nested_two_hundred_deep", deep(200)),
        ("a_delta_carries_no_invite",
         "splitzd1:" + b64url(b'{"v":1,"log":[],"invite":{"b":"Ab3","k":"Kk","v":1}}')),
    ]:
        case = {"name": name, "payload": text}
        try:
            case["expect"] = decode_payload(text)
        except Refused as r:
            case["error"] = r.code
        out.append(case)
    return out


def sealed_cases():
    def frame(version, nonce_len=NONCE_BYTES, body_len=TAG_BYTES + 8):
        return b64url(bytes([version]) + b"n" * nonce_len + b"c" * body_len)

    out = []
    for name, text in [
        ("a_well_formed_frame",        frame(1)),
        ("the_shortest_legal_frame",   frame(1, NONCE_BYTES, TAG_BYTES)),
        ("a_version_of_zero",          frame(0)),
        ("a_version_from_the_future",  frame(2)),
        ("too_short_to_hold_a_nonce",  b64url(bytes([1]) + b"n" * 10)),
        ("too_short_to_hold_a_tag",    b64url(bytes([1]) + b"n" * NONCE_BYTES + b"c" * 4)),
        ("not_base64url",              "!!!not base64!!!"),
        ("an_empty_frame",             ""),
    ]:
        case = {"name": name, "frame": text}
        try:
            case["expect"] = parse_sealed_frame(text)
        except Refused as r:
            case["error"] = r.code
        out.append(case)
    return out


def main():
    root = pathlib.Path(__file__).resolve().parents[2] / "vectors"
    for fname, desc, cases in [
        ("payload.json", "Scanned payloads. SPEC.md section 11.2.", payload_cases()),
        ("sealed.json", "Sealed entry frames. SPEC.md section 11.3.", sealed_cases()),
    ]:
        doc = {"description": desc, "count": len(cases), "cases": cases}
        (root / fname).write_text(json.dumps(doc, indent=2) + "\n")
        codes = sorted({c["error"] for c in cases if "error" in c})
        print(f"{len(cases):3} cases -> {fname}")
        print(f"    codes: {' '.join(codes)}")
        for case in cases:
            if case["name"].startswith(("the_smallest_signed_bill",
                                        "two_payable", "three_payable",
                                        "four_payable")):
                if "expect" in case:
                    body = len(case["expect"]) - len("splitz1:")
                    print(f"    size: {case['name']} is {body} of {PAYLOAD_CAP}")
                else:
                    print(f"    size: {case['name']} is {case['error']}")


if __name__ == "__main__":
    main()
