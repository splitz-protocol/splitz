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
import pathlib
import random
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parent))

import logs as _logs  # noqa: E402

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

# Addresses that separate an ASCII-alphanumeric test from a Unicode-aware one.
# ZIP 321's grammar is `zcashaddress = 1*( ALPHA / DIGIT )`, and RFC 3986's
# ALPHA and DIGIT are ASCII, so every entry here carrying a letter or digit
# outside ASCII is refused.
ADDRESSES = [
    "u1abc",
    "Ab3",
    "t1MJpRjRDKmNbFLkBFtYnPU2S9TgJph3Yc",
    "0",
    "\u00e9",              # LATIN SMALL LETTER E WITH ACUTE
    "a\u00b2",             # SUPERSCRIPT TWO: numeric, not a digit
    "\uff12",              # FULLWIDTH DIGIT TWO
    "\u00df",              # LATIN SMALL LETTER SHARP S
    "a-b",
    "a b",
    "",
]

# MAX_ZATOSHI is 21e6 ZEC in zatoshi; 18 digits is the fiat ceiling.
ZATOSHI = [1, 2, 10**8, 2_100_000_000_000_000, 2_100_000_000_000_001, 0, -1]
MEMOS = [None, "", "hi", "\u2728", "\u00e9" * 200, "\u00e9" * 300]
LABELS = [None, "", "Ana", "\u00e9" * 60, "a+b"]
MESSAGES = [None, "", "dinner", "a b", "\U0001F600"]
FIATS = [None, ["EUR", 1], ["EUR", 0], ["eur", 5], ["EUR", 10**17], ["EUR", 10**18]]

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
    # The bounds section 11.1 puts on `v` and `x`. i64::MAX is 19 digits;
    # u64::MAX is 20 and is over the bound.
    "splitz://join?v=1&b=Ab3&k=Kk&x=0",
    "splitz://join?v=1&b=Ab3&k=Kk&x=1793000000",
    "splitz://join?v=1&b=Ab3&k=Kk&x=9223372036854775807",
    "splitz://join?v=1&b=Ab3&k=Kk&x=9223372036854775808",
    "splitz://join?v=1&b=Ab3&k=Kk&x=18446744073709551615",
    "splitz://join?v=1&b=Ab3&k=Kk&x=18446744073709551616",
    "splitz://join?v=1&b=Ab3&k=Kk&x=99999999999999999999999",
    "splitz://join?v=1&b=Ab3&k=Kk&x=007",
    "splitz://join?v=1&b=Ab3&k=Kk&x=",
    "splitz://join?v=9223372036854775807&b=Ab3&k=Kk",
    "splitz://join?v=9223372036854775808&b=Ab3&k=Kk",
    "splitz://join?v=4294967296&b=Ab3&k=Kk",
    "splitz://join?v=99999999999999999999999&b=Ab3&k=Kk",
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
        "invite", "canonical", "billid", "request", "fold", "merge",
        "property",
    ]
    ops = []
    pairs = _logs.corruptions()
    fold_seen = 0
    # A log operation whose log cannot be built is skipped and redrawn, so the
    # list is always `count` long and its ids are dense: compare.py keys
    # answers by id and test_generate.py asserts both.
    while len(ops) < count:
        kind = rng.choice(kinds)
        op = {"id": len(ops), "op": kind}

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

        elif kind == "request":
            op["includeFiat"] = rng.choice([True, False])
            op["payments"] = [
                {
                    "address": rng.choice(ADDRESSES),
                    "zatoshi": rng.choice(ZATOSHI),
                    "memo": rng.choice(MEMOS),
                    "label": rng.choice(LABELS),
                    "message": rng.choice(MESSAGES),
                    "fiat": rng.choice(FIATS),
                }
                for _ in range(rng.randint(1, 3))
            ]

        elif kind == "fold":
            # The corruption list is DEALT to fold operations rather than
            # drawn for each one. A draw shared with the merge kind reaches
            # barely a third of the pairs at the count this lane is run at,
            # and the pairs it misses are not the ones anybody predicts.
            pair = pairs[fold_seen % len(pairs)] if pairs else None
            fold_seen += 1
            log = _logs.log(rng, corrupt=rng.choice([0, 0, 1]), pair=pair)
            if log is None:
                continue
            op["log"] = log

        elif kind == "merge":
            log = _logs.log(rng, corrupt=rng.choice([0, 1]))
            if log is None:
                continue
            # Two devices holding overlapping views of one log, the shared
            # entries differing in `sig` — which is what §10.2's rule 2 has to
            # separate. Slicing one log gives an overlap of byte-identical
            # entries and rule 2 no work at all.
            cut = rng.randrange(1, len(log)) if len(log) > 1 else 1
            op["parts"] = [log[:cut + 1], _logs.variants(rng, log[cut:])]

        elif kind == "property":
            # A property relates one implementation's answers to each other,
            # which is what the cross-implementation diff cannot see: three
            # implementations can agree on every input and all be wrong about
            # the union. Each run below is answered exactly as its own
            # operation would be, and `properties.py` checks the relation.
            log = _logs.log(rng, corrupt=rng.choice([0, 0, 1]))
            if log is None or len(log) < 2:
                continue
            # Every entry twice, the copies differing only in members §9.5's
            # digest excludes, so each id reaches §10.2's resolution with two
            # candidates. Disjoint parts make every one of these properties
            # true for free: nothing has to be resolved, so nothing about
            # resolution is tested.
            full = log + _logs.variants(rng, log)
            rng.shuffle(full)
            shuffled = list(full)
            rng.shuffle(shuffled)
            which = rng.choice(["merge_is_idempotent", "merge_commutes",
                                "the_union_is_the_same_set",
                                "fold_is_order_independent"])
            op["property"] = which
            if which == "merge_is_idempotent":
                op["runs"] = [{"op": "merge", "parts": [full]},
                              {"op": "merge", "parts": [full, full]}]
            elif which == "merge_commutes":
                cut = rng.randrange(1, len(full))
                a, b = full[:cut], full[cut:]
                op["runs"] = [{"op": "merge", "parts": [a, b]},
                              {"op": "merge", "parts": [b, a]}]
            elif which == "the_union_is_the_same_set":
                left, right = _logs.partitions(rng, full)
                op["runs"] = [{"op": "merge", "parts": left},
                              {"op": "merge", "parts": right}]
            else:
                op["runs"] = [{"op": "fold", "log": full},
                              {"op": "fold", "log": shuffled}]

        ops.append(op)
    return ops


def main():
    seed = int(sys.argv[1]) if len(sys.argv) > 1 else 1
    count = int(sys.argv[2]) if len(sys.argv) > 2 else 1200
    for op in operations(seed, count):
        print(json.dumps(op, ensure_ascii=False, sort_keys=True))


if __name__ == "__main__":
    main()
