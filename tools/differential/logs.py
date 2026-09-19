#!/usr/bin/env python3
"""Generates sealed logs for the differential lane's fold and merge operations.

The corpus fixes what a handful of logs fold to. This generates logs nobody
wrote an expectation for — including entries whose payload members are the
wrong type, which is the shape that reaches the fold and the decoder rather
than the ingress check — and the drivers run them through all three
implementations.

Every log is sealed, so every id is the digest section 9.5 derives. An entry
built here and then mutated is mutated in a member the digest covers, which is
what makes it a forgery rather than a different entry.
"""
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "corpus"))

import _spec  # noqa: E402

KEY = _spec.b64url(b"k" * 32)
NONCE = _spec.b64url(b"n" * 16)
SIG = "S" * 86
IDS = ["ana", "ben", "cai"]


def at(minute):
    # Deliberately coarse: several entries share one instant, so section
    # 10.2's rules below `at` and `author` are reached. A distinct minute per
    # entry leaves the id and canonical-JSON tiebreakers untested.
    return f"2026-10-28T19:{minute // 2:02d}:00.000Z"


# Values chosen to sit where one language's cast and another's check part
# company, not in the middle of a type.
#
# No float: section 9.3 refuses one in canonical JSON, so an entry carrying a
# float cannot be given an id at all and never reaches a log. That is a layer
# below this one and the corpus already pins it.
WRONG = [None, 5, True, "9999", [], {}, "", "banana"]


def _create(rng):
    return {"v": 1, "author": IDS[0], "kind": "createBill", "at": at(0),
            "name": rng.choice(["D", "Dinner", ""]), "currency": "EUR",
            "splitMode": rng.choice(["equal", "percentage"]),
            "creatorKey": KEY, "nonce": NONCE}


def _join(rng, pid, minute):
    p = {"id": pid}
    if rng.random() < 0.7:
        p["name"] = pid.title()
    if rng.random() < 0.5:
        p["payTo"] = _spec.ADDRESSES[IDS.index(pid) % len(_spec.ADDRESSES)]
    if rng.random() < 0.5:
        p["identityKey"] = KEY
    return {"v": 1, "author": pid, "kind": "joinBill", "at": at(minute),
            "participant": p}


def _expense(rng, author, n, among):
    return {"v": 1, "author": author, "kind": "addExpense", "at": at(n),
            "expense": {"id": f"x{n}", "description": "dinner",
                        "paidBy": rng.choice(among), "amount": rng.choice(
                            [1, 2, 9000, 10 ** 9, 0, -1]),
                        "at": at(n),
                        "split": {"type": "equal", "among": among}}}


def _payment(rng, n, among):
    a, b = rng.sample(among, 2)
    return {"v": 1, "author": a, "kind": "recordPayment", "at": at(n),
            "payment": {"id": f"y{n}", "from": a, "to": b,
                        "amount": rng.choice([1, 4500, 0]),
                        "method": rng.choice(["cash", "shieldedZec", "swap"]),
                        "at": at(n)}}


# Members a reader has to decide about, per payload: the ones it carries plus
# the ones it may carry and this generator otherwise never sets.
_MEMBERS = {
    "expense": ("id", "description", "paidBy", "amount", "at", "currency",
                "split"),
    "payment": ("id", "from", "to", "amount", "at", "currency", "method",
                "zatoshi", "reference", "note"),
    "participant": ("id", "name", "payTo", "identityKey", "payouts"),
    "rate": ("currency", "minorUnitsPerZec", "at", "source"),
    "confirmation": ("paymentId", "method", "reference"),
}


def _corruptions(payload):
    """Every (member, value) this generator can put on `payload`, plus drops.

    Enumerated rather than drawn twice. Drawing a member and then a value
    independently leaves combinations unreached at any practical count, and
    the combination that goes missing is not the one anybody predicts.
    """
    out = [(m, v) for m in _MEMBERS[payload] for v in WRONG]
    out += [(m, _DROP) for m in _MEMBERS[payload]]
    return out


_DROP = object()


def _corrupt(rng, entry):
    """Replaces or removes one payload member, uniformly over the pairs."""
    out = {k: (dict(v) if isinstance(v, dict) else v) for k, v in entry.items()}
    for payload in ("expense", "payment", "participant"):
        if payload in out:
            member, value = rng.choice(_corruptions(payload))
            if value is _DROP:
                out[payload].pop(member, None)
            else:
                out[payload][member] = value
            return out
    return out


def corruptions():
    """Every (payload, member, value) this module can put on a log.

    The list is stable across runs, so a caller can deal it out and know that
    each pair is reached rather than hoping a draw lands on it.
    """
    out = []
    for payload in sorted(_MEMBERS):
        for member, value in _corruptions(payload):
            out.append((payload, member, value))
    return out


def _apply(rng, entries, payload, member, value):
    """Puts one corruption on the entry carrying `payload`, if the log has one."""
    for i, e in enumerate(entries):
        if isinstance(e.get(payload), dict):
            out = dict(e)
            out[payload] = dict(out[payload])
            if value is _DROP:
                out[payload].pop(member, None)
            else:
                out[payload][member] = value
            entries[i] = out
            return True
    return False


def log(rng, corrupt=0, pair=None):
    """One sealed log: a bill, two or three joins, some expenses and payments."""
    among = IDS[:rng.randint(2, 3)]
    entries = [_create(rng)]
    for i, pid in enumerate(among):
        entries.append(_join(rng, pid, i + 1))
    n = 4
    for _ in range(rng.randint(0, 3)):
        entries.append(_expense(rng, rng.choice(among), n, among))
        n += 1
    for _ in range(rng.randint(0, 2)):
        entries.append(_payment(rng, n, among))
        n += 1
    # A confirmation vouches for the first payment, when there is one, and a
    # withdrawal and an amendment reach the section 10.8 and 10.3 passes that
    # nothing else here does.
    pay = next((e for e in entries if "payment" in e), None)
    if pay is not None and rng.random() < 0.5:
        entries.append({"v": 1, "author": pay["payment"]["to"],
                        "kind": "confirmPayment", "at": at(n),
                        "confirmation": {"paymentId": pay["payment"]["id"],
                                         "method": rng.choice(
                                             ["recipientConfirmed", "onChain"]),
                                         "reference": "tx:abc"}})
        n += 1
    if rng.random() < 0.4:
        entries.append({"v": 1, "author": rng.choice(among), "kind": "setRate",
                        "at": at(n),
                        "rate": {"currency": "EUR", "minorUnitsPerZec": 51234,
                                 "at": at(n)}})
        n += 1
    if pair is not None:
        payload, member, value = pair
        _apply(rng, entries, payload, member, value)
    for _ in range(corrupt):
        i = rng.randrange(1, len(entries))
        entries[i] = _corrupt(rng, entries[i])
    sealed = _spec.seal_log(entries)
    if sealed is None:          # two entries one id: not a log that can exist
        return None
    # Withdrawals and amendments name an id, so they are appended after
    # sealing and the log is sealed again to fix their own.
    extra = []
    if rng.random() < 0.45:
        target = rng.choice(sealed[1:]) if len(sealed) > 1 else sealed[0]
        extra.append({"v": 1, "author": rng.choice([target["author"]] + among),
                      "kind": "voidEntry", "at": at(n), "targetId": target["id"]})
        n += 1
    if rng.random() < 0.3:
        target = next((e for e in sealed if "expense" in e), None)
        if target is not None:
            amended = dict(target["expense"])
            amended["amount"] = rng.choice([1, 7, 9000])
            extra.append({"v": 1, "author": rng.choice([target["author"]] + among),
                          "kind": "amendEntry", "at": at(n),
                          "targetId": target["id"], "expense": amended})
            n += 1
    if extra:
        sealed = _spec.seal_log(sealed + extra)
        if sealed is None:
            return None
    if rng.random() < 0.6:
        for e in sealed:
            e["sig"] = SIG
    return sealed


def variants(rng, sealed):
    """Copies of some entries differing only in members the digest excludes.

    Section 9.5 derives an id from the entry with `id`, `sig` and `v` removed,
    so these carry the same id and are what section 10.2's rule 2 has to
    separate. Slicing one log into parts never produces them: every entry in
    the overlap is byte-identical and rule 2 has no work.
    """
    out = []
    for e in sealed:
        copy = dict(e)
        roll = rng.random()
        if roll < 0.4:
            copy["sig"] = SIG
        elif roll < 0.7:
            copy.pop("sig", None)
        elif roll < 0.85:
            copy["sig"] = "T" * 86
        out.append(copy)
    return out
