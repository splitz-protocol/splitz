#!/usr/bin/env python3
"""Generates vectors/invite.json from SPEC.md section 11.1."""
import json, pathlib, sys
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from _spec import parse_invite, render_invite, Refused, MAX_BILL_ID

K = "k" * 43
T = "Ab3-_xyz"

DECODE = [
    # accepted
    ("a_plain_invite",              f"splitz://join?v=1&b={T}&k={K}"),
    ("a_name_comes_along",          f"splitz://join?v=1&b={T}&k={K}&n=Zcon7"),
    ("a_percent_escaped_name",      f"splitz://join?v=1&b={T}&k={K}&n=Zcon7%20d%C3%AEner"),
    ("an_expiry",                   f"splitz://join?v=1&b={T}&k={K}&x=1793000000"),
    ("no_name_reads_as_empty",      f"splitz://join?v=1&b={T}&k={K}"),
    ("the_first_occurrence_wins",   f"splitz://join?v=1&b={T}&k={K}&t=other"),
    ("a_bill_id_at_the_cap",         f"splitz://join?v=1&b={'A' * MAX_BILL_ID}&k={K}"),
    ("an_unknown_parameter_is_ignored", f"splitz://join?v=1&b={T}&k={K}&z=1"),

    # scan padding: exactly five code points, at the ends only
    ("a_leading_space_is_padding",  f"  splitz://join?v=1&b={T}&k={K}"),
    ("a_trailing_newline_is_padding", f"splitz://join?v=1&b={T}&k={K}\n"),
    ("a_byte_order_mark_is_padding", f"﻿splitz://join?v=1&b={T}&k={K}"),
    ("a_tab_character_and_a_return",          f"\t splitz://join?v=1&b={T}&k={K}\r\n"),

    # refusals
    ("padding_inside_is_content",   f"splitz://join ?v=1&t={T}&k={K}"),
    ("a_non_breaking_space_is_not_padding", f" splitz://join?v=1&b={T}&k={K}"),
    ("the_scheme_is_case_sensitive", f"SPLITZ://join?v=1&t={T}&k={K}"),
    ("the_host_is_case_sensitive",  f"splitz://JOIN?v=1&t={T}&k={K}"),
    ("a_path_is_not_an_invite",     f"splitz://join/x?v=1&t={T}&k={K}"),
    ("a_fragment_is_not_an_invite", f"splitz://join#f?v=1&t={T}&k={K}"),
    ("some_other_scheme",           f"zcash:u1abc?amount=1"),
    ("no_version",                  f"splitz://join?t={T}&k={K}"),
    ("a_padded_version",            f"splitz://join?v=01&b={T}&k={K}"),
    ("a_signed_version",            f"splitz://join?v=+1&t={T}&k={K}"),
    ("a_version_from_the_future",   f"splitz://join?v=2&b={T}&k={K}"),
    ("no_bill_id",                   f"splitz://join?v=1&k={K}"),
    ("an_empty_bill_id",             f"splitz://join?v=1&b=&k={K}"),
    ("a_plus_in_the_bill_id",        f"splitz://join?v=1&b=Ab%2Bc&k={K}"),
    ("a_bill_id_one_over_the_cap",   f"splitz://join?v=1&b={'A' * (MAX_BILL_ID + 1)}&k={K}"),
    ("no_key",                      f"splitz://join?v=1&b={T}"),
    ("an_empty_key",                f"splitz://join?v=1&b={T}&k="),
    ("a_key_in_the_standard_alphabet", f"splitz://join?v=1&b={T}&k=ab%2Bcd%2Fef%3D"),
    ("an_expiry_that_is_not_a_number", f"splitz://join?v=1&b={T}&k={K}&x=soon"),
]

ENCODE = [
    ("encode_a_plain_invite",   T, K, "", None),
    ("encode_with_a_name",      T, K, "Zcon7", None),
    ("encode_escapes_the_name", T, K, "Zcon7 dîner ✨", None),
    ("encode_with_an_expiry",   T, K, "", 1793000000),
]


def main():
    out = []
    for name, uri in DECODE:
        case = {"name": name, "uri": uri}
        try:
            case["expect"] = parse_invite(uri)
        except Refused as r:
            case["error"] = r.code
        out.append(case)

    for name, t, k, n, x in ENCODE:
        uri = render_invite(t, k, n, x)
        # Anything encoded must parse back to what it was built from.
        back = parse_invite(uri)
        assert back["billId"] == t and back["key"] == k and back["name"] == n, \
            f"{name}: round trip lost {back}"
        case = {"name": name, "invite": {"billId": t, "key": k, "name": n},
                "expect": uri}
        if x is not None:
            case["invite"]["expiry"] = x
        out.append(case)

    doc = {"description": "Invite URIs. SPEC.md section 11.1.",
           "count": len(out), "cases": out}
    p = pathlib.Path(__file__).resolve().parents[2] / "vectors" / "invite.json"
    p.write_text(json.dumps(doc, indent=2) + "\n")
    codes = sorted({c["error"] for c in out if "error" in c})
    print(f"{len(out)} cases -> {p.name}")
    print(f"codes covered: {' '.join(codes)}")


if __name__ == "__main__":
    main()
