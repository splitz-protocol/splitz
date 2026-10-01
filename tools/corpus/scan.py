#!/usr/bin/env python3
"""Generates vectors/scan.json from SPEC.md section 9.4.

A bill code carries a log and the bill's key together. When the bill's own
create commits to a key, a code carrying any other key is refused: a key
handed over with a real bill's id opens a version of the bill only its holder
sees.
"""
import json
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from _spec import (ADDRESSES, BILL_PREFIX, Refused, bill_key_digest,  # noqa: E402
                   derive_bill_id, encode_payload, read_scan, seal_log)

CREATOR = "c" * 43
NONCE = "n" * 22
OWN = "A" * 43
STRANGER = "Q" * 43


def at(minute):
    return f"2026-10-28T19:{minute:02d}:00.000Z"


def bill(commits):
    create = {"v": 1, "author": "ana", "kind": "createBill", "at": at(0),
              "name": "Dinner", "currency": "EUR", "splitMode": "equal",
              "creatorKey": CREATOR, "nonce": NONCE}
    if commits:
        create["keyDigest"] = bill_key_digest(OWN)
    create["id"] = derive_bill_id(create)
    join = {"v": 1, "author": "ana", "kind": "joinBill", "at": at(1),
            "participant": {"id": "ana", "name": "Ana", "payTo": ADDRESSES[0]}}
    entries = seal_log([create, join])
    assert entries is not None
    return create["id"], entries


def code(bill_id, entries, key):
    return encode_payload(BILL_PREFIX, {
        "v": 1, "invite": {"v": 1, "b": bill_id, "k": key}, "log": entries})


def cases():
    out = []

    def case(name, text):
        c = {"name": name, "text": text}
        try:
            c["expect"] = read_scan(text)
        except Refused as r:
            c["error"] = r.code
        out.append(c)

    bid, entries = bill(commits=True)
    case("a_bill_code_with_its_own_key_reads", code(bid, entries, OWN))
    case("a_bill_code_with_another_key_is_refused", code(bid, entries, STRANGER))
    # A create that states the bill id and commits to another key, without
    # being the bill's own: its id does not derive. The genuine one decides.
    genuine = next(e for e in entries if e["kind"] == "createBill")
    forged = dict(genuine, keyDigest=bill_key_digest(STRANGER))
    case("a_create_only_stating_the_bill_id_does_not_refuse_its_key",
         code(bid, entries + [forged], OWN))
    case("and_the_bills_own_create_still_refuses_another_key_beside_it",
         code(bid, entries + [forged], STRANGER))
    bid, entries = bill(commits=False)
    case("a_create_committing_to_no_key_reads_with_any",
         code(bid, entries, STRANGER))
    return out


def main():
    cs = cases()
    doc = {"description": "Scanned bill codes and the key their bill was made "
                          "with. SPEC.md section 9.4.",
           "count": len(cs), "cases": cs}
    p = pathlib.Path(__file__).resolve().parents[2] / "vectors" / "scan.json"
    p.write_text(json.dumps(doc, indent=2) + "\n")
    print(f"{len(cs):3} cases -> scan.json")
    for c in cs:
        print("   ", c["name"], c.get("error") or c.get("expect"))


if __name__ == "__main__":
    main()
