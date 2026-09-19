#!/usr/bin/env python3
"""Diffs the differential lane's answers against each other.

Every implementation answered the same operation list. This compares their
answers to one another, not to an expectation: an expectation written by one
implementation cannot catch a mistake that implementation is consistent about.

Usage: compare.py <operations> <name>=<answers> [<name>=<answers> ...]
Exit status is 1 when any two disagree, so it can gate a commit.
"""
import json
import sys


def read_answers(path):
    answers = {}
    with open(path, encoding="utf-8") as f:
        for line in f:
            if not line.strip():
                continue
            row = json.loads(line)
            answers[row["id"]] = {k: v for k, v in row.items() if k != "id"}
    return answers


def main():
    if len(sys.argv) < 3:
        print(__doc__.strip(), file=sys.stderr)
        return 2

    ops = {}
    with open(sys.argv[1], encoding="utf-8") as f:
        for line in f:
            if line.strip():
                op = json.loads(line)
                ops[op["id"]] = op

    runs = {}
    for arg in sys.argv[2:]:
        name, _, path = arg.partition("=")
        runs[name] = read_answers(path)

    names = list(runs)
    print(f"{len(ops)} operations, {len(names)} implementations: {', '.join(names)}")

    missing = [
        f"{name} answered {len(runs[name])} of {len(ops)} operations"
        for name in names
        if len(runs[name]) != len(ops)
    ]
    if missing:
        for line in missing:
            print(f"  {line}")
        return 1

    divergences = []
    for op_id in sorted(ops):
        answers = {name: runs[name].get(op_id) for name in names}
        distinct = {json.dumps(a, sort_keys=True) for a in answers.values()}
        if len(distinct) > 1:
            divergences.append((op_id, ops[op_id], answers))

    if not divergences:
        print(f"agreement on all {len(ops)} operations")
        return 0

    shapes = {}
    for op_id, op, answers in divergences:
        shapes.setdefault(op["op"], []).append((op_id, op, answers))

    print(
        f"DIVERGENCE on {len(divergences)} of {len(ops)} operations, "
        f"{len(shapes)} distinct shapes"
    )
    for shape, rows in sorted(shapes.items()):
        op_id, op, answers = rows[0]
        print(f"\n  [{shape}] {len(rows)} operation(s), first is id {op_id}")
        print(f"    input: {json.dumps(op, ensure_ascii=False, sort_keys=True)}")
        for name in names:
            print(f"    {name:14} {json.dumps(answers[name], ensure_ascii=False)}")
    return 1


if __name__ == "__main__":
    sys.exit(main())
