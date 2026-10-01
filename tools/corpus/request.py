#!/usr/bin/env python3
"""Generates vectors/request.json from SPEC.md sections 8.7 and 14.6."""
import json, pathlib, sys
sys.path.insert(0, str(pathlib.Path(__file__).parent))
from _spec import render_uri, read_request, check_proposal, b64url, Refused, ADDRESSES

A = ADDRESSES[0]
B = ADDRESSES[1]

ONE = render_uri([{"address": A, "zatoshi": 36843}])
TWO = render_uri([{"address": A, "zatoshi": 7004}, {"address": B, "zatoshi": 9246}])

READ = [
    # accepted: whatever render_uri writes reads back to what it was given
    ("one_payment", ONE),
    ("two_payments", TWO),
    ("every_parameter", render_uri([{"address": A, "zatoshi": 7004, "memo": "thanks",
                                     "label": "Zcon7 dîner ✨", "message": "dinner"}])),
    ("fiat_on_each_payment", render_uri(
        [{"address": A, "zatoshi": 7004, "fiat": ("EUR", 1000)},
         {"address": B, "zatoshi": 9246, "fiat": ("EUR", 1500)}], include_fiat=True)),
    ("fiat_on_one_payment_only", render_uri(
        [{"address": A, "zatoshi": 7004, "fiat": ("EUR", 1000)},
         {"address": B, "zatoshi": 9246}], include_fiat=True)),
    ("a_label_at_the_cap", render_uri([{"address": A, "zatoshi": 1, "label": "é" * 48}])),
    ("an_empty_memo", render_uri([{"address": A, "zatoshi": 1, "memo": ""}])),
    ("the_largest_amount_there_is", f"zcash:{A}?amount=21000000"),
    ("a_two_digit_index", render_uri([{"address": A, "zatoshi": i + 1} for i in range(11)])),

    # refused: not a request at all
    ("another_scheme", f"bitcoin:{A}?amount=1"),
    ("the_scheme_is_case_sensitive", f"ZCASH:{A}?amount=1"),
    ("no_query", f"zcash:{A}"),
    ("an_empty_query", f"zcash:{A}?"),
    ("a_trailing_ampersand", f"zcash:{A}?amount=1&"),
    ("a_parameter_with_no_name", f"zcash:{A}?=1"),
    ("a_parameter_with_no_value_sign", f"zcash:{A}?amount"),

    # refused: a request, not in the form section 8 writes
    ("a_required_parameter", f"zcash:{A}?amount=1&req-note=x"),
    ("an_unknown_parameter", f"zcash:{A}?amount=1&foo=bar"),
    ("a_repeated_parameter", f"zcash:{A}?amount=1&amount=2"),
    ("index_zero_is_not_written", f"zcash:?address.0={A}&amount.0=1"),
    ("an_index_with_a_leading_zero", f"zcash:?address={A}&amount=1&address.01={B}&amount.01=1"),
    ("an_index_past_9999", f"zcash:?address={A}&amount=1&address.10000={B}&amount.10000=1"),
    ("a_gap_in_the_indices", f"zcash:?address={A}&amount=1&address.2={B}&amount.2=1"),
    ("the_address_written_twice", f"zcash:{A}?address={A}&amount=1"),
    ("a_payment_with_no_address", f"zcash:?amount=1"),
    ("a_payment_with_no_amount", f"zcash:{A}?label=Ana"),
    ("one_payment_in_the_indexed_form", f"zcash:?address={A}&amount=1"),
    ("parameters_out_of_order", f"zcash:{A}?label=Ana&amount=1"),
    ("an_amount_with_trailing_zeros", f"zcash:{A}?amount=0.50"),
    ("an_amount_with_a_bare_point", f"zcash:{A}?amount=1."),
    ("an_amount_with_a_leading_zero", f"zcash:{A}?amount=01"),
    ("an_amount_with_nine_places", f"zcash:{A}?amount=0.000000001"),
    ("an_amount_with_a_sign", f"zcash:{A}?amount=+1"),
    ("a_lower_case_escape", f"zcash:{A}?amount=1&label=d%c3%aener"),
    ("an_escape_that_is_not_utf8", f"zcash:{A}?amount=1&label=%FF"),
    ("a_cut_off_escape", f"zcash:{A}?amount=1&label=Ana%2"),
    ("an_escape_that_is_not_hex", f"zcash:{A}?amount=1&label=%G1"),
    ("a_raw_space", f"zcash:{A}?amount=1&label=A na"),
    ("a_raw_non_ascii_character", f"zcash:{A}?amount=1&label=dîner"),
    ("a_label_past_the_cap", f"zcash:{A}?amount=1&label=" + "a" * 97),
    ("a_padded_memo", f"zcash:{A}?amount=1&memo=dGhhbmtz" + "=="[:0] + "YQ=="),
    ("a_memo_of_impossible_length", f"zcash:{A}?amount=1&memo=A"),
    ("a_memo_with_stray_bits", f"zcash:{A}?amount=1&memo=AB"),
    # §8.7: reading comes first, so a memo that does not read is refused as
    # such even when the request is also wrong in another way.
    ("a_memo_with_stray_bits_on_a_zero_amount", f"zcash:{A}?amount=0&memo=AB"),
    ("a_standard_base64_memo", f"zcash:{A}?amount=1&memo=a+b/"),
    ("a_lower_case_fiat_code", f"zcash:{A}?amount=1&fiat=eur:100"),
    ("a_fiat_with_no_count", f"zcash:{A}?amount=1&fiat=EUR:"),

    # refused by the writer: the reader hands a value on and it says why
    ("a_zero_amount", f"zcash:{A}?amount=0"),
    ("more_zec_than_exists", f"zcash:{A}?amount=21000000.00000001"),
    ("an_address_the_grammar_refuses", "zcash:a-b?amount=1"),
    ("a_zero_fiat_price", f"zcash:{A}?amount=1&fiat=EUR:0"),
    ("a_memo_past_the_cap", f"zcash:{A}?amount=1&memo=" + b64url(b"m" * 513)),
]

CHECK = [
    ("the_same_payments", TWO,
     [{"address": A, "zatoshi": 7004}, {"address": B, "zatoshi": 9246}]),
    ("order_is_not_significant", TWO,
     [{"address": B, "zatoshi": 9246}, {"address": A, "zatoshi": 7004}]),
    ("a_reader_that_kept_only_the_first", TWO,
     [{"address": A, "zatoshi": 7004}]),
    ("nothing_proposed", TWO, []),
    ("an_output_nobody_asked_for", ONE,
     [{"address": A, "zatoshi": 36843}, {"address": B, "zatoshi": 1}]),
    ("the_right_address_the_wrong_amount", ONE,
     [{"address": A, "zatoshi": 36844}]),
    ("an_address_in_another_case", ONE,
     [{"address": A.upper(), "zatoshi": 36843}]),
    ("one_output_cannot_answer_for_two_payments",
     render_uri([{"address": A, "zatoshi": 5}, {"address": A, "zatoshi": 5}]),
     [{"address": A, "zatoshi": 5}]),
    ("two_equal_payments_two_equal_outputs",
     render_uri([{"address": A, "zatoshi": 5}, {"address": A, "zatoshi": 5}]),
     [{"address": A, "zatoshi": 5}, {"address": A, "zatoshi": 5}]),
    ("a_request_this_protocol_did_not_write", f"zcash:{A}?amount=1&foo=bar",
     [{"address": A, "zatoshi": ZAT} for ZAT in [100_000_000]]),
]


def out_payment(p):
    o = {"address": p["address"], "zatoshi": p["zatoshi"]}
    if "fiat" in p:
        o["fiat"] = f"{p['fiat'][0]}:{p['fiat'][1]}"
    if "memo" in p:
        o["memo"] = b64url(p["memo"])
    for k in ("label", "message"):
        if k in p:
            o[k] = p[k]
    return o


def main():
    cases = []
    names = set()
    for name, uri in READ:
        assert name not in names, name
        names.add(name)
        case = {"name": name, "uri": uri}
        try:
            case["expect"] = [out_payment(p) for p in read_request(uri)]
        except Refused as e:
            case["error"] = e.code
        cases.append(case)
    for name, uri, outputs in CHECK:
        assert name not in names, name
        names.add(name)
        case = {"name": name, "uri": uri, "outputs": outputs}
        try:
            missing, unexpected = check_proposal(uri, outputs)
            case["expect"] = {
                "missing": [{"address": p["address"], "zatoshi": p["zatoshi"]} for p in missing],
                "unexpected": unexpected,
            }
        except Refused as e:
            case["error"] = e.code
        cases.append(case)

    root = pathlib.Path(__file__).resolve().parents[2]
    doc = {
        "description": "SPEC.md §8.7 and §14.6: a request read back, and the payments a wallet is about to sign checked against it.",
        "count": len(cases),
        "cases": cases,
    }
    out = root / "vectors" / "request.json"
    out.write_text(json.dumps(doc, indent=2, ensure_ascii=False) + "\n")
    print(f"{out.relative_to(root)}: {len(cases)} cases")


if __name__ == "__main__":
    main()
