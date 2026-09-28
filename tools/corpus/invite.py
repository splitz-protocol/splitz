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
    # `b` repeats: the first occurrence wins, so the second id is ignored. A
    # case whose repeated parameter carries two different names proves nothing.
    ("the_first_occurrence_wins",   f"splitz://join?v=1&b={T}&k={K}&b=Zz99"),
    ("a_repeated_name_keeps_the_first", f"splitz://join?v=1&b={T}&k={K}&n=Ana&n=Ben"),
    ("a_repeated_expiry_keeps_the_first", f"splitz://join?v=1&b={T}&k={K}&x=1793000000&x=1"),
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
    # Section 11.1: `k` decodes as unpadded base64url, not merely draws from
    # its alphabet. Five characters hold no whole number of bytes, and a last
    # character with an unused bit set decodes to the same bytes as another.
    ("a_key_whose_length_decodes_to_nothing", f"splitz://join?v=1&b={T}&k=kkkkk"),
    ("a_key_that_is_not_its_bytes_encoding", f"splitz://join?v=1&b={T}&k={K[:-1]}l"),
    ("an_expiry_that_is_not_a_number", f"splitz://join?v=1&b={T}&k={K}&x=soon"),
    ("an_empty_expiry",              f"splitz://join?v=1&b={T}&k={K}&x="),
    # Section 11.1 bounds `x` and `v` at i64::MAX, which is 19 digits.
    ("an_expiry_at_the_bound",       f"splitz://join?v=1&b={T}&k={K}&x=9223372036854775807"),
    ("an_expiry_one_over_the_bound", f"splitz://join?v=1&b={T}&k={K}&x=9223372036854775808"),
    ("an_expiry_at_the_unsigned_bound", f"splitz://join?v=1&b={T}&k={K}&x=18446744073709551615"),
    ("an_expiry_of_twenty_three_digits", f"splitz://join?v=1&b={T}&k={K}&x=99999999999999999999999"),
    ("a_version_at_the_bound",       f"splitz://join?v=9223372036854775807&b={T}&k={K}"),
    ("a_version_one_over_the_bound", f"splitz://join?v=9223372036854775808&b={T}&k={K}"),
    ("a_version_over_thirty_two_bits", f"splitz://join?v=4294967296&b={T}&k={K}"),
    # A Unicode digit is not a DIGIT: U+0663 is ARABIC-INDIC DIGIT THREE.
    ("an_expiry_in_arabic_indic_digits", f"splitz://join?v=1&b={T}&k={K}&x=\u0663"),
    ("a_version_in_arabic_indic_digits", f"splitz://join?v=\u0663&b={T}&k={K}"),
    # Section 11.1. `x` is a bare decimal integer, exactly as `v` is: padding
    # makes one numeral two, and a length bound then disagrees with a value
    # bound about which is which.
    ("a_padded_expiry",              f"splitz://join?v=1&b={T}&k={K}&x=007"),
    ("an_expiry_padded_past_the_bound",
     f"splitz://join?v=1&b={T}&k={K}&x=00000000000000000001"),
    ("an_expiry_of_five_thousand_digits",
     f"splitz://join?v=1&b={T}&k={K}&x={'9' * 5000}"),
    ("a_version_of_five_thousand_digits",
     f"splitz://join?v={'9' * 5000}&b={T}&k={K}"),
]

ENCODE = [
    ("encode_a_plain_invite",   T, K, "", None),
    ("encode_refuses_a_negative_expiry", T, K, "", -1),
    ("encode_with_a_name",      T, K, "Zcon7", None),
    ("encode_escapes_the_name", T, K, "Zcon7 dîner ✨", None),
    ("encode_with_an_expiry",   T, K, "", 1793000000),
    ("encode_refuses_a_key_that_does_not_decode", T, "kkkkk", "", None),
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
        case = {"name": name, "invite": {"billId": t, "key": k, "name": n}}
        if x is not None:
            case["invite"]["expiry"] = x
        try:
            uri = render_invite(t, k, n, x)
        except Refused as r:
            case["error"] = r.code
            out.append(case)
            continue
        # Anything encoded must parse back to what it was built from.
        back = parse_invite(uri)
        assert back["billId"] == t and back["key"] == k and back["name"] == n, \
            f"{name}: round trip lost {back}"
        assert back.get("expiry") == x, f"{name}: round trip lost the expiry"
        case["expect"] = uri
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
