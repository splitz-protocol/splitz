#!/usr/bin/env python3
"""Generates vectors/closing.json from SPEC.md sections 10.9 and 14.9.

What a host refuses around a bill's close: a payment started on a bill its
creator has not closed, an expense written on one that is closed, and a close
or a reopening written by anybody but the creator. Each case is a log, who is
acting, and one operation.
"""
import json
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from _spec import (Refused, check_close, check_expense, check_settle,  # noqa: E402
                   fold, reopens)
from log import BASE, E2, OVER, closing, sealed, void  # noqa: E402

OPEN = BASE
CLOSED = BASE + [closing("c1", "ana", 5, OVER)]
REOPENED_BY_EXPENSE = CLOSED + [E2]
WITHDRAWN = CLOSED + [void("v1", "ana", "c1", 6)]


def cases():
    out = []

    def case(name, log, actor, op):
        entries = sealed(log)
        c = {"name": name, "log": entries, "actor": actor, "op": op}
        folded = fold(entries)
        try:
            if op == "settle":
                check_settle(folded)
                c["expect"] = {"accepted": True}
            elif op == "expense":
                check_expense(folded)
                c["expect"] = {"accepted": True}
            elif op == "close":
                check_close(folded, actor)
                c["expect"] = {"accepted": True}
            else:
                c["expect"] = {"reopens": reopens(folded, actor)}
        except Refused as r:
            c["error"] = r.code
        out.append(c)

    case("an_open_bill_is_not_settled", OPEN, "ben", "settle")
    case("a_closed_bill_is_settled", CLOSED, "ben", "settle")
    case("an_expense_after_the_close_reopens_it_for_settling",
         REOPENED_BY_EXPENSE, "ben", "settle")
    case("a_withdrawn_close_leaves_it_unsettled", WITHDRAWN, "ben", "settle")

    case("an_expense_is_written_on_an_open_bill", OPEN, "ben", "expense")
    case("no_expense_is_written_on_a_closed_bill", CLOSED, "ben", "expense")
    case("nor_by_its_creator", CLOSED, "ana", "expense")
    case("once_reopened_an_expense_is_written", WITHDRAWN, "ben", "expense")

    case("the_creator_closes", OPEN, "ana", "close")
    case("nobody_else_closes", OPEN, "ben", "close")
    case("the_creator_closes_again_over_what_now_stands",
         REOPENED_BY_EXPENSE, "ana", "close")

    case("the_creator_reopens_a_closed_bill", CLOSED, "ana", "reopen")
    case("nobody_else_reopens_it", CLOSED, "ben", "reopen")
    case("an_open_bill_has_nothing_to_reopen", OPEN, "ana", "reopen")
    return out


def main():
    root = pathlib.Path(__file__).resolve().parents[2] / "vectors"
    cs = cases()
    doc = {"description": "What a host refuses around a bill's close. "
                          "SPEC.md sections 10.9 and 14.9.",
           "count": len(cs), "cases": cs}
    (root / "closing.json").write_text(json.dumps(doc, indent=2) + "\n")
    errors = sorted({c["error"] for c in cs if "error" in c})
    print(f"{len(cs):3} cases -> closing.json  ({' '.join(errors)})")


if __name__ == "__main__":
    main()
