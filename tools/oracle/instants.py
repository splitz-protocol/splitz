#!/usr/bin/env python3
"""Checks §9.3's instants against an RFC 3339 reader nobody here wrote.

SPEC.md:788 says every instant is RFC 3339, UTC. Every other lane in this
repository compares this protocol's three implementations with each other, and
all three were written from one specification by one author — they can agree
and be wrong together. `rust/tests/oracle.rs` is the only outside check and it
covers ZIP 321 rendering. This is the second: CPython's `datetime` module,
written by other people for another purpose, reading the same strings.

§9.3 is deliberately NARROWER than RFC 3339 — no numeric offset, no leap
second, exactly one spelling of the fractional part — so equality is the wrong
relation. What must hold is containment and agreement:

  contained   every instant §9.3 accepts, the stdlib also accepts
  agrees      where both accept, they denote the same moment
  narrower    the strings the stdlib takes and §9.3 refuses are counted, so
              the gap is a number somebody can look at rather than a belief

A string the stdlib refuses and §9.3 accepts is the finding: it means §9.3 has
admitted something that is not RFC 3339 at all, and SPEC.md:788 is false.

Usage: python3 tools/oracle/instants.py
Exit status is 1 on a disagreement, so it can gate a commit.
"""
import datetime
import json
import pathlib
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
sys.path.insert(0, str(ROOT / "tools" / "corpus"))

import _spec  # noqa: E402


def _upper_designators(text):
    """Upper-cases the date/time separator and the zone designator, only.

    CPython's `fromisoformat` is an ISO 8601 reader and refuses a lower-case
    `t` or `z`. RFC 3339 does not: its grammar writes them as the ABNF quoted
    strings "T" and "Z", and RFC 5234 §2.3 makes a quoted string
    case-insensitive. chrono agrees — `chrono-0.4.38/src/format/scan.rs:213`
    matches `Some(&b'Z' | &b'z')`. So this normalises the oracle's input
    rather than narrowing §9.3, and the assertion below is what stops the
    normalisation touching anything else.
    """
    out = list(text)
    if len(out) > 10 and out[10] in "tT":
        out[10] = "T"
    if out and out[-1] in "zZ":
        out[-1] = "Z"
    normalised = "".join(out)
    assert normalised.upper() == text.upper(), \
        f"normalising case changed more than the designators: {text!r}"
    return normalised


def stdlib(text):
    """The moment CPython reads from `text`, or None when it refuses."""
    if not isinstance(text, str):
        return None
    try:
        moment = datetime.datetime.fromisoformat(_upper_designators(text))
    except ValueError:
        return None
    if moment.tzinfo is None:
        return None
    return moment.astimezone(datetime.timezone.utc)


def ours(text):
    """The instant §9.3 reads, or None when it refuses."""
    try:
        return _spec.parse_instant(text)
    except _spec.Refused:
        return None


def candidates():
    """Every instant in the corpus, plus the boundaries §9.3 argues about."""
    seen = set()

    def walk(node, key=None):
        if isinstance(node, dict):
            for k, v in node.items():
                walk(v, k)
        elif isinstance(node, list):
            for item in node:
                walk(item, key)
        elif isinstance(node, str) and key in ("at", "expect") and len(node) >= 10:
            seen.add(node)

    for path in sorted((ROOT / "vectors").glob("*.json")):
        walk(json.loads(path.read_text(encoding="utf-8")))

    seen.update([
        # The grammar's own edges, so the comparison is not only over strings
        # this protocol already emits.
        "2026-10-28T19:30:00.000Z", "2026-10-28t19:30:00.000z",
        "2026-10-28T19:30:00Z", "2026-10-28T19:30:00.1Z",
        "2026-10-28T19:30:00.123456789Z",
        "2024-02-29T00:00:00.000Z", "2026-02-29T00:00:00.000Z",
        "2000-02-29T00:00:00.000Z", "1900-02-29T00:00:00.000Z",
        "2026-12-31T23:59:60.000Z", "2026-13-01T00:00:00.000Z",
        "2026-10-28T24:00:00.000Z", "2026-10-28T19:30:00+01:00",
        "2026-10-28T19:30:00", "2026-10-28 19:30:00.000Z", "2026-10-28",
        "0001-01-01T00:00:00.000Z", "9999-12-31T23:59:59.999Z",
    ])
    return sorted(seen)


def main():
    accepted = refused = narrower = 0
    failures = []

    for text in candidates():
        mine, theirs = ours(text), stdlib(text)
        if mine is None:
            refused += 1
            if theirs is not None:
                narrower += 1
            continue
        accepted += 1
        if theirs is None:
            failures.append(
                f"§9.3 accepts {text!r} and CPython does not read it as RFC 3339 "
                f"— SPEC.md:788 says every instant is RFC 3339")
            continue
        # §9.3 truncates the fraction to milliseconds and never rounds, so the
        # two agree once the same truncation is applied to the oracle's answer.
        #
        # Built field by field rather than with `strftime`: `%Y` pads the year
        # to four digits on some platforms and not on others, so year 1 comes
        # back as "0001" on one machine and "1" on the next, and the oracle
        # reports a disagreement that exists only between two C libraries.
        canonical = (
            f"{theirs.year:04d}-{theirs.month:02d}-{theirs.day:02d}"
            f"T{theirs.hour:02d}:{theirs.minute:02d}:{theirs.second:02d}"
            f".{theirs.microsecond // 1000:03d}Z")
        if canonical != mine:
            failures.append(
                f"{text!r}: §9.3 reads {mine}, CPython reads {canonical}")

    print(f"{accepted + refused} instants: {accepted} accepted by §9.3, "
          f"{refused} refused, of which {narrower} are RFC 3339 that §9.3 "
          f"deliberately narrows away")

    if accepted == 0 or narrower == 0:
        print("  the comparison read nothing on one side — it is broken, "
              "not conclusive")
        return 1
    if failures:
        print(f"\n{len(failures)} DISAGREEMENT(S) WITH THE OUTSIDE READER")
        for line in failures:
            print(f"  {line}")
        return 1
    print("every instant §9.3 accepts is RFC 3339, and denotes the same moment")
    return 0


if __name__ == "__main__":
    sys.exit(main())
