#!/usr/bin/env python3
"""Generates an operation list for the differential lane.

The corpus is generated from one reference, so it cannot contain a case that
reference is self-consistently wrong about. This lane covers what the corpus
structurally cannot: it invents inputs nobody wrote an expectation for, runs
them through every implementation, and diffs the answers against each other
rather than against anybody's expectation.

Usage: generate.py [seed] [count]
"""
import json
import random
import sys

# Values chosen to sit on boundaries rather than in the middle of ranges: a
# wrong answer in the interior is usually wrong at the edge too, and the edge
# is where two languages' integer and string handling diverge.
TOTALS = [
    0, 1, -1, 2, 7, 99, 100, 101, 999, 1000, 123456789,
    2**31 - 1, 2**31, 2**53 - 1, 2**53, 2**62, 2**63 - 1, -(2**63 - 1),
]
WEIGHTS = [
    [1], [1, 1], [1, 1, 1], [1, 2], [1, 2, 4], [0, 1], [1, 0, 1],
    [3, 5, 7, 11], [1] * 13, [2**31, 1], [2**62, 1], [10**9, 10**9],
]
CURRENCIES = ["EUR", "USD", "JPY", "KWD", "MXN", "XAU"]

# Strings that sort differently under UTF-16 and UTF-8, plus the padding and
# escaping cases sections 11.1 and 8.3 fix.
NAMES = [
    "Ana",
    "ben",
    "Zoë",
    "p\U0001F600",
    "p�",
    "",
    " ",
    "a+b",
    "a b",
    "Zcon7 dîner ✨",
    "é" * 60,
    "ß",
    "İ",
    "ﬁ",
]
RATES = [1, 2, 3, 51234, 950000, 10**8, 2**31, 2**53 - 1]

INSTANTS = [
    "2026-10-28T19:30:00.000Z",
    "2026-10-28t19:30:00.0009Z",
    "2027-02-29T00:00:00.000Z",
    "2028-02-29T00:00:00.000Z",
    "2026-12-31T23:59:60.000Z",
    "2026-10-28 19:30:00.000Z",
    "2026-10-28T19:30:00.000+01:00",
    "2026-10-28",
    "0001-01-01T00:00:00.000Z",
    "9999-12-31T23:59:59.999Z",
    "2026-13-01T00:00:00.000Z",
]

INVITES = [
    "splitz://join?v=1&b=Ab3&k=Kk",
    "﻿splitz://join?v=1&b=Ab3&k=Kk",
    " splitz://join?v=1&b=Ab3&k=Kk",
    "splitz://join?v=1&b=a%2Bb&k=Kk",
    "splitz://join?v=01&b=Ab3&k=Kk",
    "  splitz://join?v=1&b=Ab3&k=Kk  ",
    "splitz://join?v=1&b=Ab3&k=Kk&t=other",
    "SPLITZ://join?v=1&t=Ab3&k=Kk",
]

CANONICAL = [
    {"b": 2, "a": [1, -3]},
    {"z": {"y": 1, "x": 2}},
    {"é": 1, "e": 2},
    {"\U0001F600": 1, "�": 2},
    {"a": None, "b": True, "c": ""},
    [],
    {},
]


def operations(seed, count):
    rng = random.Random(seed)
    kinds = [
        "allocate", "split", "rate", "amount", "qchar", "instant",
        "invite", "canonical", "billid",
    ]
    ops = []
    for i in range(count):
        kind = rng.choice(kinds)
        op = {"id": i, "op": kind}

        if kind == "allocate":
            op["total"] = rng.choice(TOTALS)
            op["weights"] = rng.choice(WEIGHTS)

        elif kind == "split":
            op["total"] = rng.choice(TOTALS)
            op["split"] = rng.choice([
                {"type": "equal",
                 "among": rng.sample(["ana", "ben", "cai", "dee"], rng.randint(1, 4))},
                {"type": "exact", "amounts": {"ana": 40, "ben": 60}},
                {"type": "percentage", "basisPoints": {"ana": 3333, "ben": 6667}},
                {"type": "shares",
                 "shareCounts": {"ana": rng.randint(0, 5), "ben": rng.randint(0, 5)}},
                {"type": "itemized", "extraMinorUnits": rng.choice([0, 1, 500]),
                 "items": [{"minorUnits": rng.choice([0, 1, 5200]),
                            "sharedBy": ["ana", "ben"]}]},
            ])

        elif kind == "rate":
            op["minorUnits"] = abs(rng.choice(TOTALS))
            op["minorUnitsPerZec"] = rng.choice(RATES)
            op["currency"] = rng.choice(CURRENCIES)
            op["rounding"] = rng.choice(["up", "down", "nearest"])

        elif kind == "amount":
            op["zatoshi"] = rng.choice(TOTALS)

        elif kind == "qchar":
            op["text"] = rng.choice(NAMES)

        elif kind == "instant":
            op["text"] = rng.choice(INSTANTS)

        elif kind == "invite":
            op["uri"] = rng.choice(INVITES)

        elif kind == "canonical":
            op["value"] = rng.choice(CANONICAL)

        elif kind == "billid":
            op["entry"] = {
                "v": 1,
                "author": rng.choice(["ana", "ben"]),
                "kind": "createBill",
                "at": "2026-10-28T19:30:00.000Z",
                "name": rng.choice(NAMES[:3]),
                "currency": rng.choice(CURRENCIES[:3]),
                "splitMode": "equal",
                "creatorKey": "k" * 43,
                "nonce": "n" * 22,
            }

        ops.append(op)
    return ops


def main():
    seed = int(sys.argv[1]) if len(sys.argv) > 1 else 1
    count = int(sys.argv[2]) if len(sys.argv) > 2 else 1200
    for op in operations(seed, count):
        print(json.dumps(op, ensure_ascii=False, sort_keys=True))


if __name__ == "__main__":
    main()
