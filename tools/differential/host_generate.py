#!/usr/bin/env python3
"""Operation lists for the host layer, answered by both implementations.

The protocol's corpus is generated from one reference, so it cannot hold a case
that reference is self-consistently wrong about. The host layer has no corpus
at all: nothing in `vectors/` covers a curve operation, because a vector cannot
carry a private key. This generates the inputs instead, and the two host
implementations answer them independently.

Usage: python3 tools/differential/host_generate.py <seed> <count>
"""
import base64
import json
import random
import sys


def b64(raw: bytes) -> str:
    return base64.urlsafe_b64encode(raw).decode().rstrip("=")


def a_seed(rng: random.Random) -> str:
    return b64(bytes(rng.randrange(256) for _ in range(32)))


def an_entry(rng: random.Random) -> dict:
    """An entry shaped the way §9.1 writes one, with the fields §10.6 covers."""
    entry = {
        "v": 1,
        "id": b64(bytes(rng.randrange(256) for _ in range(16))),
        "author": rng.choice(["ana", "ben", "cal", "dee"]),
        "kind": rng.choice(["joinBill", "addExpense", "recordPayment"]),
        "at": f"2026-10-28T19:{rng.randrange(60):02}:{rng.randrange(60):02}Z",
    }
    if rng.random() < 0.5:
        entry["participant"] = {
            "id": entry["author"],
            "name": rng.choice(["Ana", "Ben", "Cal", ""]),
        }
    if rng.random() < 0.3:
        entry["amount"] = rng.randrange(-10_000, 10_000)
    if rng.random() < 0.3:
        # Non-ASCII, because §9.3's canonical form is where two encoders
        # disagree first.
        entry["note"] = rng.choice(["café", "naïve", "日本語", "a\u0000b", "🧾"])
    return entry


def a_key_ish(rng: random.Random) -> str:
    """Something a wallet might be handed as a key. Most are not valid."""
    kind = rng.randrange(7)
    if kind == 0:
        return b64(bytes(rng.randrange(256) for _ in range(32)))
    if kind == 1:
        return base64.urlsafe_b64encode(
            bytes(rng.randrange(256) for _ in range(32))).decode()  # padded
    if kind == 2:
        return ""
    if kind == 3:
        return "A" * rng.choice([42, 43, 44, 45])
    if kind == 4:
        return b64(bytes(rng.randrange(256) for _ in range(rng.randrange(1, 40))))
    if kind == 5:
        return "not base64url!!"
    return "+/" + "A" * 41  # standard base64's alphabet, not url's


def main() -> int:
    seed = int(sys.argv[1]) if len(sys.argv) > 1 else 1
    count = int(sys.argv[2]) if len(sys.argv) > 2 else 400
    rng = random.Random(seed)
    seeds = [a_seed(rng) for _ in range(8)]
    out = sys.stdout
    for _ in range(count):
        op = rng.choice([
            "public_key", "sign_entry", "verify", "identity_seed",
            "well_formed_key", "b64_round_trip",
        ])
        if op == "public_key":
            json.dump({"op": op, "seed": rng.choice(seeds)}, out)
        elif op == "sign_entry":
            json.dump({"op": op, "seed": rng.choice(seeds),
                       "entry": an_entry(rng)}, out)
        elif op == "verify":
            json.dump({"op": op, "seed": rng.choice(seeds),
                       "entry": an_entry(rng),
                       "key": a_key_ish(rng),
                       "signWith": rng.choice(seeds),
                       "tamper": rng.random() < 0.3}, out)
        elif op == "identity_seed":
            json.dump({"op": op, "viewingKey": rng.choice(
                ["", "uview1abc", "uview1def", "日本語", "u" * 200])}, out)
        elif op == "well_formed_key":
            json.dump({"op": op, "key": a_key_ish(rng)}, out)
        else:
            json.dump({"op": op, "text": a_key_ish(rng)}, out)
        out.write("\n")
    return 0


if __name__ == "__main__":
    sys.exit(main())
