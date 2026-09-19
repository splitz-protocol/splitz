#!/usr/bin/env python3
"""Answers the differential lane's operation list from the reference.

Reads one JSON operation per line on stdin and writes one JSON answer per line
on stdout, in the shape `dart/tool/differential.dart` and
`rust/examples/differential.rs` write. Every implementation answers the same
list, and `tools/differential/compare.py` diffs the answers against each other
rather than against anybody's expectation.

The reference writes every expectation in `vectors/`, so a corpus case cannot
contradict it. This lane is the only place its answers are checked by something
that did not come from it.

An exception that is not a refusal is reported as `crashed` rather than raised,
so one operation's failure leaves the remaining answers comparable.
"""
import json
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).resolve().parents[1] / "corpus"))

import _spec  # noqa: E402


def attempt(body):
    """Runs `body` and returns its value, or the refusal code that stopped it."""
    try:
        return {"ok": body()}
    except _spec.Refused as e:
        return {"refused": e.code}
    except Exception as e:  # noqa: BLE001 - reported, not swallowed
        return {"crashed": type(e).__name__}


def answer(op):
    kind = op.get("op")

    if kind == "allocate":
        return attempt(lambda: _spec.allocate(op["total"], op["weights"]))

    if kind == "split":
        return attempt(lambda: _spec.split(op["total"], op["split"]))

    if kind == "rate":
        return attempt(lambda: _spec.fiat_to_zatoshi(
            op["minorUnits"],
            {"currency": op["currency"],
             "minorUnitsPerZec": op["minorUnitsPerZec"],
             "at": "2026-10-28T19:30:00.000Z"},
            None,
            op["rounding"],
        ))

    if kind == "amount":
        return attempt(lambda: _spec.render_amount(op["zatoshi"]))

    if kind == "qchar":
        return attempt(lambda: _spec.qchar(op["text"]))

    if kind == "instant":
        return attempt(lambda: _spec.parse_instant(op["text"]))

    if kind == "invite":
        def invite():
            i = _spec.parse_invite(op["uri"])
            return {"billId": i["billId"], "key": i["key"], "name": i["name"],
                    "expiry": i.get("expiry")}
        return attempt(invite)

    if kind == "canonical":
        return attempt(lambda: _spec.canonical_json(op["value"]))

    if kind == "request":
        return attempt(lambda: _spec.render_uri(op["payments"], op["includeFiat"]))

    if kind == "fold":
        def fold():
            r = _spec.fold(op["log"])
            # The bill goes through the decoder: a fold that returns a
            # document its own decoder refuses is the defect this op exists
            # to catch, and it must show as a divergence rather than a crash.
            _spec.decode_bill(r["bill"])
            return {"bill": r["bill"], "setAside": r["setAside"],
                    "withdrawn": r["withdrawn"]}
        return attempt(fold)

    if kind == "merge":
        def merge():
            merged, refused = _spec.merge(*op["parts"])
            # The entries themselves: §10.2 rule 2 decides which copy under
            # one id survives, and an id list is the same either way.
            return {"merged": merged, "refused": refused}
        return attempt(merge)

    if kind == "billid":
        return attempt(lambda: _spec.derive_bill_id(op["entry"]))

    return {"refused": "unknown_operation"}


def main():
    out = sys.stdout
    for line in sys.stdin:
        if not line.strip():
            continue
        op = json.loads(line)
        result = answer(op)
        out.write(json.dumps({"id": op["id"], **result}, ensure_ascii=False) + "\n")


if __name__ == "__main__":
    main()
