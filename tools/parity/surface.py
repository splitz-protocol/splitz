#!/usr/bin/env python3
"""Diffs the two implementations' public surfaces.

The corpus compares serialised values and the differential lane compares
answers, so an API one side has and the other does not — a getter a consumer
reads, a constant a caller branches on — is invisible to both. It never reaches
the wire, so nothing that watches the wire can see it.

A divergence is either idiom, in which case it belongs in `allow.txt` with a
reason, or it is a gap. Exit status is 1 when one is neither.

Usage: tools/parity/surface.py
"""
import re
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parents[2]

# Names one language spells differently for reasons that carry no meaning.
RENAMES = {
    "splitExpense": "split_expense",
    "allocateEvenly": "allocate_evenly",
    "fiatToZatoshi": "fiat_to_zatoshi",
    "zatoshiToFiat": "zatoshi_to_fiat",
    "renderAmount": "render_amount",
    "renderUri": "render_uri",
    "renderInvite": "render_invite",
    "parseInvite": "parse_invite",
    "parseSealedFrame": "parse_sealed_frame",
    "encodePayload": "encode_payload",
    "decodePayload": "decode_payload",
    "decodeBill": "decode_bill",
    "canonicalJson": "canonical_json",
    "canonicalInstant": "canonical_instant",
    "deriveBillId": "derive_bill_id",
    "checkEntry": "check_entry",
    "foldLog": "fold_log",
    "mergeLogs": "merge_logs",
    "orderEntries": "order_entries",
    "netBalances": "net_balances",
    "directDebts": "direct_debts",
    "settleBalances": "settle_balances",
    "settleBill": "settle_bill",
    "boundedLabel": "bounded_label",
    "compareUtf8": "compare_utf8",
    "sortedUtf8": "sorted_utf8",
    "uniqueSortedUtf8": "unique_sorted_utf8",
    "stripScanPadding": "strip_scan_padding",
    "sha256Hex": "sha256_hex",
    "residualIsZero": "residual_is_zero",
    "attributeCoverage": "attribute_coverage",
    "checkIdLists": "check_id_lists",
    "deriveEntryId": "derive_entry_id",
    "entryIdDomain": "ENTRY_ID_DOMAIN",
    "isRerouted": "is_rerouted",
    "signingMessage": "signing_message",
    "resolveIdentities": "resolve_identities",
    "renderObligation": "render_obligation",
    "entrySigningDomain": "ENTRY_SIGNING_DOMAIN",
    "checkCurrency": "check_currency",
    "isCurrency": "is_currency",
    "checkedAdd": "checked_add",
    "checkedSum": "checked_sum",
    "checkedMultiply": "checked_mul",
    "zatoshiPerZec": "ZATOSHI_PER_ZEC",
    "maxAmount": "MAX_AMOUNT",
    "minAmount": "MIN_AMOUNT",
    "maxZatoshi": "MAX_ZATOSHI",
    "maxMemoBytes": "MAX_MEMO_BYTES",
    "maxLabelBytes": "MAX_LABEL_BYTES",
    "maxPayments": "MAX_PAYMENTS",
    "maxFiatDigits": "MAX_FIAT_DIGITS",
    "maxConvertibleMinorUnits": "MAX_CONVERTIBLE_MINOR_UNITS",
    "defaultExactLimit": "DEFAULT_EXACT_LIMIT",
    "maxExactLimit": "MAX_EXACT_LIMIT",
    "billVersion": "BILL_VERSION",
    "inviteVersion": "INVITE_VERSION",
    "payloadVersion": "PAYLOAD_VERSION",
    "sealedVersion": "SEALED_VERSION",
    "payloadCap": "PAYLOAD_CAP",
    "maxInviteBillId": "MAX_INVITE_BILL_ID",
    "nonceBytes": "NONCE_BYTES",
    "tagBytes": "TAG_BYTES",
    "scanPadding": "SCAN_PADDING",
    "entryKinds": "ENTRY_KINDS",
    "billIdDomain": "BILL_ID_DOMAIN",
    "payloadForKind": "payload_for",
    "confirmationMethods": "confirmation_rule",
}


def dart_surface() -> set[str]:
    names: set[str] = set()
    for path in (ROOT / "dart" / "lib" / "src").glob("*.dart"):
        text = path.read_text(encoding="utf-8")
        # Top-level declarations only: anything indented belongs to a class.
        for match in re.finditer(
            r"^(?:abstract\s+final\s+|final\s+|sealed\s+)?"
            r"(?:class|enum|mixin|extension|typedef)\s+([A-Z]\w*)",
            text,
            re.M,
        ):
            names.add(match.group(1))
        for match in re.finditer(r"^const\s+(?:\w[\w<>,? ]*\s+)?(\w+)\s*=", text, re.M):
            names.add(match.group(1))
        for match in re.finditer(
            r"^(?!\s)(?:[A-Za-z_][\w<>,?\[\] ]*\s+)(\w+)\s*\(", text, re.M
        ):
            name = match.group(1)
            if not name.startswith("_") and name not in {"if", "for", "while", "switch"}:
                names.add(name)
    return {n for n in names if not n.startswith("_")}


def rust_surface() -> set[str]:
    names: set[str] = set()
    for path in (ROOT / "rust" / "src").glob("*.rs"):
        text = path.read_text(encoding="utf-8")
        # Strip test modules: they are not surface.
        text = re.split(r"^#\[cfg\(test\)\]", text, maxsplit=1, flags=re.M)[0]
        for match in re.finditer(
            r"^pub\s+(?:fn|struct|enum|trait|type)\s+(\w+)", text, re.M
        ):
            names.add(match.group(1))
        for match in re.finditer(r"^\s*pub\s+const\s+(\w+)", text, re.M):
            names.add(match.group(1))
    return names


def normalise(name: str) -> str:
    return RENAMES.get(name, name)


def main() -> int:
    dart = {normalise(n) for n in dart_surface()}
    rust = rust_surface()

    allow_path = ROOT / "tools" / "parity" / "allow.txt"
    allowed: dict[str, str] = {}
    if allow_path.exists():
        for line in allow_path.read_text(encoding="utf-8").splitlines():
            line = line.strip()
            if not line or line.startswith("#"):
                continue
            name, _, reason = line.partition("#")
            allowed[name.strip()] = reason.strip()

    only_dart = sorted(f"dart::{n}" for n in dart - rust)
    only_rust = sorted(f"rust::{n}" for n in rust - dart)
    divergences = only_dart + only_rust

    unexplained = [d for d in divergences if d not in allowed]
    explained = [d for d in divergences if d in allowed]
    stale = [name for name in allowed if name not in divergences]

    print(
        f"{len(dart & rust)} shared, {len(divergences)} divergences: "
        f"{len(explained)} recorded, {len(unexplained)} unexplained"
    )
    for name in stale:
        print(f"  allow.txt names something that no longer diverges: {name}")
    for name in unexplained:
        print(f"  UNEXPLAINED {name}")

    return 1 if unexplained or stale else 0


if __name__ == "__main__":
    sys.exit(main())
