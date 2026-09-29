#!/usr/bin/env python3
"""Generates vectors/messages.json: a plain-language sentence per SPEC.md §12 code.

The table is the default a host MAY show a person for each refusal (§12). It
is checked against §12's list, so a code added to the specification without a
sentence here fails generation.
"""
import json, pathlib, re, sys

ROOT = pathlib.Path(__file__).resolve().parents[2]

MESSAGES = {
    # §3 allocation
    "empty_weights": "Nobody is sharing this cost.",
    "negative_weight": "A share can't be negative.",
    "zero_weight_sum": "The shares add up to nothing.",
    "allocation_overflow": "This amount is too large to divide.",
    "weight_sum_overflow": "The shares are too large to add up.",
    # §4 split methods
    "negative_share": "A share can't be negative.",
    "amount_overflow": "This amount is too large.",
    "amount_too_large": "This amount is too large.",
    "empty_split": "Pick who shares this cost.",
    "exact_total_mismatch": "The amounts don't add up to the total.",
    "percentage_not_full_scale": "The percentages don't add up to 100.",
    "itemized_no_items": "Add at least one item.",
    "itemized_unassigned_item": "Pick who shares each item.",
    "itemized_total_mismatch": "The items don't add up to the total.",
    # §2, §5, §6
    "currency_mismatch": "This uses a different currency from the bill.",
    "unknown_participant": "That person isn't on this bill.",
    "unknown_entry": "That item isn't on this bill.",
    "duplicate_participant": "That person is already on this bill.",
    "duplicate_payment": "This payment is already recorded.",
    "duplicate_expense": "This expense is already on the bill.",
    "self_payment": "You can't pay yourself.",
    "balances_nonzero_residual": "The balances don't add up. The bill may be damaged.",
    "exact_limit_too_large": "This bill has too many people to settle exactly.",
    # §7 rates
    "rate_currency_mismatch": "This price is in a different currency from the bill.",
    "rate_not_positive": "A price must be more than zero.",
    "negative_amount": "An amount can't be negative.",
    "rate_amount_too_large": "This amount is too large to price.",
    # §9 the bill wire format
    "bill_missing_version": "This bill can't be read.",
    "bill_future_version": "This bill needs a newer app. Update to open it.",
    "bill_type_error": "This bill is damaged.",
    "bill_not_scalar_values": "This bill is damaged.",
    "entry_id_not_derived": "Part of this bill is damaged.",
    "bill_bad_participant_id": "Part of this bill is damaged.",
    "bill_missing_currency": "This bill has no currency.",
    "bill_bad_currency": "This bill's currency isn't valid.",
    "bill_unknown_split_type": "This bill needs a newer app. Update to open it.",
    "bill_unknown_split_mode": "This bill needs a newer app. Update to open it.",
    "bill_unknown_entry_kind": "This bill needs a newer app. Update to open it.",
    "bill_unknown_settlement_method": "This bill needs a newer app. Update to open it.",
    "bill_unknown_payout_method": "This bill needs a newer app. Update to open it.",
    "bill_unknown_confirmation_method": "This bill needs a newer app. Update to open it.",
    "bill_ambiguous_entry": "Part of this bill is damaged.",
    "bill_missing_entry_payload": "Part of this bill is damaged.",
    "canonical_json_float": "This bill holds a number this app can't read.",
    # §10 the log
    "log_empty": "This bill is empty.",
    "log_no_create": "This bill's start is missing. Sync again.",
    "ambiguous_create": "This bill was started twice. It can't be opened.",
    "create_unbound": "This bill's start is damaged.",
    "create_id_not_derived": "This bill's start is damaged.",
    "unauthorized_entry": "Only the person who wrote this can change it.",
    "unauthorized_payment": "Only the payer can record this payment.",
    "unauthorized_confirmation": "Only the person paid can confirm this.",
    "confirmation_missing_reference": "Add the transaction to confirm this.",
    "unknown_payment": "That payment isn't on this bill.",
    "amend_kind_mismatch": "A change must be the same kind as what it changes.",
    "participant_still_named": "This person still has costs or payments on the bill.",
    "participant_id_not_derived": "This person's id doesn't match their key.",
    # §11 invites, payloads and sealing
    "invite_not_an_invite": "This isn't a bill invite.",
    "invite_missing_version": "This invite can't be read.",
    "invite_future_version": "This invite needs a newer app. Update to join.",
    "invite_missing_bill_id": "This invite is incomplete. Ask for a new one.",
    "invite_bad_bill_id": "This invite is damaged. Ask for a new one.",
    "invite_missing_key": "This invite is incomplete. Ask for a new one.",
    "invite_bad_expiry": "This invite is damaged. Ask for a new one.",
    "invite_bad_link": "This invite link can't be made from that address.",
    "payload_not_a_payload": "This isn't a bill code.",
    "payload_damaged": "This code is damaged. Scan it again.",
    "payload_missing_body": "This code is incomplete. Scan it again.",
    "payload_future_version": "This code needs a newer app. Update to scan it.",
    "payload_too_large": "This bill is too big for one code. Share an invite.",
    "sealed_malformed": "An update to this bill is damaged.",
    "sealed_future_version": "An update to this bill needs a newer app.",
    # §8 payment requests
    "zip321_no_payments": "There's nothing to pay.",
    "zip321_too_many_payments": "Too many people to pay at once.",
    "zip321_amount_not_positive": "An amount to pay must be more than zero.",
    "zip321_amount_too_large": "This amount is more ZEC than exists.",
    "zip321_memo_too_large": "This note is too long.",
    "zip321_bad_address": "An address on this bill isn't valid.",
    "zip321_bad_currency_code": "This bill's currency isn't valid.",
    "zip321_fiat_not_positive": "An amount must be more than zero.",
    "zip321_fiat_too_many_digits": "This amount is too large.",
    "zip321_no_address": "Someone on this bill has no address to be paid at.",
    "zip321_not_canonical": "This payment request wasn't made by this bill.",
    "zip321_memo_undeliverable": "A note can't be sent to this address.",
    # §8.6 addresses
    "address_invalid": "This isn't a Zcash address.",
    # §14.8 paying by a lower preference
    "payout_not_declared": "They haven't added that way to be paid.",
}


def section_12_codes():
    spec = (ROOT / "SPEC.md").read_text()
    start = spec.index("## 12. Error codes")
    end = spec.index("**The code is part of the protocol", start)
    return set(re.findall(r"`([a-z0-9_]+)`", spec[start:end]))


def main():
    listed = section_12_codes()
    missing = sorted(listed - MESSAGES.keys())
    extra = sorted(MESSAGES.keys() - listed)
    if missing or extra:
        sys.exit(f"§12 and the messages disagree: missing {missing}, extra {extra}")
    for code, text in MESSAGES.items():
        if not text or len(text) > 80 or not text.endswith("."):
            sys.exit(f"{code}: a message is one short sentence ending in a period")
    cases = [{"name": code, "code": code, "expect": text}
             for code, text in sorted(MESSAGES.items())]
    doc = {
        "description": "SPEC.md §12: the plain-language sentence a host MAY show for each code.",
        "count": len(cases),
        "cases": cases,
    }
    out = ROOT / "vectors" / "messages.json"
    out.write_text(json.dumps(doc, indent=2, ensure_ascii=False) + "\n")
    print(f"{out.relative_to(ROOT)}: {len(cases)} cases")
    write_table(ROOT / "dart/lib/src/errors.dart", "  ", dart_row)
    write_table(ROOT / "rust/splitz-core/src/error.rs", "    ", rust_row)


def dart_row(code, text):
    """One map entry as `dart format` lays it out: split after the key when
    the whole entry is past 80 columns."""
    value = "'" + text.replace("\\", "\\\\").replace("'", "\\'") + "',"
    row = f"'{code}': {value}"
    return row if len("  " + row) <= 80 else f"'{code}':\n      {value}"


def rust_row(code, text):
    """One tuple as `rustfmt` lays it out: one element to a line when its
    elements are past the 60 columns of `fn_call_width`."""
    value = '"' + text.replace("\\", "\\\\").replace('"', '\\"') + '"'
    row = f'("{code}", {value}),'
    if len(f'"{code}", {value}') <= 60:
        return row
    return f'(\n        "{code}",\n        {value},\n    ),'


def write_table(path, indent, line):
    """Rewrites the generated table in `path`, between its two marker lines."""
    begin = f"{indent}{'//'} Generated by tools/corpus/messages.py.\n"
    end = f"{indent}{'//'} End of generated table.\n"
    text = path.read_text()
    start, stop = text.index(begin) + len(begin), text.index(end)
    rows = "".join(f"{indent}{line(c, t)}\n" for c, t in sorted(MESSAGES.items()))
    path.write_text(text[:start] + rows + text[stop:])
    print(f"{path.relative_to(ROOT)}: {len(MESSAGES)} messages")


if __name__ == "__main__":
    main()
