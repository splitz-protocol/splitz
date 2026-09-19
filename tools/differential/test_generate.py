#!/usr/bin/env python3
"""Checks the differential lane's generator.

A lane that always prints "agreement" is worthless, and a generator that
quietly narrowed — one operation kind, one input value, an empty list — would
produce exactly that. These assertions are what stand between the lane saying
something and the lane saying nothing.

Run: python3 tools/differential/test_generate.py
"""
import json
import subprocess
import sys
from pathlib import Path

HERE = Path(__file__).resolve().parent
GENERATE = HERE / "generate.py"

# Every shape the drivers answer. A kind here that the generator never emits is
# a shape no run exercises; a kind it emits that the drivers do not answer
# comes back `unknown_operation` from all of them and agrees vacuously.
EXPECTED_KINDS = {
    "allocate", "split", "rate", "amount", "qchar", "instant",
    "invite", "canonical", "billid",
}


def run(seed, count):
    out = subprocess.run(
        [sys.executable, str(GENERATE), str(seed), str(count)],
        capture_output=True, text=True, check=True,
    ).stdout
    return [json.loads(line) for line in out.splitlines() if line.strip()]


def main():
    failures = []

    def check(name, condition, detail=""):
        if not condition:
            failures.append(f"{name}: {detail}")

    ops = run(1, 1200)

    check("emits the requested count", len(ops) == 1200, f"got {len(ops)}")
    check("ids are 0..n-1 in order",
          [o["id"] for o in ops] == list(range(1200)))

    kinds = {o["op"] for o in ops}
    check("covers every operation shape",
          kinds == EXPECTED_KINDS,
          f"missing {EXPECTED_KINDS - kinds}, unexpected {kinds - EXPECTED_KINDS}")

    # A shape that appears twice in twelve hundred draws is a shape the lane
    # barely explores.
    counts = {k: sum(1 for o in ops if o["op"] == k) for k in kinds}
    thin = {k: n for k, n in counts.items() if n < 20}
    check("no shape is vanishingly rare", not thin, f"{thin}")

    # Determinism: a seed that did not reproduce would make a divergence
    # impossible to re-run.
    check("a seed reproduces exactly", run(1, 200) == ops[:200])
    check("a different seed differs", run(2, 200) != ops[:200])

    # Boundary coverage. The lane exists to find where two languages' integer
    # and string handling part company, which is not in the middle of a range.
    totals = {o["total"] for o in ops if "total" in o}
    for boundary in (2**53 - 1, 2**53, 2**62, 2**63 - 1):
        check(f"reaches {boundary}", boundary in totals)

    texts = {o.get("text", "") for o in ops}
    check("reaches a character outside the BMP",
          any("\U0001F600" in t for t in texts))
    check("reaches a replacement character", any("�" in t for t in texts))

    uris = {o.get("uri", "") for o in ops}
    check("reaches a byte order mark", any("﻿" in u for u in uris))
    check("reaches a non-breaking space", any(" " in u for u in uris))

    instants = {o.get("text", "") for o in ops if o["op"] == "instant"}
    check("reaches a leap second",
          any(":60." in i for i in instants), f"{sorted(instants)[:3]}")

    # Every operation must be JSON a driver can read back unchanged.
    for op in ops[:50]:
        check("round trips through JSON",
              json.loads(json.dumps(op, ensure_ascii=False)) == op)

    if failures:
        print(f"{len(failures)} check(s) failed:")
        for f in failures:
            print(f"  {f}")
        return 1
    print(f"generator: {len(ops)} operations, {len(kinds)} shapes, all checks pass")
    for kind in sorted(counts):
        print(f"  {kind:10} {counts[kind]:4}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
