#!/usr/bin/env python3
"""Holds each implementation to its own answers, run against run.

`compare.py` diffs the implementations against each other, which cannot see a
property they all get wrong the same way: three readers written from one
specification by one author can agree on every input and still be wrong about
the union. A property relates one implementation's answers to each other, so
it fails on a tree where every implementation agrees.

§10.2's union is a set keyed by entry id, and README's claim is that it is
idempotent, commutative and associative, and that two devices seeing different
subsets of one history materialise the same bill. Each `property` operation
carries two runs whose answers must match in the member named below.

Usage: properties.py <operations> <name>=<answers> [<name>=<answers> ...]
Exit status is 1 when a property fails, so it can gate a commit.
"""
import json
import sys

# What each property compares, and what it is a claim about.
COMPARED = {
    "merge_is_idempotent": ("merged", "a log merged with itself is that log"),
    "merge_commutes": ("merged", "which device spoke first decides nothing"),
    "the_union_is_the_same_set": (
        "merged", "one history dealt out two ways is one union"),
    "fold_is_order_independent": (
        "bill", "the bill is a function of the entry set, not of arrival"),
}


def read(path):
    answers = {}
    with open(path, encoding="utf-8") as f:
        for line in f:
            if line.strip():
                row = json.loads(line)
                answers[row["id"]] = row
    return answers


def member(run_answer, name):
    """The compared member of one run's answer, or a marker for a refusal.

    A refusal is compared too: two runs of one property must agree about
    whether they refused, and about the code.
    """
    if "ok" not in run_answer:
        return {"refused": run_answer.get("refused", run_answer.get("crashed"))}
    return run_answer["ok"].get(name)


def main():
    if len(sys.argv) < 3:
        print(__doc__.strip(), file=sys.stderr)
        return 2

    ops = {}
    with open(sys.argv[1], encoding="utf-8") as f:
        for line in f:
            if line.strip():
                op = json.loads(line)
                if op.get("op") == "property":
                    ops[op["id"]] = op

    runs = {}
    for arg in sys.argv[2:]:
        name, _, path = arg.partition("=")
        runs[name] = read(path)

    failures = []
    checked = {k: 0 for k in COMPARED}
    for op_id, op in sorted(ops.items()):
        name = op["property"]
        compared, claim = COMPARED[name]
        for impl, answers in sorted(runs.items()):
            row = answers.get(op_id)
            if row is None or "ok" not in row:
                failures.append(f"{impl} did not answer property {op_id} ({name})")
                continue
            got = [member(a, compared) for a in row["ok"]]
            checked[name] += 1
            if json.dumps(got[0], sort_keys=True) != json.dumps(got[1], sort_keys=True):
                failures.append(
                    f"{impl} breaks {name} at operation {op_id} — {claim}\n"
                    f"      run 1 {compared}: {json.dumps(got[0], sort_keys=True)[:200]}\n"
                    f"      run 2 {compared}: {json.dumps(got[1], sort_keys=True)[:200]}")

    total = sum(checked.values())
    print(f"{len(ops)} property operations over {len(runs)} implementation(s), "
          f"{total} checks")
    for name in sorted(checked):
        print(f"  {name:28} {checked[name]:4}")

    # A property with no operations is a claim nothing is testing.
    empty = [n for n, c in checked.items() if c == 0]
    if empty:
        for n in empty:
            print(f"  NO OPERATIONS reached {n}")
        return 1

    if failures:
        print(f"\nPROPERTY FAILURES: {len(failures)}")
        for line in failures:
            print(f"  {line}")
        return 1
    print("\nevery property holds in every implementation")
    return 0


if __name__ == "__main__":
    sys.exit(main())
