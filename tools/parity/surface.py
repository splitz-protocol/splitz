#!/usr/bin/env python3
"""Diffs the two implementations' public surfaces.

Two surfaces, not one: the protocol (`package:splitz_core/splitz_core.dart` against
`splitz_core::`) and the wallet seam (`package:splitz_core/host.dart` against
`splitz_core::host::`). They are compared separately because they are separately
importable — a name that is on the crate root and not on `host` is not a gap,
and a consumer reaching for the seam gets only what the seam exports.

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
    "deltaFor": "delta_for",
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
    "amendEntry": "amend_entry",
    "sealedPlaintext": "sealed_plaintext",
    "sealedNonce": "sealed_nonce",
    "frameSealed": "frame_sealed",
    "channelFor": "channel_for",
    "billToJson": "bill_to_json",
    "participantToJson": "participant_to_json",
    "payoutToJson": "payout_to_json",
    "expenseToJson": "expense_to_json",
    "paymentToJson": "payment_to_json",
    "rateToJson": "rate_to_json",
    "laneFor": "lane_for",
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
    "paymentDigestDomain": "PAYMENT_DIGEST_DOMAIN",
    "paymentDigest": "payment_digest",
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
    "checkedSubtract": "checked_sub",
    "isZip321Address": "is_zip321_address",
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
    # --- the wallet seam ---
    "createBill": "create_bill",
    "joinBill": "join_bill",
    "addExpense": "add_expense",
    "recordPayment": "record_payment",
    "confirmPayment": "confirm_payment",
    "setRate": "set_rate",
    "voidEntry": "void_entry",
    "signEntry": "sign_entry",
    "base64UrlNoPad": "base64url_no_pad",
    "entryVersion": "ENTRY_VERSION",
    "creatorKeyBytes": "CREATOR_KEY_BYTES",
    "readScan": "read_scan",
    "inviteFor": "invite_for",
    "shareableBill": "shareable_bill",
    "acceptScan": "accept_scan",
    "obligationFor": "obligation_for",
    "paymentIdForSend": "payment_id_for_send",
    "recordSend": "record_send",
}


def dart_surface(source: Path, recurse: bool = False) -> set[str]:
    """Top-level declarations under `source`.

    Not recursive by default: the protocol and the seam are two directories,
    one inside the other, and are compared separately. A package whose barrel
    exports a subdirectory passes `recurse`, or the lane would say two
    surfaces match while never reading part of one.
    """
    names: set[str] = set()
    for path in sorted(source.rglob("*.dart") if recurse else source.glob("*.dart")):
        text = path.read_text(encoding="utf-8")
        # Top-level declarations only: anything indented belongs to a class.
        for match in re.finditer(
            r"^(?:abstract\s+|final\s+|sealed\s+|base\s+|interface\s+)*"
            r"(?:class|enum|mixin|extension|typedef)\s+([A-Z]\w*)",
            text,
            re.M,
        ):
            names.add(match.group(1))
        for match in re.finditer(r"^const\s+(?:\w[\w<>,? ]*\s+)?(\w+)\s*=", text, re.M):
            names.add(match.group(1))
        for match in re.finditer(
            r"^(?!\s)(?:[A-Za-z_][\w<>,?\[\]. ]*\s+)(\w+)\s*\(", text, re.M
        ):
            name = match.group(1)
            if not name.startswith("_") and name not in {"if", "for", "while", "switch"}:
                names.add(name)
    return {n for n in names if not n.startswith("_")}


def rust_surface(source: Path) -> set[str]:
    """What `splitz_core::` re-exports — the surface a consumer actually reaches.

    Not every `pub` item in every module. A module is `pub mod`, so a name can
    be `pub` and still cost a consumer a compile error and a search, because
    the crate root does not carry it. Dart's barrel re-exports everything, so
    comparing module-level `pub` against it says two surfaces match when one
    of them is reachable only by spelling out the module.
    """
    text = source.read_text(encoding="utf-8")
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


def _param_text(text: str, open_paren: int) -> str | None:
    """The parameter list starting at `open_paren`, brackets balanced.

    Scanned rather than matched. Dart writes named parameters inside braces —
    `foldLog(List<Object?> raw, {String? billId})` — and a regex that stops at
    the first `{` reads that declaration as no declaration at all, so every
    function with a named parameter fell out of the comparison and its arity
    was never checked against Rust's.
    """
    depth = 0
    for i in range(open_paren, len(text)):
        ch = text[i]
        if ch in "([{":
            depth += 1
        elif ch in ")]}":
            depth -= 1
            if depth == 0:
                return text[open_paren + 1 : i]
        elif ch == ";" and depth == 1:
            return None
    return None


def dart_signatures(source: Path, recurse: bool = False) -> dict[str, list[str]]:
    """Top-level function name -> its parameter declarations."""
    sigs: dict[str, list[str]] = {}
    for path in sorted(source.rglob("*.dart") if recurse else source.glob("*.dart")):
        text = path.read_text(encoding="utf-8")
        for match in re.finditer(
            r"^(?!\s)(?:[A-Za-z_][\w<>,?\[\]. ]*\s+)(\w+)\s*\(", text, re.M
        ):
            name = match.group(1)
            if name.startswith("_") or name in {"if", "for", "while", "switch"}:
                continue
            params = _param_text(text, match.end() - 1)
            if params is None:
                continue
            body = text[match.end() - 1 + len(params) + 2 :].lstrip()
            # A declaration, not a call: what follows the list is a body.
            if not (body.startswith("{") or body.startswith("=>")):
                continue
            cleaned = params.replace("{", "").replace("}", "")
            sigs[name] = _split_params(cleaned)
    return sigs


def rust_signatures(source: Path) -> dict[str, list[str]]:
    sigs: dict[str, list[str]] = {}
    for path in sorted(source.glob("*.rs")):
        text = path.read_text(encoding="utf-8")
        text = re.split(r"^#\[cfg\(test\)\]", text, maxsplit=1, flags=re.M)[0]
        for match in re.finditer(
            r"^pub\s+fn\s+(\w+)\s*(?:<[^>]*>)?\s*\(([^;{]*?)\)\s*(?:->|\{)",
            text,
            re.M | re.S,
        ):
            sigs[match.group(1)] = _split_params(match.group(2))
    return sigs


def _snake(name: str) -> str:
    """`minorUnitsPerZec` -> `minor_units_per_zec`."""
    return re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower()


def normalise(name: str, auto_snake: bool = False) -> str:
    """The name both languages are compared under.

    `RENAMES` is explicit for the protocol surface, where a spelling that
    differs for a reason is worth writing down. The host surface is large and
    mechanically cased, so `auto_snake` maps what is left — a function or a
    constant — rather than adding fifty lines that say only "Rust writes snake
    case". A type keeps its capitals in both languages and is left alone.
    """
    if name in RENAMES:
        return RENAMES[name]
    if auto_snake and not name[:1].isupper():
        return _snake(name)
    return name


def _allowed(path: Path) -> dict[str, str]:
    allowed: dict[str, str] = {}
    if not path.exists():
        return allowed
    for line in path.read_text(encoding="utf-8").splitlines():
        line = line.strip()
        if not line or line.startswith("#"):
            continue
        name, _, reason = line.partition("#")
        allowed[name.strip()] = reason.strip()
    return allowed


def compare(
    label: str,
    dart_src: Path,
    rust_names: Path,
    rust_sigs: Path,
    allow_path: Path,
    auto_snake: bool = False,
    recurse: bool = False,
) -> int:
    """One surface pair. Returns the number of divergences neither side owns."""
    dart = {normalise(n, auto_snake) for n in dart_surface(dart_src, recurse)}
    rust = rust_surface(rust_names)
    allowed = _allowed(allow_path)

    only_dart = sorted(f"dart::{n}" for n in dart - rust)
    only_rust = sorted(f"rust::{n}" for n in rust - dart)

    # Shared names whose signatures disagree. An arity difference is a
    # consumer's compile error; an `Object?` on one side against a concrete
    # type on the other is worse, because the typed side cannot be handed the
    # value that the untyped side has to decide about.
    d_sigs = {normalise(n, auto_snake): ps
              for n, ps in dart_signatures(dart_src, recurse).items()}
    r_sigs = rust_signatures(rust_sigs)
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
        f"{label}: {len(dart & rust)} shared names, "
        f"{len(set(d_sigs) & set(r_sigs))} shared signatures, "
        f"{len(divergences)} divergences: "
        f"{len(explained)} recorded, {len(unexplained)} unexplained"
    )
    for name in stale:
        print(f"  allow.txt names something that no longer diverges: {name}")
    for name in unexplained:
        print(f"  UNEXPLAINED {name}")

    return len(unexplained) + len(stale)


def main() -> int:
    here = ROOT / "tools" / "parity"
    open_items = compare(
        "protocol",
        ROOT / "dart" / "lib" / "src",
        ROOT / "rust" / "splitz-core" / "src" / "lib.rs",
        ROOT / "rust" / "splitz-core" / "src",
        here / "allow.txt",
    )
    # The seam is its own import on both sides, so it is its own comparison.
    # A protocol name missing from it is correct, not a gap.
    open_items += compare(
        "host",
        ROOT / "dart" / "lib" / "src" / "host",
        ROOT / "rust" / "splitz-core" / "src" / "host" / "mod.rs",
        ROOT / "rust" / "splitz-core" / "src" / "host",
        here / "allow-host.txt",
    )
    # The plumbing a wallet needs around the protocol: two packages rather
    # than two modules of one, so their surfaces are compared on their own.
    open_items += compare(
        "plumbing",
        ROOT / "splitz_host" / "lib" / "src",
        ROOT / "rust" / "splitz-host" / "src" / "lib.rs",
        ROOT / "rust" / "splitz-host" / "src",
        here / "allow-plumbing.txt",
        auto_snake=True,
        recurse=True,
    )
    return 1 if open_items else 0


if __name__ == "__main__":
    sys.exit(main())
