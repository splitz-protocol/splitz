#!/usr/bin/env python3
"""Diffs the two implementations' public surfaces.

The corpus compares serialised values and the differential lane compares
answers, so an API one side has and the other does not — a getter a consumer
reads, a constant a caller branches on — is invisible to both. It never reaches
the wire, so nothing that watches the wire can see it.

Names are not the whole surface. Two functions can share a name and disagree
about what they accept, and the wider signature then reaches a value the
narrower one cannot be called with at all — so one implementation runs a check
the other never gets to. This lane therefore compares arity, and reports a
parameter Dart types as `Object?` or `dynamic` whose Rust counterpart is typed
concretely.

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
    "splitParticipants": "split_participants",
    "withholdings": "withholdings",
    "billPrefix": "BILL_PREFIX",
    "deltaPrefix": "DELTA_PREFIX",
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
    "decodeRate": "decode_rate",
    "billSplitModes": "SPLIT_MODES",
    "checkCurrency": "check_currency",
    "isCurrency": "is_currency",
    "decodeParticipant": "decode_participant",
    "decodeExpense": "decode_expense",
    "decodePayment": "decode_payment",
    "maxDocumentDepth": "MAX_DOCUMENT_DEPTH",
    "withinDepth": "within_depth",
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
    """What `splitz::` re-exports — the surface a consumer actually reaches.

    Not every `pub` item in every module. A module is `pub mod`, so a name can
    be `pub` and still cost a consumer a compile error and a search, because
    the crate root does not carry it. Dart's barrel re-exports everything, so
    comparing module-level `pub` against it says two surfaces match when one
    of them is reachable only by spelling out the module.
    """
    text = (ROOT / "rust" / "src" / "lib.rs").read_text(encoding="utf-8")
    names: set[str] = set()
    for match in re.finditer(r"^pub use [\w:]+\{([^}]*)\};", text, re.M | re.S):
        for item in match.group(1).split(","):
            item = item.strip()
            if item:
                names.add(item.split(" as ")[-1])
    for match in re.finditer(r"^pub use [\w:]*::(\w+);", text, re.M):
        names.add(match.group(1))
    return names


def _split_params(text: str) -> list[str]:
    """Top-level comma-separated parameters, ignoring nested brackets."""
    out, depth, cur = [], 0, ""
    for ch in text:
        if ch in "<([{":
            depth += 1
        elif ch in ">)]}":
            depth -= 1
        if ch == "," and depth == 0:
            out.append(cur.strip())
            cur = ""
        else:
            cur += ch
    if cur.strip():
        out.append(cur.strip())
    return out


def dart_signatures() -> dict[str, list[str]]:
    """Top-level function name -> its parameter declarations."""
    sigs: dict[str, list[str]] = {}
    for path in (ROOT / "dart" / "lib" / "src").glob("*.dart"):
        text = path.read_text(encoding="utf-8")
        for match in re.finditer(
            r"^(?!\s)(?:[A-Za-z_][\w<>,?\[\] ]*\s+)(\w+)\s*\(([^;{]*?)\)\s*(?:\{|=>)",
            text,
            re.M | re.S,
        ):
            name, params = match.group(1), match.group(2)
            if name.startswith("_"):
                continue
            sigs[name] = [p for p in _split_params(params.replace("{", "").replace("}", ""))]
    return sigs


def rust_signatures() -> dict[str, list[str]]:
    sigs: dict[str, list[str]] = {}
    for path in (ROOT / "rust" / "src").glob("*.rs"):
        text = path.read_text(encoding="utf-8")
        text = re.split(r"^#\[cfg\(test\)\]", text, maxsplit=1, flags=re.M)[0]
        for match in re.finditer(
            r"^pub\s+fn\s+(\w+)\s*(?:<[^>]*>)?\s*\(([^;{]*?)\)\s*(?:->|\{)",
            text,
            re.M | re.S,
        ):
            sigs[match.group(1)] = _split_params(match.group(2))
    return sigs


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

    # Shared names whose signatures disagree. An arity difference is a
    # consumer's compile error; an `Object?` on one side against a concrete
    # type on the other is worse, because the typed side cannot be handed the
    # value that the untyped side has to decide about.
    d_sigs = {normalise(n): ps for n, ps in dart_signatures().items()}
    r_sigs = rust_signatures()
    shape: list[str] = []
    for name in sorted(set(d_sigs) & set(r_sigs)):
        dp, rp = d_sigs[name], r_sigs[name]
        if len(dp) != len(rp):
            shape.append(f"shape::{name} takes {len(dp)} in dart, {len(rp)} in rust")
            continue
        for i, (d, r) in enumerate(zip(dp, rp)):
            loose = re.match(r"^(Object\?|dynamic)(\s|$)", d.strip())
            if loose and "dyn " not in r and "Value" not in r:
                shape.append(
                    f"shape::{name} parameter {i} is {d.strip()} in dart "
                    f"and {r.strip()} in rust"
                )

    divergences = only_dart + only_rust + shape

    unexplained = [d for d in divergences if d not in allowed]
    explained = [d for d in divergences if d in allowed]
    stale = [name for name in allowed if name not in divergences]

    print(
        f"{len(dart & rust)} shared names, "
        f"{len(set(d_sigs) & set(r_sigs))} shared signatures, "
        f"{len(divergences)} divergences: "
        f"{len(explained)} recorded, {len(unexplained)} unexplained"
    )
    for name in stale:
        print(f"  allow.txt names something that no longer diverges: {name}")
    for name in unexplained:
        print(f"  UNEXPLAINED {name}")

    return 1 if unexplained or stale else 0


if __name__ == "__main__":
    sys.exit(main())
