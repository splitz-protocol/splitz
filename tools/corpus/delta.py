#!/usr/bin/env python3
"""Generates vectors/delta.json from SPEC.md section 14.5.

What a peer has not seen, and whether it fits in one square. Three answers,
not two: a peer who holds everything and a peer who holds none of a log too
long to encode are opposite states, and an implementation with one value for
both tells somebody their bill is up to date while entries on it have never
reached them.
"""
import json
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from _spec import ADDRESSES, PAYLOAD_CAP, copy_key, delta_for, seal_log  # noqa: E402

KEY = "k" * 43
NONCE = "n" * 22


def at(minute):
    return f"2026-10-28T19:{minute:02d}:00.000Z"


def log(joins=1, expenses=0):
    """A sealed log: one create, `joins` participants, `expenses` expenses."""
    ids = [f"p{i}" for i in range(joins)]
    entries = [{"v": 1, "author": ids[0], "kind": "createBill", "at": at(0),
                "name": "Dinner", "currency": "EUR", "splitMode": "equal",
                "creatorKey": KEY, "nonce": NONCE}]
    for i, pid in enumerate(ids):
        entries.append({"v": 1, "author": pid, "kind": "joinBill",
                        "at": at(i + 1),
                        "participant": {"id": pid, "name": pid.upper(),
                                        "payTo": ADDRESSES[i % len(ADDRESSES)]}})
    for k in range(expenses):
        entries.append({"v": 1, "author": ids[0], "kind": "addExpense",
                        "at": at(joins + k + 1),
                        "expense": {"id": f"x{k}", "description": "dinner",
                                    "paidBy": ids[0], "amount": 9000,
                                    "at": at(joins + k + 1),
                                    "split": {"type": "equal", "among": ids}}})
    sealed = seal_log(entries)
    assert sealed is not None, "this fixture's entries cannot be sealed"
    return sealed


def cases():
    out = []

    def case(name, entries, they_have):
        out.append({"name": name, "log": entries, "theyHave": list(they_have),
                    "expect": delta_for(entries, they_have)})

    small = log(joins=2, expenses=1)
    ids = [e["id"] for e in small]

    case("a_peer_who_has_nothing_gets_every_entry", small, [])
    case("a_peer_who_has_everything_is_missing_nothing", small, ids)
    case("a_peer_missing_one_entry_gets_one", small, ids[:-1])
    case("a_peer_missing_the_middle_gets_the_middle", small,
         ids[:1] + ids[2:])
    # An id the log does not carry says nothing about what is missing.
    case("an_id_the_log_does_not_hold_is_ignored", small,
         ids + ["not-an-entry-of-this-log"])
    # Section 14.5: a peer names a copy, not an id. The union keeps copies by
    # id and signature, so holding a copy under another signature is holding
    # the id and lacking the entry.
    signed = small[:-1] + [dict(small[-1], sig="GENUINE")]
    keys = [copy_key(e) for e in signed]
    case("a_peer_holding_every_copy_is_missing_nothing", signed, keys)
    case("a_peer_holding_a_copy_under_another_signature_lacks_it", signed,
         keys[:-1] + [signed[-1]["id"] + "|FORGED"])
    case("a_peer_naming_a_signed_entry_by_id_alone_is_sent_it", signed,
         [e["id"] for e in signed])
    case("an_empty_log_is_missing_nothing", [], [])
    case("an_empty_log_with_a_peer_who_claims_entries", [], ["whatever"])

    # Section 11.2's cap. A running bill outgrows one square, and a peer who
    # has seen none of it is the case an implementation must not report as
    # current.
    big = log(joins=4, expenses=6)
    case("a_log_past_the_cap_does_not_fit_one_square", big, [])
    # The same log, one entry behind: a delta is what keeps a bill that no
    # longer fits whole inside the cap.
    case("one_entry_behind_on_a_log_past_the_cap",
         big, [e["id"] for e in big[:-1]])

    return out


def main():
    root = pathlib.Path(__file__).resolve().parents[2] / "vectors"
    cs = cases()
    doc = {"description": "What a peer has not seen. SPEC.md section 14.5.",
           "count": len(cs), "cases": cs}
    (root / "delta.json").write_text(json.dumps(doc, indent=2) + "\n")
    states = {}
    for c in cs:
        states[c["expect"]["state"]] = states.get(c["expect"]["state"], 0) + 1
    print(f"{len(cs):3} cases -> delta.json")
    print("    states: " + "  ".join(f"{k}: {v}" for k, v in sorted(states.items())))
    print(f"    cap: {PAYLOAD_CAP}")


if __name__ == "__main__":
    main()
