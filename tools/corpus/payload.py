#!/usr/bin/env python3
"""Generates vectors/payload.json and vectors/sealed.json.

SPEC.md sections 11.2 and 11.3.
"""
import base64, json, pathlib, sys
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from _spec import (encode_payload, decode_payload, parse_sealed_frame, b64url,
                   canonical_json, Refused, PAYLOAD_CAP, NONCE_BYTES, TAG_BYTES)

ENTRY = {"v": 1, "id": "e1", "author": "ana", "kind": "addExpense",
         "at": "2026-10-28T19:30:00.000Z"}


def payload_cases():
    out = []

    # --- encode ---
    for name, prefix, body in [
        ("encode_a_tab",            "splitz1:",  {"v": 1, "log": [ENTRY]}),
        ("encode_a_delta",          "splitzd1:", {"v": 1, "log": [ENTRY]}),
        ("encode_an_empty_log",     "splitz1:",  {"v": 1, "log": []}),
        ("encode_sorts_keys",       "splitz1:",  {"log": [ENTRY], "v": 1}),
        ("encode_past_the_cap",     "splitz1:",  {"v": 1, "log": [ENTRY] * 400}),
        ("encode_refuses_a_float",  "splitz1:",
         {"v": 1, "log": [dict(ENTRY, expense={"amount": 90.0})]}),
        ("encode_refuses_a_nested_float", "splitz1:",
         {"v": 1, "log": [dict(ENTRY, expense={"items": [{"minorUnits": 1.5}]})]}),
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
        ("decode_a_tab",                 good),
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


if __name__ == "__main__":
    main()
