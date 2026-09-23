"""A reference for the computational sections of SPEC.md, written from the
specification text.

It exists to generate vectors. It is not one of the shipped implementations,
and no shipped implementation may be used to produce an expectation: a corpus
derived from an implementation cannot contain a case that implementation is
self-consistently wrong about.
"""

I64_MAX = 2**63 - 1
I64_MIN = -(2**63)
ZAT_PER_ZEC = 100_000_000


class Refused(Exception):
    def __init__(self, code):
        super().__init__(code)
        self.code = code


def _ascii_alnum(text):
    """`1*( ALPHA / DIGIT )` with RFC 3986's ALPHA and DIGIT, which are ASCII.

    `str.isalnum` admits every Unicode letter and digit, so U+00E9 and U+FF12
    pass it and are not in the grammar.
    """
    return bool(text) and all(
        "0" <= c <= "9" or "A" <= c <= "Z" or "a" <= c <= "z" for c in text
    )


def _ascii_digits(text):
    """`1*DIGIT`, ASCII only.

    `str.isdigit` admits U+00B2 and U+0663, and `int` then raises on the first
    and silently reads the second as 3.
    """
    return bool(text) and all("0" <= c <= "9" for c in text)


# i64::MAX is 9223372036854775807: 19 digits.
I64_MAX_DIGITS = 19


def bare_decimal(text):
    """A bare decimal integer within a signed 64-bit range, or None.

    The length is checked before the conversion, not after: CPython refuses to
    convert a numeral past 4300 digits at all, so bounding by value alone
    raises on exactly the input the bound exists to refuse. No sign, no
    padding, no whitespace.
    """
    if not _ascii_digits(text) or len(text) > I64_MAX_DIGITS:
        return None
    value = int(text)
    if value > I64_MAX or text != str(value):
        return None
    return value


def _as_dict(value):
    """`value` when it is a dict, otherwise an empty one.

    For the passes that read inside a payload before it has been decoded:
    section 10.1 types the payload itself, not its members, so anything under
    it is whatever a peer wrote and indexing there raises on one entry.
    """
    return value if isinstance(value, dict) else {}


def _as_list(value):
    """`value` when it is a list, otherwise an empty one. See `_as_dict`."""
    return value if isinstance(value, list) else []


def is_currency(code):
    """Section 2.1: exactly three upper-case letters."""
    return (isinstance(code, str) and len(code) == 3
            and all("A" <= c <= "Z" for c in code))


def check_currency(code):
    """Section 2.1: exactly three upper-case letters."""
    if not is_currency(code):
        raise Refused("bill_bad_currency")


def allocate(total, weights):
    """Section 3, steps 1 to 7."""
    if not weights:
        raise Refused("empty_weights")
    if any(w < 0 for w in weights):
        raise Refused("negative_weight")
    W = 0
    for w in weights:
        W += w
        if W > I64_MAX:
            raise Refused("weight_sum_overflow")
    if W == 0:
        raise Refused("zero_weight_sum")
    if total == I64_MIN:
        raise Refused("allocation_overflow")
    s = -1 if total < 0 else 1
    m = abs(total)
    for w in weights:
        if m * w > I64_MAX:
            raise Refused("allocation_overflow")
    parts = [(m * w) // W for w in weights]
    rems = [(m * w) % W for w in weights]
    leftover = m - sum(parts)
    if not (0 <= leftover < len(weights)):
        raise Refused("allocation_overflow")
    for i in sorted(range(len(weights)), key=lambda i: (-rems[i], i))[:leftover]:
        parts[i] += 1
    return [p * s for p in parts] if s < 0 else parts


def allocate_evenly(total, count):
    return allocate(total, [1] * count)


def _sorted_ids(ids):
    """Section 2.3: ascending UTF-8 byte order."""
    return sorted(set(ids), key=lambda s: s.encode("utf-8"))


def _checked_sum(values, code="amount_overflow"):
    t = 0
    for v in values:
        t += v
        if t > I64_MAX or t < I64_MIN:
            raise Refused(code)
    return t


def _against(value, total):
    """True when `value` carries the sign opposite to `total` (section 4)."""
    return value > 0 if total < 0 else value < 0


def split(total, spec):
    """Section 4. Returns {id: owed minor units}."""
    _check_id_lists(spec)
    kind = spec.get("type")

    if kind == "equal":
        among = _sorted_ids(spec.get("among") or [])
        if not among:
            raise Refused("empty_split")
        return dict(zip(among, allocate_evenly(total, len(among))))

    if kind == "exact":
        amounts = spec.get("amounts") or {}
        ids = _sorted_ids(amounts)
        if any(_against(amounts[i], total) for i in ids):
            raise Refused("negative_share")
        if _checked_sum([amounts[i] for i in ids]) != total:
            raise Refused("exact_total_mismatch")
        return {i: amounts[i] for i in ids}

    if kind == "percentage":
        bp = spec.get("basisPoints") or {}
        ids = _sorted_ids(bp)
        if any(bp[i] < 0 for i in ids):
            raise Refused("negative_weight")
        # The overflow check precedes the full-scale check (section 4.3).
        if _checked_sum([bp[i] for i in ids]) != 10000:
            raise Refused("percentage_not_full_scale")
        return dict(zip(ids, allocate(total, [bp[i] for i in ids])))

    if kind == "shares":
        counts = spec.get("shareCounts") or {}
        ids = _sorted_ids(counts)
        return dict(zip(ids, allocate(total, [counts[i] for i in ids])))

    if kind == "itemized":
        items = spec.get("items") or []
        extra = spec.get("extraMinorUnits", 0)
        if not items:
            raise Refused("itemized_no_items")
        # An item is an object (section 4.5). Typed before it is indexed:
        # every check below reads a member of it.
        if any(not isinstance(it, dict) for it in items):
            raise Refused("bill_type_error")
        if any(not (it.get("sharedBy") or []) for it in items):
            raise Refused("itemized_unassigned_item")
        if _against(extra, total) or any(
                _against(it["minorUnits"], total) for it in items):
            raise Refused("negative_share")
        if _checked_sum([it["minorUnits"] for it in items] + [extra]) != total:
            raise Refused("itemized_total_mismatch")
        subtotal = {}
        for it in items:
            who = _sorted_ids(it["sharedBy"])
            for pid, part in zip(who, allocate_evenly(it["minorUnits"], len(who))):
                subtotal[pid] = subtotal.get(pid, 0) + part
        ids = _sorted_ids(subtotal)
        # Magnitudes: section 3.1 refuses a negative weight, and a refund
        # subtotals negative. The proportion is the same either way.
        weights = [abs(subtotal[i]) for i in ids]
        if extra:
            extras = allocate(extra, weights) if any(weights) else allocate_evenly(extra, len(ids))
            for pid, e in zip(ids, extras):
                subtotal[pid] += e
        return {i: subtotal[i] for i in ids}

    raise Refused("bill_unknown_split_type")


def fiat_to_zatoshi(minor_units, rate, amount_currency=None, rounding="up"):
    """Section 7.1."""
    check_currency(rate["currency"])
    if amount_currency is not None:
        check_currency(amount_currency)
        if amount_currency != rate["currency"]:
            raise Refused("rate_currency_mismatch")
    per = rate["minorUnitsPerZec"]
    if per <= 0:
        raise Refused("rate_not_positive")
    if minor_units < 0:
        raise Refused("negative_amount")
    if minor_units > I64_MAX // ZAT_PER_ZEC:
        raise Refused("rate_amount_too_large")
    numerator = minor_units * ZAT_PER_ZEC
    q, r = divmod(numerator, per)
    if r == 0:
        return q
    if rounding == "up":
        return q + 1
    if rounding == "down":
        return q
    if rounding == "nearest":
        # Written as a comparison rather than r * 2, which wraps at 2^62
        # and then rounds the wrong way. Both sides are bounded by `per`.
        return q + 1 if r >= per - r else q
    raise Refused("bill_unknown_settlement_method")


def zatoshi_to_fiat(zatoshi, rate):
    """Section 7.2. Display only."""
    check_currency(rate["currency"])
    per = rate["minorUnitsPerZec"]
    if per <= 0:
        raise Refused("rate_not_positive")
    if zatoshi < 0:
        raise Refused("negative_amount")
    if zatoshi * per > I64_MAX:
        raise Refused("amount_overflow")
    q, r = divmod(zatoshi * per, ZAT_PER_ZEC)
    return q + 1 if r * 2 >= ZAT_PER_ZEC else q


# Mainnet Unified Addresses from the librustzcash test corpus, each confirmed
# to decode with `ZcashAddress::try_from_encoded`. Real ones are here so that a
# wallet running this corpus puts every address through its own decoder rather
# than skipping that check: filler like "u1abc" satisfies §8.3's syntactic test
# and nothing else.
ADDRESSES = [
    "u13j3q8q8f9hx2nx0w9l52dqksy4png7fgm0lqjh8ahn9enyvz5z9xnwzdcdjmpf756s2y88rnyr9px4f4k9w03sl6fr4vwsqcvg8ggfjx",
    "u16cynw2u6nshm44gjv9vy9dvav6zvvksphexzjs3tjke8mr3p942er0pu8held7zy7wpjxzqgkpdrjzd72h7pwf34df8a0xcv0su3acx7",
    "u187vrwl4ampyxd5m6aj38n4ndkmj8v6gs97hkt23aps3sn5k89a0gk2smluexgdprcrtm56ezc5c7tjwlrnnl79tjtrxmqd42c5mpyz7g",
    "u1ddnjsdcpm36r6aq79n3s68shjweksnmwtdltrh046s8m6xcws9ygyawalxx8n6hg6vegk0wh8zjnafxgh6msppjsljvyt0ynece3lvm0",
    "u1dqavtnjvu42hlsjw6sc2mxajqlyt03zg8l4luykz9fnchunq74nqxhfp58h5n5xfpyqhheax8thta8lfkjgp8wqwsavc0g4mgu4du02c",
    "u1qpatys4zruk99pg59gcscrt7y6akvl9vrhcfyhm9yxvxz7h87q6n8cgrzzpe9zru68uq39uhmlpp5uefxu0su5uqyqfe5zp3tycn0ecl",
]


# --- Section 8: payment request URIs ---------------------------------------

MAX_ZATOSHI = 21_000_000 * ZAT_PER_ZEC
MAX_MEMO_BYTES = 512
MAX_LABEL_BYTES = 96
MAX_FIAT_DIGITS = 18
MAX_PAYMENTS = 10_000          # indices run to 9999 (section 8.2)

QCHAR_EXTRA = set("!$'()*+,;:@")
UNRESERVED = set(
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-._~"
)


def render_amount(zatoshi):
    """Section 8.1. Decimal ZEC, trailing zeros removed, one representation."""
    if zatoshi <= 0:
        raise Refused("zip321_amount_not_positive")
    if zatoshi > MAX_ZATOSHI:
        raise Refused("zip321_amount_too_large")
    coins, zats = divmod(zatoshi, ZAT_PER_ZEC)
    if zats == 0:
        return str(coins)
    return f"{coins}.{str(zats).zfill(8).rstrip('0')}"


def qchar(text):
    """Section 8.3. Percent-escape every byte outside the literal set."""
    out = []
    for byte in text.encode("utf-8"):
        ch = chr(byte)
        if ch in UNRESERVED or ch in QCHAR_EXTRA:
            out.append(ch)
        else:
            out.append(f"%{byte:02X}")
    return "".join(out)


def bounded_label(name):
    """Section 8.3. At most 96 UTF-8 bytes, cut on a character boundary."""
    raw = name.encode("utf-8")
    if len(raw) <= MAX_LABEL_BYTES:
        return name
    cut = raw[:MAX_LABEL_BYTES]
    while cut:
        try:
            return cut.decode("utf-8")
        except UnicodeDecodeError:
            cut = cut[:-1]
    return ""


def b64url(raw):
    import base64
    return base64.urlsafe_b64encode(raw).decode("ascii").rstrip("=")


def render_fiat(currency, minor_units):
    """Section 8.4. The value of this payment, not the price of one ZEC."""
    if not isinstance(currency, str) or len(currency) != 3 \
            or not all("A" <= c <= "Z" for c in currency):
        raise Refused("zip321_bad_currency_code")
    if minor_units <= 0:
        raise Refused("zip321_fiat_not_positive")
    if len(str(minor_units)) > MAX_FIAT_DIGITS:
        raise Refused("zip321_fiat_too_many_digits")
    return f"{currency}:{minor_units}"


def render_uri(payments, include_fiat=False):
    """Section 8.2 and 8.5. One canonical rendering."""
    if not payments:
        raise Refused("zip321_no_payments")
    if len(payments) > MAX_PAYMENTS:
        raise Refused("zip321_too_many_payments")

    for p in payments:
        addr = p.get("address")
        # The address is checked before any other parameter of this payment.
        if not addr:
            raise Refused("zip321_no_address")
        if not _ascii_alnum(addr):
            raise Refused("zip321_bad_address")

    single = len(payments) == 1
    parts = []
    for i, p in enumerate(payments):
        sfx = "" if i == 0 else f".{i}"
        if not (single and i == 0):
            parts.append(f"address{sfx}={p['address']}")
        parts.append(f"amount{sfx}={render_amount(p['zatoshi'])}")
        if include_fiat and p.get("fiat"):
            parts.append(f"fiat{sfx}={render_fiat(*p['fiat'])}")
        if p.get("memo") is not None:
            raw = p["memo"] if isinstance(p["memo"], bytes) else p["memo"].encode("utf-8")
            if len(raw) > MAX_MEMO_BYTES:
                raise Refused("zip321_memo_too_large")
            parts.append(f"memo{sfx}={b64url(raw)}")
        if p.get("label") is not None:
            parts.append(f"label{sfx}={qchar(bounded_label(p['label']))}")
        if p.get("message") is not None:
            parts.append(f"message{sfx}={qchar(p['message'])}")

    head = f"zcash:{payments[0]['address']}?" if single else "zcash:?"
    return head + "&".join(parts)


# --- Section 11.1: the invite URI ------------------------------------------

INVITE_PREFIX = "splitz://join"
INVITE_VERSION = 1
MAX_BILL_ID = 128
SCAN_PADDING = "\t\n\r ﻿"      # exactly these, section 11.1
B64URL_ALPHABET = set(
    "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789-_"
)


def strip_scan_padding(text):
    """Section 11.1. Exactly five code points, at the ends only.

    Not a general trim: Dart strips every Unicode White_Space and U+FEFF,
    Rust strips the whitespace and leaves U+FEFF, so one code with a leading
    byte order mark is an invite to one reader and not the other.
    """
    return text.strip(SCAN_PADDING)


def _percent_decode(value):
    """Section 11.1. `+` is a literal plus; a malformed escape stays literal."""
    out = bytearray()
    i = 0
    raw = value.encode("utf-8")
    while i < len(raw):
        if raw[i] == 0x25 and i + 2 < len(raw):          # '%'
            hexpair = raw[i + 1:i + 3].decode("ascii", "replace")
            try:
                out.append(int(hexpair, 16))
                i += 3
                continue
            except ValueError:
                pass
        out.append(raw[i])
        i += 1
    return out.decode("utf-8", "replace")


def parse_invite(text):
    """Section 11.1. An exact grammar, never a general URI library."""
    s = strip_scan_padding(text)

    if not s.startswith(INVITE_PREFIX):
        raise Refused("invite_not_an_invite")
    rest = s[len(INVITE_PREFIX):]
    if rest and not rest.startswith("?"):
        # a path, a port, userinfo or a fragment is not an invite
        raise Refused("invite_not_an_invite")
    query = rest[1:] if rest.startswith("?") else ""

    # The first occurrence of a parameter wins.
    fields = {}
    for pair in query.split("&") if query else []:
        key, _, value = pair.partition("=")
        if key and key not in fields:
            fields[key] = _percent_decode(value)

    if "v" not in fields:
        raise Refused("invite_missing_version")
    raw_v = fields["v"]
    # Bounded before it is converted (section 11.1).
    version = bare_decimal(raw_v)
    if version is None or version < 1:
        raise Refused("invite_missing_version")
    if version > INVITE_VERSION:
        raise Refused("invite_future_version")

    bill_id = fields.get("b", "")
    if not bill_id:
        raise Refused("invite_missing_bill_id")
    if len(bill_id) > MAX_BILL_ID or any(c not in B64URL_ALPHABET for c in bill_id):
        raise Refused("invite_bad_bill_id")

    key = fields.get("k", "")
    if not key:
        raise Refused("invite_missing_key")
    if any(c not in B64URL_ALPHABET for c in key):
        raise Refused("invite_missing_key")

    out = {"version": version, "billId": bill_id, "key": key,
           "name": fields.get("n", "")}

    if "x" in fields:
        x = fields["x"]
        expiry = bare_decimal(x)
        if expiry is None:
            raise Refused("invite_bad_expiry")
        out["expiry"] = expiry
    return out


def render_invite(bill_id, key, name="", expiry=None):
    """Section 11.1. An encoder refuses what this parser refuses.

    A bound enforced only on decode lets a caller build a URI that no reader
    accepts, and the caller learns of it from somebody else's scanner.
    """
    if expiry is not None and (not isinstance(expiry, int)
                               or isinstance(expiry, bool)
                               or expiry < 0 or expiry > I64_MAX):
        raise Refused("invite_bad_expiry")
    if not bill_id or len(bill_id) > MAX_BILL_ID \
            or any(c not in B64URL_ALPHABET for c in bill_id):
        raise Refused("invite_bad_bill_id")
    if not key or any(c not in B64URL_ALPHABET for c in key):
        raise Refused("invite_missing_key")
    parts = [f"v={INVITE_VERSION}", f"b={qchar(bill_id)}", f"k={qchar(key)}"]
    if name:
        parts.append(f"n={qchar(name)}")
    if expiry is not None:
        parts.append(f"x={expiry}")
    return f"{INVITE_PREFIX}?" + "&".join(parts)


# --- Section 9.3: canonical JSON -------------------------------------------

def canonical_json(value):
    """Ascending key order by UTF-8 bytes, no insignificant whitespace.

    A float is refused rather than truncated: section 2 puts every amount in
    minor units as an integer, so a document carrying one was not written by a
    conforming writer.
    """
    import json as _json

    def check(v):
        if isinstance(v, float):
            raise Refused("canonical_json_float")
        if isinstance(v, dict):
            for k in v:
                check(v[k])
        elif isinstance(v, list):
            for item in v:
                check(item)

    def order(v):
        if isinstance(v, dict):
            return {k: order(v[k]) for k in sorted(v, key=lambda s: s.encode("utf-8"))}
        if isinstance(v, list):
            return [order(i) for i in v]
        return v

    check(value)
    return _json.dumps(order(value), separators=(",", ":"), ensure_ascii=False)


# --- Section 11.2: scanned payloads ----------------------------------------

PAYLOAD_CAP = 2331          # a version-40 QR code, byte mode, EC level M

# Sections 10.1 and 11.2. Stated here rather than inherited from a JSON
# library: one
# reader's parser gives up at its own depth and another does not, and the cap
# is no defence because a level of nesting costs two bytes. The deepest a
# conforming document reaches is the sharedBy array inside an itemised split,
# at eight.
MAX_DOCUMENT_DEPTH = 64


def _depth(value, limit):
    """True when `value` nests no deeper than `limit`."""
    stack = [(value, 1)]
    while stack:
        node, d = stack.pop()
        if d > limit:
            return False
        if isinstance(node, dict):
            stack.extend((v, d + 1) for v in node.values())
        elif isinstance(node, list):
            stack.extend((v, d + 1) for v in node)
    return True
PAYLOAD_VERSION = 1
BILL_PREFIX = "splitz1:"
DELTA_PREFIX = "splitzd1:"
PAYLOAD_PREFIXES = (BILL_PREFIX, DELTA_PREFIX)


def encode_payload(prefix, body):
    """Section 11.2. Canonical JSON, base64url, capped in both directions."""
    if prefix not in PAYLOAD_PREFIXES:
        raise Refused("payload_not_a_payload")
    encoded = b64url(canonical_json(body).encode("utf-8"))
    if len(encoded) > PAYLOAD_CAP:
        raise Refused("payload_too_large")
    return prefix + encoded


def decode_payload(text):
    import base64
    s = strip_scan_padding(text)
    for prefix in PAYLOAD_PREFIXES:
        if s.startswith(prefix):
            break
    else:
        raise Refused("payload_not_a_payload")

    encoded = s[len(prefix):]
    if len(encoded) > PAYLOAD_CAP:
        raise Refused("payload_too_large")
    if any(c not in B64URL_ALPHABET for c in encoded):
        raise Refused("payload_damaged")
    try:
        raw = base64.urlsafe_b64decode(encoded + "=" * (-len(encoded) % 4))
        # Section 9.4: a body that is not its bytes' canonical encoding is
        # refused as one that does not decode.
        if b64url(raw) != encoded:
            raise ValueError("not canonical")
        import json as _json
        body = _json.loads(raw.decode("utf-8"))
    except Exception:
        raise Refused("payload_damaged")
    if not isinstance(body, dict):
        raise Refused("payload_damaged")
    if not _depth(body, MAX_DOCUMENT_DEPTH):
        raise Refused("payload_damaged")

    version = body.get("v")
    # Section 11.2 bounds it as section 11.1 bounds the invite's: a JSON
    # number outside a signed 64-bit integer is not a version this format
    # numbers, and three readers' integer types must not each decide.
    if not isinstance(version, int) or isinstance(version, bool) \
            or version < 1 or version > I64_MAX:
        raise Refused("payload_damaged")
    if version > PAYLOAD_VERSION:
        raise Refused("payload_future_version")
    if "log" not in body or not isinstance(body["log"], list):
        raise Refused("payload_missing_body")
    # Section 11.2. Only the bill prefix carries an invite, and only an object
    # is one: a delta's reader already holds a key, and a second one arriving
    # from a peer names a bill and a key that reader never chose.
    invite = body.get("invite")
    if prefix != BILL_PREFIX or not isinstance(invite, dict):
        invite = None
    return {"prefix": prefix, "version": version, "log": body["log"],
            "invite": invite}


# --- Section 11.3: sealing --------------------------------------------------

SEALED_VERSION = 1
NONCE_BYTES = 24
TAG_BYTES = 16


def parse_sealed_frame(text):
    """Section 11.3. Version byte, 24-byte nonce, ciphertext and tag.

    Opening it is the host's; what this checks is the frame two wallets must
    agree on before the cipher runs.
    """
    import base64
    s = strip_scan_padding(text)
    if any(c not in B64URL_ALPHABET for c in s) or not s:
        raise Refused("sealed_malformed")
    try:
        raw = base64.urlsafe_b64decode(s + "=" * (-len(s) % 4))
    except Exception:
        raise Refused("sealed_malformed")
    # Section 9.4: canonical, so re-encoding reproduces it exactly.
    if b64url(raw) != s:
        raise Refused("sealed_malformed")
    if len(raw) < 1 + NONCE_BYTES + TAG_BYTES:
        raise Refused("sealed_malformed")
    version = raw[0]
    # A version of zero is a malformed frame, not a future format: telling
    # somebody their app is too old sends them to an update that will not help.
    if version < 1:
        raise Refused("sealed_malformed")
    if version > SEALED_VERSION:
        raise Refused("sealed_future_version")
    return {"version": version,
            "nonce": b64url(raw[1:1 + NONCE_BYTES]),
            "bodyBytes": len(raw) - 1 - NONCE_BYTES}


def seal_nonce(plaintext):
    """Section 11.3. SHA-256 of the canonical bytes, truncated to 24."""
    import hashlib
    return hashlib.sha256(plaintext.encode("utf-8")).digest()[:NONCE_BYTES]


def channel_for(bill_id):
    """Section 11.3. The bill id's SHA-256, lower-case hex."""
    import hashlib
    return hashlib.sha256(bill_id.encode("utf-8")).hexdigest()


# --- Section 9: decoding a bill ---------------------------------------------

BILL_VERSION = 1
SPLIT_MODES = ("equal", "percentage")
PAYOUT_TYPES = ("zec", "swap", "cash")
SETTLEMENT_METHODS = ("shieldedZec", "swap", "cash")
CONFIRMATION_METHODS = {
    "recipientConfirmed": ("to", False, True),
    "walletReceived":     ("to", False, True),
    "onChain":            ("to", True, True),
    "payerAttested":      ("from", False, False),
}

_INSTANT = __import__("re").compile(
    r"^([0-9]{4})-([0-9]{2})-([0-9]{2})[Tt]([0-9]{2}):([0-9]{2}):([0-9]{2})(\.[0-9]+)?[Zz]$"
)


def _days_in_month(y, m):
    if m == 2:
        leap = (y % 4 == 0 and y % 100 != 0) or y % 400 == 0
        return 29 if leap else 28
    return 31 if m in (1, 3, 5, 7, 8, 10, 12) else 30


def parse_instant(text):
    """Section 9.3. Exactly this grammar, no numeric offset, no leap second."""
    if not isinstance(text, str):
        raise Refused("bill_type_error")
    m = _INSTANT.match(text)
    if not m:
        raise Refused("bill_type_error")
    y, mo, d, h, mi, s = (int(m.group(i)) for i in range(1, 7))
    if not (1 <= y <= 9999 and 1 <= mo <= 12):
        raise Refused("bill_type_error")
    if not (1 <= d <= _days_in_month(y, mo)):
        raise Refused("bill_type_error")
    if h > 23 or mi > 59 or s > 59:          # a leap second is refused
        raise Refused("bill_type_error")
    frac = (m.group(7) or ".000")[1:]
    frac = (frac + "000")[:3]                # truncated, never rounded
    return f"{y:04d}-{mo:02d}-{d:02d}T{h:02d}:{mi:02d}:{s:02d}.{frac}Z"


def _int(value, code="bill_type_error"):
    if isinstance(value, bool) or not isinstance(value, int):
        raise Refused(code)
    return value


def _str(value):
    if not isinstance(value, str):
        raise Refused("bill_type_error")
    return value


def _check_scalar_values(value):
    """Section 2.3. Every string is a sequence of Unicode scalar values.

    A lone surrogate has no UTF-8 encoding, so two strings that are not equal
    compare equal once encoded and ascending id stops being a total order.
    """
    if isinstance(value, str):
        try:
            value.encode("utf-8")
        except UnicodeEncodeError:
            raise Refused("bill_not_scalar_values") from None
    elif isinstance(value, dict):
        for k, v in value.items():
            _check_scalar_values(k)
            _check_scalar_values(v)
    elif isinstance(value, list):
        for v in value:
            _check_scalar_values(v)


def decode_rate(raw):
    """Section 7. One exchange rate, decoded by the rules section 9 states.

    Shared with the fold, which applies it to each setRate before the rate
    reaches a bill document: a member the decoder would refuse sets its entry
    aside (section 10.3) rather than making the whole document undecodable.
    """
    if not isinstance(raw, dict):
        raise Refused("bill_type_error")
    check_currency(raw.get("currency"))
    per = _int(raw.get("minorUnitsPerZec"))
    if per <= 0:
        raise Refused("rate_not_positive")
    out = {"currency": raw["currency"], "minorUnitsPerZec": per,
           "at": parse_instant(raw.get("at"))}
    if "source" in raw:
        out["source"] = _str(raw["source"])
    return out


def decode_participant(raw):
    """Section 9.1, one participant.

    Shared with the fold, which applies it to each joinBill before the
    participant reaches a bill document: a member the decoder would refuse
    sets its entry aside (section 10.3) rather than making the whole document
    undecodable.
    """
    if not isinstance(raw, dict):
        raise Refused("bill_type_error")
    pid = _str(raw.get("id"))
    # Section 9.1. An empty id is not a name anyone can be settled to.
    if not pid:
        raise Refused("bill_bad_participant_id")
    p = {"id": pid, "name": _str(raw.get("name", ""))}
    if "payTo" in raw:
        p["payTo"] = _str(raw["payTo"])
    if "identityKey" in raw:
        p["identityKey"] = _str(raw["identityKey"])
    # Section 9.1. An optional LIST reads null as absent: both denote none,
    # and the fallback is the empty list rather than a substituted value. A
    # scalar member does not get this — there the fallback would stand in for
    # something, which is why a null `currency` is refused and a null
    # `payouts` is not.
    if raw.get("payouts") is not None:
        if not isinstance(raw["payouts"], list):
            raise Refused("bill_type_error")
        outs = []
        for po in raw["payouts"]:
            if not isinstance(po, dict):
                raise Refused("bill_type_error")
            # Refused rather than skipped: skipping settles to the next
            # preference down, which is a different address.
            if po.get("type") not in PAYOUT_TYPES:
                raise Refused("bill_unknown_payout_method")
            # Section 9: a field of the wrong type is refused, optional or
            # not. These name the address money is sent to.
            for member in ("address", "asset", "chain"):
                if member in po:
                    _str(po[member])
            outs.append(po)
        p["payouts"] = outs
    return p


def split_participants(spec):
    """Every participant id a split names, whatever method it uses.

    Kept beside the split methods so a new method cannot add a place an id
    hides. The ids are returned rather than checked here: this function knows
    nothing about which bill a split belongs to.
    """
    out = set()
    if not isinstance(spec, dict):
        return out
    among = spec.get("among")
    if isinstance(among, list):
        out.update(x for x in among if isinstance(x, str))
    for key in ("amounts", "basisPoints", "shareCounts"):
        m = spec.get(key)
        if isinstance(m, dict):
            out.update(k for k in m if isinstance(k, str))
    items = spec.get("items")
    if isinstance(items, list):
        for item in items:
            if isinstance(item, dict):
                shared = item.get("sharedBy")
                if isinstance(shared, list):
                    out.update(x for x in shared if isinstance(x, str))
    return out


def decode_expense(raw, currency, ids):
    """Section 9.1, one expense, against the ids already on the bill."""
    if not isinstance(raw, dict):
        raise Refused("bill_type_error")
    cur = raw.get("currency", currency)
    check_currency(cur)
    if cur != currency:
        raise Refused("currency_mismatch")
    if raw.get("paidBy") not in ids:
        raise Refused("unknown_participant")
    # Every id a split names must be on the bill, not just `paidBy`. Without
    # this an expense splitting to a stranger is decoded, folded and kept, and
    # the refusal surfaces from `balances` on a bill that already looks whole.
    for pid in split_participants(raw.get("split")):
        if pid not in ids:
            raise Refused("unknown_participant")
    e = {"id": _str(raw.get("id")),
         "description": _str(raw.get("description", "")),
         "paidBy": raw["paidBy"],
         "amount": _int(raw.get("amount")),
         "currency": cur,
         "at": parse_instant(raw.get("at")),
         "split": raw.get("split")}
    if not isinstance(e["split"], dict):
        raise Refused("bill_type_error")
    return e


def decode_payment(raw, currency, ids):
    """Section 9.2, one payment, against the ids already on the bill."""
    if not isinstance(raw, dict):
        raise Refused("bill_type_error")
    cur = raw.get("currency", currency)
    check_currency(cur)
    if cur != currency:
        raise Refused("currency_mismatch")
    frm, to = raw.get("from"), raw.get("to")
    if frm not in ids or to not in ids:
        raise Refused("unknown_participant")
    if frm == to:
        raise Refused("self_payment")
    method = raw.get("method")
    if method not in SETTLEMENT_METHODS:
        raise Refused("bill_unknown_settlement_method")
    amount = _int(raw.get("amount"))
    if amount < 0:
        raise Refused("negative_amount")
    p = {"id": _str(raw.get("id")), "from": frm, "to": to,
         "amount": amount, "currency": cur, "method": method,
         "at": parse_instant(raw.get("at"))}
    if "zatoshi" in raw:
        z = _int(raw["zatoshi"])
        if z <= 0:
            raise Refused("negative_amount")
        p["zatoshi"] = z
    if "paidAtRate" in raw:
        r = raw["paidAtRate"]
        if not isinstance(r, dict):
            raise Refused("bill_type_error")
        check_currency(r.get("currency"))
        # Checked against the currency the payment states, never inherited.
        if r["currency"] != cur:
            raise Refused("rate_currency_mismatch")
        p["paidAtRate"] = r
    # Section 9.2. Both are free text a UI shows; both are still typed, for
    # the reason at the head of section 9: a field of the wrong type is
    # refused, optional or not.
    if "reference" in raw:
        p["reference"] = _str(raw["reference"])
    if "note" in raw:
        p["note"] = _str(raw["note"])
    return p


def decode_bill(doc):
    """Section 9. A field of the wrong type is refused, optional or not."""
    if not isinstance(doc, dict):
        raise Refused("bill_type_error")
    # Section 2.3, before any string is compared or encoded.
    _check_scalar_values(doc)

    if "v" not in doc:
        raise Refused("bill_missing_version")
    version = doc["v"]
    # A version written as a string is not a version.
    if isinstance(version, bool) or not isinstance(version, int):
        raise Refused("bill_missing_version")
    if version < 1:
        raise Refused("bill_type_error")
    if version > BILL_VERSION:
        raise Refused("bill_future_version")

    currency = doc.get("currency")
    if currency is None or currency == "":
        raise Refused("bill_missing_currency")
    check_currency(currency)

    mode = doc.get("splitMode", "equal")
    if not isinstance(mode, str):
        raise Refused("bill_type_error")
    if mode not in SPLIT_MODES:
        raise Refused("bill_unknown_split_mode")

    participants = []
    seen = set()
    for raw in doc.get("participants") or []:
        p = decode_participant(raw)
        if p["id"] in seen:
            raise Refused("duplicate_participant")
        seen.add(p["id"])
        participants.append(p)

    expenses = [decode_expense(raw, currency, seen)
                for raw in doc.get("expenses") or []]

    payments = [decode_payment(raw, currency, seen)
                for raw in doc.get("payments") or []]

    # Section 9.1. Carried through unchanged; absent means nothing is
    # confirmed, never everything.
    raw_confirmed = doc.get("confirmedPayments")
    if raw_confirmed is not None and not isinstance(raw_confirmed, list):
        raise Refused("bill_type_error")
    confirmed = []
    for cid in raw_confirmed or []:
        if not isinstance(cid, str):
            raise Refused("bill_type_error")
        confirmed.append(cid)

    bill = {"v": version, "id": _str(doc.get("id", "")),
           "name": _str(doc.get("name", "")), "currency": currency,
           "splitMode": mode, "participants": participants,
           "expenses": expenses, "payments": payments,
           "confirmedPayments": confirmed}
    if "rate" in doc:
        r = doc["rate"]
        if not isinstance(r, dict):
            raise Refused("bill_type_error")
        check_currency(r.get("currency"))
        bill["rate"] = r
    return bill


# --- Sections 5 and 6: balances and settlement ------------------------------

DEFAULT_EXACT_LIMIT = 14
MAX_EXACT_LIMIT = 20


def _confirmed(bill):
    """Section 5.1. Only a confirmed payment moves a balance.

    A document that omits `confirmedPayments` has confirmed nothing, not
    everything: reading it the other way settles a debt on the debtor's own
    unconfirmed claim, which section 5.1 forbids.
    """
    settled = bill.get("confirmedPayments") or []
    return [p for p in bill["payments"] if p["id"] in settled]



def _residual_is_zero(values):
    """Section 5.1, by cancellation.

    Deciding it with a running total, or with the sum of the positives, forms
    a value the set need not contain: both exceed a signed 64-bit integer for
    sets whose residual is zero and whose every member is representable.
    Cancelling largest against largest never forms one.
    """
    owed = sorted((v for v in values if v > 0), reverse=True)
    owes = sorted((-v for v in values if v < 0), reverse=True)
    while owed and owes:
        a = owed.pop(0)
        b = owes.pop(0)
        if a > b:
            owed.insert(0, a - b)
        elif b > a:
            owes.insert(0, b - a)
    return not owed and not owes


def _in_range(value):
    """Section 2.2. `value`, or `amount_overflow` when 64 bits cannot hold it."""
    if not I64_MIN <= value <= I64_MAX:
        raise Refused("amount_overflow")
    return value


def balances(bill):
    """Section 5.1."""
    net = {p["id"]: 0 for p in bill["participants"]}
    for e in bill["expenses"]:
        shares = split(e["amount"], e["split"])
        for pid in shares:
            if pid not in net:
                raise Refused("unknown_participant")
        net[e["paidBy"]] = _in_range(net[e["paidBy"]] + e["amount"])
        for pid, owed in shares.items():
            net[pid] = _in_range(net[pid] - owed)
    for p in _confirmed(bill):
        net[p["from"]] = _in_range(net[p["from"]] + p["amount"])
        net[p["to"]] = _in_range(net[p["to"]] - p["amount"])
    # The residual is a property of the set, not of an accumulation order: a
    # running total can exceed a signed 64-bit integer at some orderings of a
    # set whose total is zero, and the order a map yields is the
    # implementation's, not the document's.
    if not _residual_is_zero(net.values()):
        raise Refused("balances_nonzero_residual")
    return net


def _by_id(ids):
    return sorted(ids, key=lambda s: s.encode("utf-8"))


def creditors_debtors(net):
    """Section 5.1. Most owed first, largest debt first, ties by ascending id."""
    cred = [(i, net[i]) for i in _by_id(net) if net[i] > 0]
    debt = [(i, net[i]) for i in _by_id(net) if net[i] < 0]
    cred.sort(key=lambda kv: (-kv[1], kv[0].encode("utf-8")))
    debt.sort(key=lambda kv: (kv[1], kv[0].encode("utf-8")))
    return cred, debt


def direct_debts(bill):
    """Section 5.2. The debts as they arose, before any netting."""
    pairs = {}
    for e in bill["expenses"]:
        shares = split(e["amount"], e["split"])
        for pid, owed in shares.items():
            if pid == e["paidBy"] or owed == 0:
                continue
            pairs[(pid, e["paidBy"])] = pairs.get((pid, e["paidBy"]), 0) + owed
    out = [{"from": d, "to": c, "amount": a}
           for (d, c), a in pairs.items() if a != 0]
    out.sort(key=lambda r: (r["from"].encode("utf-8"), r["to"].encode("utf-8")))
    return out


def _zero_sum_groups(items, exact_limit):
    """Section 6.1. As many zero-sum groups as possible."""
    n = len(items)
    if n == 0:
        return [], True
    if n > exact_limit:
        return [list(range(n))], False

    values = [v for _, v in items]
    full = (1 << n) - 1
    # None marks a subset whose sum cannot be formed in a signed 64-bit
    # integer. It is therefore not zero and not a group — but the bill around
    # it may settle perfectly, so it is skipped rather than refused.
    sums = [0] * (1 << n)
    exact = [True] * (1 << n)
    for mask in range(1, 1 << n):
        low = mask & -mask
        rest = mask ^ low
        value = values[low.bit_length() - 1]
        if not exact[rest]:
            exact[mask] = False
            continue
        total = sums[rest] + value
        if total > I64_MAX or total < I64_MIN:
            exact[mask] = False
        else:
            sums[mask] = total

    best = [0] * (1 << n)
    pick = [0] * (1 << n)
    for mask in range(1, 1 << n):
        lowest = mask & -mask
        sub = mask
        while sub:
            if sub & lowest and exact[sub] and sums[sub] == 0:
                cand = best[mask ^ sub] + 1
                if cand > best[mask]:
                    best[mask], pick[mask] = cand, sub
            sub = (sub - 1) & mask
        if pick[mask] == 0:
            pick[mask] = mask

    groups, mask = [], full
    while mask:
        sub = pick[mask]
        groups.append([i for i in range(n) if sub >> i & 1])
        mask ^= sub
    return groups, True


def _exact_i64(value, code="amount_overflow"):
    """Section 2.2. A Python integer is arbitrary precision, so a sum that
    would wrap in a 64-bit implementation has to be checked for explicitly."""
    if value > I64_MAX or value < I64_MIN:
        raise Refused(code)
    return value


def settle(bill_or_net, exact_limit=DEFAULT_EXACT_LIMIT):
    """Section 6."""
    if exact_limit > MAX_EXACT_LIMIT:
        raise Refused("exact_limit_too_large")
    net = bill_or_net if isinstance(bill_or_net, dict) and all(
        isinstance(v, int) for v in bill_or_net.values()) else balances(bill_or_net)

    # Section 5.1: balances reaching the search sum to zero, checked without
    # wrapping.
    # Summed in ascending id order (§2.3), so the check does not depend on a
    # map's iteration order: Dart preserves insertion and Rust sorts.
    # A balance of the most negative 64-bit integer has no positive
    # counterpart, so no settlement amount can carry it (§2.2, §8.1).
    if any(v == I64_MIN for v in net.values()):
        raise Refused("amount_overflow")
    if not _residual_is_zero(net.values()):
        raise Refused("balances_nonzero_residual")

    items = [(i, net[i]) for i in _by_id(net) if net[i] != 0]
    groups, optimal = _zero_sum_groups(items, exact_limit)

    settlements = []
    for group in groups:
        bal = {items[i][0]: items[i][1] for i in group}
        while True:
            debtors = sorted(((v, k) for k, v in bal.items() if v < 0),
                             key=lambda kv: (kv[0], kv[1].encode("utf-8")))
            creds = sorted(((v, k) for k, v in bal.items() if v > 0),
                           key=lambda kv: (-kv[0], kv[1].encode("utf-8")))
            if not debtors or not creds:
                break
            (dv, d), (cv, c) = debtors[0], creds[0]
            amount = min(-dv, cv)
            settlements.append({"from": d, "to": c, "amount": amount})
            bal[d] += amount
            bal[c] -= amount

    settlements.sort(key=lambda s: (s["from"].encode("utf-8"),
                                    s["to"].encode("utf-8")))
    return {"settlements": settlements, "isOptimal": optimal,
            "paymentCount": len(settlements)}


def attribute_coverage(settlements, debts):
    """Section 6.3. The original debts each settlement discharges.

    A payer's direct debts, in their section 5.2 order, are consumed against
    that payer's settlements in plan order, each settlement taking as much of
    each remaining debt as it needs.
    """
    remaining = {}
    for d in debts:
        remaining.setdefault(d["from"], []).append([d["to"], d["amount"]])

    out = []
    for s in settlements:
        need = s["amount"]
        covers = []
        for row in remaining.get(s["from"], []):
            if need <= 0:
                break
            # A pair aggregating to a negative amount is a credit on that pair.
            if row[1] <= 0:
                continue
            take = min(need, row[1])
            covers.append({"from": s["from"], "to": row[0], "amount": take})
            row[1] -= take
            need -= take
        # Membership is not the test: a payment covering a little of what the
        # payee lent and a great deal of what others lent is still rerouted.
        elsewhere = sum(c["amount"] for c in covers if c["to"] != s["to"])
        # Section 6.3. The part no direct debt explains: a peer may attribute
        # a refund to somebody who never agreed to it, and the victim's
        # settlement then exceeds every debt the bill records for them.
        covered = sum(c["amount"] for c in covers)
        out.append({**s, "covers": covers, "rerouted": elsewhere > 0,
                    "unexplained": max(s["amount"] - covered, 0)})
    return out


def settle_bill(bill, exact_limit=DEFAULT_EXACT_LIMIT):
    """Section 6, planned from a bill, so section 6.3 coverage is available."""
    plan = settle(balances(bill), exact_limit)
    plan["settlements"] = attribute_coverage(plan["settlements"],
                                             direct_debts(bill))
    return plan


# --- Section 10: the log ----------------------------------------------------

ENTRY_KINDS = ("createBill", "joinBill", "addExpense", "amendEntry", "voidEntry",
               "recordPayment", "confirmPayment", "setRate")
PAYLOAD_FOR = {"joinBill": "participant", "addExpense": "expense",
               "recordPayment": "payment", "confirmPayment": "confirmation",
               "setRate": "rate"}
PAYLOADS = ("rate", "expense", "payment", "confirmation")
# Every member that is a payload. PAYLOADS drives the ambiguity check and
# omits `participant`; the type check at ingress must not, because an
# amendEntry may carry any of them and every later pass indexes what it finds.
PAYLOAD_MEMBERS = PAYLOADS + ("participant",)
BILL_ID_DOMAIN = "splitz-bill-id-v1"
ENTRY_ID_DOMAIN = "splitz-entry-id-v1"
KEY_BYTES, NONCE_LEN = 32, 16


def _b64url_len(text, want):
    import base64
    if not isinstance(text, str) or any(c not in B64URL_ALPHABET for c in text):
        return False
    try:
        raw = base64.urlsafe_b64decode(text + "=" * (-len(text) % 4))
    except Exception:
        return False
    # Section 9.4: canonical, so re-encoding reproduces it exactly.
    return len(raw) == want and b64url(raw) == text


def _derive_id(domain, entry):
    import base64, hashlib
    body = {k: v for k, v in entry.items() if k not in ("id", "sig", "v")}
    digest = hashlib.sha256(
        domain.encode("utf-8") + canonical_json(body).encode("utf-8")
    ).digest()[:16]
    return base64.urlsafe_b64encode(digest).decode("ascii").rstrip("=")


def derive_bill_id(entry):
    """Section 9.4. The digest of the entry that opens the bill."""
    return _derive_id(BILL_ID_DOMAIN, entry)


def derive_entry_id(entry):
    """Section 9.5. The digest of the entry, for every other kind."""
    return _derive_id(ENTRY_ID_DOMAIN, entry)


# The members of a payload that name a participant or an entry (section 10.1).
_ID_MEMBERS_OF = {
    "participant": ("id",),
    "expense": ("id", "paidBy"),
    "payment": ("id", "from", "to"),
    "confirmation": ("paymentId",),
}


def _check_id_lists(spec):
    """Section 10.1, section 4. An id list holds only strings.

    A reader that drops a member it cannot read reassigns that participant's
    share to the others.
    """
    for key in ("among", "sharedBy"):
        value = spec.get(key)
        if isinstance(value, list) and any(not isinstance(v, str) for v in value):
            raise Refused("bill_type_error")
    for key in ("amounts", "basisPoints", "shareCounts"):
        value = spec.get(key)
        if isinstance(value, dict) and any(
                not isinstance(k, str) for k in value):
            raise Refused("bill_type_error")
    items = spec.get("items")
    if isinstance(items, list):
        for item in items:
            if isinstance(item, dict):
                _check_id_lists(item)


def seal_log(entries):
    """Section 9.5. Derives every entry's id and rewrites references to it.

    A withdrawal's id covers its `targetId`, so sealing the target changes the
    withdrawal in turn: the pass repeats until no id moves. Two entries naming
    each other never settle, and such a log cannot be built at all — which is
    the point of deriving the id rather than choosing it.
    """
    out = [dict(e) for e in entries]
    for _ in range(len(out) + 2):
        remap = {}
        # Two DIFFERENT entries carrying one written id give that id two
        # answers, and a withdrawal naming it would follow whichever was
        # rewritten last. There is no correct retarget, so such a log is not
        # one this pass can build. Two copies of one entry derive the same id
        # and are not ambiguous.
        # Ambiguity is over the written ids of EVERY entry, not only those
        # whose id already differs from their digest. One id written by two
        # entries whose digests differ has no single answer whichever of the
        # two happens to be correct already.
        claims = {}
        for e in out:
            if e.get("kind") not in ENTRY_KINDS:
                continue
            old_id = e.get("id")
            want = (derive_bill_id(e) if e["kind"] == "createBill"
                    else derive_entry_id(e))
            # An entry carrying no id names nothing and is named by nothing,
            # so several of them are not a collision.
            if old_id is not None:
                if claims.setdefault(old_id, want) != want:
                    return None
            if old_id != want:
                if old_id is not None:
                    remap[old_id] = want
                e["id"] = want
        moved = bool(remap)
        for e in out:
            # An entry that carries no id maps under None, which is not a
            # target any withdrawal names.
            if "targetId" in e and e["targetId"] in remap:
                e["targetId"] = remap[e["targetId"]]
                moved = True
        if not moved:
            return out
    return None  # never settles: a reference cycle


# Section 10.4: the member naming what each kind of entry is about, which an
# amendment may not change.
AMENDED_SUBJECT = {
    "joinBill": ("participant", "id"),
    "addExpense": ("expense", "id"),
    "recordPayment": ("payment", "id"),
    "confirmPayment": ("confirmation", "paymentId"),
}


def check_entry(entry):
    """Section 10.1. Refused before the entry reaches a log."""
    if not isinstance(entry, dict):
        raise Refused("bill_type_error")
    # Section 10.1, before anything that walks the entry — the scalar-value
    # check below and the id derivation both recurse. An entry arriving over a
    # relay (section 11.3) never passes section 11.2's cap, so a depth nobody
    # bounded here is a stack the peer chose.
    if not _depth(entry, MAX_DOCUMENT_DEPTH):
        raise Refused("bill_type_error")
    kind = entry.get("kind")
    if kind not in ENTRY_KINDS:
        raise Refused("bill_unknown_entry_kind")
    # Section 10.1. A signature is a string or absent. Anything else is a
    # third state section 10.2 would have to rank, and `null` sorts above
    # every string, so it would win every merge it entered.
    if "sig" in entry and not isinstance(entry["sig"], str):
        raise Refused("bill_type_error")
    # Section 10.1. `v` sits outside the id (section 9.5), so a copy with any
    # value keeps the honest id; one that is not an integer would reach the
    # canonical encoding the merge and the order compare, and stop there.
    if "v" in entry:
        v = entry["v"]
        if isinstance(v, bool) or not isinstance(v, int) or v < 1:
            raise Refused("bill_type_error")
    # Section 2.3, at entry ingress: the section 10.2 order depends on it.
    _check_scalar_values(entry)

    carried = [p for p in PAYLOADS if p in entry]
    if len(carried) > 1:
        raise Refused("bill_ambiguous_entry")
    # Every payload an entry carries is an object, whatever the kind names.
    # An amendEntry carries the payload it replaces and no kind declares it,
    # so without this a scalar reaches the fold and is indexed there.
    for name in PAYLOAD_MEMBERS:
        if name in entry and not isinstance(entry[name], dict):
            raise Refused("bill_type_error")

    if "targetId" in entry and not isinstance(entry["targetId"], str):
        raise Refused("bill_type_error")

    want = PAYLOAD_FOR.get(kind)
    if want and want not in entry:
        raise Refused("bill_missing_entry_payload")
    # Section 10.1. Every later pass indexes the payload without re-checking
    # it, and the fold's authorisation pass reads the target's payload to
    # decide who may withdraw an entry.
    if want:
        payload = entry[want]
        if not isinstance(payload, dict):
            raise Refused("bill_type_error")
        for member in _ID_MEMBERS_OF.get(want, ()):
            value = payload.get(member)
            if value is not None and not isinstance(value, str):
                raise Refused("bill_type_error")
        if isinstance(payload.get("split"), dict):
            _check_id_lists(payload["split"])
    if kind in ("voidEntry", "amendEntry") and not entry.get("targetId"):
        raise Refused("bill_missing_entry_payload")

    parse_instant(entry.get("at"))
    _str(entry.get("id"))
    _str(entry.get("author"))

    if kind == "createBill":
        # Section 9.4: both fields, or the entry is unbound.
        if not _b64url_len(entry.get("creatorKey", ""), KEY_BYTES) \
                or not _b64url_len(entry.get("nonce", ""), NONCE_LEN):
            raise Refused("create_unbound")
        # The fold copies these into the bill document without re-reading
        # them, so they are decided here rather than at decode, where the
        # whole bill would be unopenable instead of this entry refused.
        if "name" in entry and not isinstance(entry["name"], str):
            raise Refused("bill_type_error")
        check_currency(entry.get("currency"))
        mode = entry.get("splitMode", "equal")
        if not isinstance(mode, str):
            raise Refused("bill_type_error")
        if mode not in SPLIT_MODES:
            raise Refused("bill_unknown_split_mode")
        if entry["id"] != derive_bill_id(entry):
            raise Refused("create_id_not_derived")
    elif entry["id"] != derive_entry_id(entry):
        # Section 9.5. An id anyone may choose is an id anyone may take.
        raise Refused("entry_id_not_derived")
    return entry


def merge(*logs):
    """Section 10.2. Set union keyed by entry id and signature.

    Returns (merged, refused). Section 10.1 is applied at ingress: an entry
    that does not carry the payload its kind uses never enters the union.
    Removing a payload member makes an entry sort higher under section 9.3, so
    without this the stripped copy wins rule 3 and displaces the genuine entry
    on every device.

    A signed copy beats an unsigned one. Two signed copies with different
    signatures are both kept: section 9.5's digest does not cover `sig`, so
    nothing in the pair says which is the author's, and the fold decides with
    the key (section 10.3). Copies sharing a signature, or all unsigned,
    resolve to the one whose canonical encoding sorts higher.
    """
    copies = {}
    refused = []
    for log in logs:
        for entry in log:
            try:
                check_entry(entry)
            except Refused as e:
                # Coerced, not taken raw: the refusal path is total over
                # every value the check refuses, including a non-string id.
                reported = entry.get("id") if isinstance(entry, dict) else None
                refused.append(
                    {"id": reported if isinstance(reported, str) else "",
                     "code": e.code})
                continue
            held = copies.setdefault(entry["id"], {})
            sig = entry.get("sig")
            other = held.get(sig)
            if other is None or (canonical_json(entry).encode("utf-8")
                                 > canonical_json(other).encode("utf-8")):
                held[sig] = entry
    out = []
    for held in copies.values():
        signed = [e for sig, e in held.items() if sig is not None]
        out.extend(signed if signed else list(held.values()))
    # Section 10.2. Total: two rows sharing an id are ordered by their code.
    refused.sort(key=lambda r: ((r["id"] or "").encode("utf-8"),
                                r["code"].encode("utf-8")))
    return order(out), refused


def order(entries):
    """Section 10.2. at, then author, then id, then canonical encoding.

    The order is total: a key that ties for two entries that are not equal
    leaves them to the host's sort, and a sort stable in one language and not
    in another then gives one input two different bill documents.
    """
    return sorted(entries, key=lambda e: (e["at"].encode("utf-8"),
                                          e["author"].encode("utf-8"),
                                          e["id"].encode("utf-8"),
                                          canonical_json(e).encode("utf-8")))


def _destination(participant):
    """Section 10.3 step 4. Where a participant is paid: the address of their
    first payout when they declare any, their payTo otherwise, or None."""
    payouts = participant.get("payouts")
    if isinstance(payouts, list) and payouts:
        first = payouts[0] if isinstance(payouts[0], dict) else {}
        address = first.get("address")
    else:
        address = participant.get("payTo")
    return address if isinstance(address, str) else None


def fold(entries, bill_id=None, verify=None):
    """Section 10.3. Returns the bill and everything the fold reached."""
    if not entries:
        raise Refused("log_empty")

    # Section 10.3. An entry that cannot be applied is set aside, never raised
    # as a failure of the whole fold: the log merges by union, so one
    # malformed entry reaches every device, and aborting leaves the bill
    # unopenable -- including unopenable to append the withdrawal that would
    # remove it.
    refused_at_ingress, admitted = [], []
    for e in entries:
        try:
            check_entry(e)
            admitted.append(e)
        except Refused as r:
            eid = e.get("id") if isinstance(e, dict) else None
            refused_at_ingress.append(
                {"id": eid if isinstance(eid, str) else "", "code": r.code})

    # Section 10.3. Every copy the merge kept, several under one id when their
    # signatures differ.
    copies = order(merge(admitted)[0])
    creates = [e for e in copies if e["kind"] == "createBill"]
    if bill_id is not None:
        creates = [e for e in creates if e["id"] == bill_id]
    if verify is not None:
        # Section 10.1. A host that verifies MUST check a create entry's
        # signature against the creatorKey that same entry states. A create
        # is refused only when no copy of it verifies.
        kept = [e for e in creates if verify(e, e.get("creatorKey", ""))]
        for eid in sorted({e["id"] for e in creates} - {e["id"] for e in kept}):
            refused_at_ingress.append({"id": eid, "code": "unauthorized_entry"})
        creates = kept
    create_ids = {e["id"] for e in creates}
    if not create_ids:
        raise Refused("log_no_create")
    if len(create_ids) > 1:
        raise Refused("ambiguous_create")
    create = max(creates, key=lambda e: canonical_json(e).encode("utf-8"))

    # Section 10.7, over every copy: a withdrawal does not undo a claim, and a
    # copy nobody applies is still evidence that was made.
    identities = (({}, set()) if verify is None
                  else resolve_identities(copies, create, verify))

    # Section 10.3. One id names one entry: a re-sent entry would otherwise be
    # applied twice. An author with a key is spoken for only by a copy that
    # verifies against it.
    groups = {}
    for e in copies:
        groups.setdefault(e["id"], []).append(e)
    entries = []
    for eid, group in groups.items():
        author = group[0]["author"]
        key = None
        if verify is not None:
            if group[0]["kind"] == "createBill":
                key = group[0].get("creatorKey")
            else:
                key = identities[0].get(author)
        if key is not None:
            group = [e for e in group if verify(e, key)]
            if not group:
                refused_at_ingress.append(
                    {"id": eid, "code": "unauthorized_entry"})
                continue
        entries.append(max(group, key=lambda e: canonical_json(e).encode("utf-8")))
    entries = order(entries)

    currency = create.get("currency")
    check_currency(currency)
    mode = create.get("splitMode", "equal")
    if mode not in SPLIT_MODES:
        raise Refused("bill_unknown_split_mode")

    voided, amendments = set(), {}
    set_aside = list(refused_at_ingress)

    def aside(entry, code, why):
        # The code is part of the protocol; the message beside it is prose and
        # is not (SPEC.md §12), so only the code is recorded here.
        del why
        set_aside.append({"id": entry["id"], "code": code})

    by_id = {e["id"]: e for e in entries}

    # Amendments: authored by the author of their target, same payload kind.
    for e in entries:
        if e["kind"] != "amendEntry":
            continue
        target = by_id.get(e["targetId"])
        if target is None:
            aside(e, "unknown_entry", "amends an entry the log does not hold")
            continue
        if e["author"] != target["author"]:
            aside(e, "unauthorized_entry", "amends an entry it did not write")
            continue
        want = PAYLOAD_FOR.get(target["kind"])
        if want and want not in e:
            aside(e, "amend_kind_mismatch", "carries no payload of its target's kind")
            continue
        # Section 10.4. The id the target is about stays: renaming it makes a
        # different entry the section 10.8 checks never read.
        subject = AMENDED_SUBJECT.get(target["kind"])
        if subject and e[subject[0]].get(subject[1]) != target[subject[0]].get(subject[1]):
            aside(e, "amend_kind_mismatch", "renames what its target is about")
            continue
        amendments[e["targetId"]] = e

    creator = create["author"]

    # Withdrawals, section 10.8, in two stages.
    #
    # Authorisation first, because only a withdrawal that is allowed to stand
    # may take another one back. Resolving in force over every withdrawal lets
    # a stranger cancel a legitimate one: the fold would report their entry
    # refused and honour it in the same breath.
    voids = [e for e in entries if e["kind"] == "voidEntry"]
    authorised = {}
    for e in voids:
        target = by_id.get(e["targetId"])
        if target is None:
            aside(e, "unknown_entry", "withdraws an entry the log does not hold")
            authorised[e["id"]] = False
            continue
        kind = target["kind"]
        if kind == "addExpense":
            allowed = {target["author"], creator}
        elif kind == "recordPayment":
            pay = target.get("payment", {})
            allowed = {target["author"], pay.get("from"), pay.get("to")}
        elif kind == "joinBill":
            allowed = {creator, target.get("participant", {}).get("id")}
        else:
            allowed = {target["author"]}
        if e["author"] not in allowed:
            aside(e, "unauthorized_entry", f"may not withdraw a {kind}")
            authorised[e["id"]] = False
            continue
        authorised[e["id"]] = True

    # A withdrawal is in force unless an authorised withdrawal naming it is
    # itself in force. `at` plays no part: an id is the digest of its entry,
    # so a withdrawal can only name one that existed when it was written, and
    # the chains are acyclic. Resolved by what names what, from the entries
    # nothing names inwards.
    naming = {}
    for e in voids:
        naming.setdefault(e["targetId"], []).append(e)
    in_force = {}

    def decide(v):
        if v["id"] in in_force:
            return in_force[v["id"]]
        stack = [(v, False)]
        while stack:
            cur, expanded = stack.pop()
            if cur["id"] in in_force:
                continue
            if not authorised[cur["id"]]:
                in_force[cur["id"]] = False
                continue
            namers = naming.get(cur["id"], [])
            pending = [w for w in namers if w["id"] not in in_force]
            if pending and not expanded:
                stack.append((cur, True))
                stack.extend((w, False) for w in pending)
                continue
            in_force[cur["id"]] = not any(in_force[w["id"]] for w in namers)
        return in_force[v["id"]]

    for e in voids:
        decide(e)

    for e in voids:
        if in_force[e["id"]]:
            voided.add(e["targetId"])

    # An amendment whose own entry was withdrawn is discarded with it, so the
    # entry it corrected reads as it was written. Collecting amendments before
    # withdrawals are resolved and applying them afterwards would leave a
    # retracted correction standing: the figure a person took back would be
    # the figure the bill shows.
    #
    # This precedes the still-named check below, which reads the amendments:
    # evaluating that check against a version the fold will go on to discard
    # lets a participant be removed while a surviving entry still names them.
    amendments = {
        target: e for target, e in amendments.items() if e["id"] not in voided
    }

    # Taking somebody off the bill, section 10.8. This runs after every other
    # withdrawal is resolved and before the joins are applied: a check made
    # once the person is gone is a check made too late.
    removals = [e for e in entries
                if e["kind"] == "voidEntry" and e["targetId"] in voided
                and by_id[e["targetId"]]["kind"] == "joinBill"]
    for e in removals:
        target = by_id[e["targetId"]]
        gone = target.get("participant", {}).get("id")
        surviving = [o for o in entries
                     if o["id"] not in voided and o["kind"] != "voidEntry"]
        named = False
        for other in surviving:
            eff = amendments.get(other["id"], other)
            if other["kind"] == "addExpense":
                # Total readers, not indexing: this pass runs before the
                # expense is decoded, so `split` and everything under it is
                # whatever a peer wrote. Section 10.1 types the payload
                # itself; it does not type inside it.
                ex = _as_dict(eff.get("expense"))
                sp = _as_dict(ex.get("split"))
                pool = set(_as_list(sp.get("among")))
                for member in ("amounts", "basisPoints", "shareCounts"):
                    pool |= set(_as_dict(sp.get(member)))
                for item in _as_list(sp.get("items")):
                    pool |= set(_as_list(_as_dict(item).get("sharedBy")))
                if ex.get("paidBy") == gone or gone in pool:
                    named = True
            elif other["kind"] == "recordPayment":
                pay = _as_dict(eff.get("payment"))
                if gone in (pay.get("from"), pay.get("to")):
                    named = True
            elif other["kind"] == "confirmPayment" and other["author"] == gone:
                named = True
            if named:
                break
        if named:
            voided.discard(e["targetId"])
            aside(e, "participant_still_named",
                  "a surviving entry still names that participant")

    live = [e for e in entries if e["id"] not in voided and e["kind"] != "voidEntry"]

    def effective(entry):
        return amendments.get(entry["id"], entry)

    # Participants, in a pass of their own.
    participants, replaced = {}, []
    for e in live:
        if e["kind"] != "joinBill":
            continue
        p = effective(e).get("participant", {})
        pid = p.get("id")
        if pid is None or pid == "":
            aside(e, "bill_missing_entry_payload", "names no participant")
            continue
        if pid in identities[0] and e["author"] != pid:
            # Section 10.7. A bound participant's record is theirs to create
            # as well as to change.
            aside(e, "unauthorized_entry", "writes a bound participant's record")
            continue
        if pid in participants and e["author"] != pid:
            aside(e, "unauthorized_entry", "changes a record it does not own")
            continue
        # The decoder decides what a participant is, here rather than once the
        # document is assembled: a member it would refuse sets this entry
        # aside (section 10.3) instead of making the whole bill undecodable.
        try:
            decode_participant(p)
        except Refused as r:
            aside(e, r.code, "carries a participant this reader cannot decode")
            continue
        # Section 10.3 step 4. The destination this record replaces: the one
        # held for the participant, or, for the first record, the one the
        # join was written with before an amendment changed it. A destination
        # the decoder never read is taken as none.
        if pid in participants:
            before = _destination(participants[pid])
        elif e["id"] in amendments:
            before = _destination(_as_dict(e.get("participant")))
        else:
            before = _destination(p)
        if before != _destination(p):
            replaced.append({"id": pid, "from": before, "to": _destination(p)})
        participants[pid] = p

    # Section 10.1. The latest live setRate by a participant decides, by
    # section 10.2's order, so the answer is a function of the log and not of
    # which device last spoke. Decided after the participants, because only
    # they may set it.
    rate = None
    for e in live:
        if e["kind"] != "setRate":
            continue
        if e["author"] not in participants:
            aside(e, "unknown_participant", "sets a rate on a bill it is not on")
            continue
        payload = effective(e).get("rate")
        try:
            decode_rate(payload)
        except Refused as r:
            aside(e, r.code, "carries a rate this reader cannot decode")
            continue
        rate = payload

    expenses, payments = [], []
    # Who wrote each applied payment record, by its id: section 14.4 withholds
    # only for a record the payer wrote.
    payment_authors = {}
    # Section 5.1's balances, formed as this pass applies each entry and in
    # the order section 5.1 forms them, so a bill this fold returns always has
    # balances section 2.2 can hold. An entry whose effect would carry one out
    # of range is set aside, deterministically and in log order, rather than
    # left to make section 5 refuse the whole bill.
    running = {pid: 0 for pid in participants}
    pair_total = {}

    def _fits(v):
        return I64_MIN <= v <= I64_MAX

    for e in live:
        eff = effective(e)
        if e["kind"] == "addExpense":
            ex = dict(eff["expense"])
            # Section 9.1 falls back only when the member is ABSENT. A present
            # value that is not a currency is an entry that cannot be applied,
            # and section 10.3 sets those aside rather than raising: one such
            # entry must not take the bill with it.
            if "currency" not in ex:
                ex["currency"] = currency          # denominated by the fold
            elif not is_currency(ex["currency"]):
                aside(e, "bill_bad_currency", "states a value that is not a currency")
                continue
            elif ex["currency"] != currency:
                aside(e, "currency_mismatch", "states a currency the bill does not use")
                continue
            if ex.get("paidBy") not in participants:
                aside(e, "unknown_participant", "paid by somebody not on the bill")
                continue
            try:
                decode_expense(ex, currency, set(participants))
                # Section 4 is what turns an expense into what each person
                # owes, and section 5 runs it downstream of this fold. An
                # expense whose split section 4 refuses cannot be applied, so
                # it is set aside here rather than raising out of `balances`
                # once the bill is already built.
                shares = split(_int(ex.get("amount")), ex.get("split"))
            except Refused as r:
                aside(e, r.code, "carries an expense this reader cannot apply")
                continue
            moved = dict(running)
            moved[ex["paidBy"]] += ex["amount"]
            ok = _fits(moved[ex["paidBy"]])
            for pid, owed in shares.items():
                moved[pid] -= owed
                ok = ok and _fits(moved[pid])
            if not ok:
                aside(e, "amount_overflow", "would carry a balance out of range")
                continue
            running = moved
            expenses.append(ex)
        elif e["kind"] == "recordPayment":
            pay = dict(eff["payment"])
            if e["author"] not in (pay.get("from"), pay.get("to")):
                aside(e, "unauthorized_payment", "written by neither party")
                continue
            if pay.get("from") not in participants or pay.get("to") not in participants:
                aside(e, "unknown_participant", "names somebody not on the bill")
                continue
            if pay.get("from") == pay.get("to"):
                aside(e, "self_payment", "pays its own author")
                continue
            if "currency" not in pay:
                pay["currency"] = currency
            elif not is_currency(pay["currency"]):
                aside(e, "bill_bad_currency", "states a value that is not a currency")
                continue
            elif pay["currency"] != currency:
                aside(e, "currency_mismatch", "states a currency the bill does not use")
                continue
            try:
                decode_payment(pay, currency, set(participants))
            except Refused as r:
                aside(e, r.code, "carries a payment this reader cannot decode")
                continue
            # SPEC.md 10.5: a confirmation names one record, and a method that
            # speaks for the payment's `to` is checked against that record's
            # `to`. Two records under one id name a payee ambiguously, so one
            # recipient's confirmation would settle a debt another never
            # vouched for. The first stands; the second is refused.
            if any(p["id"] == pay["id"] for p in payments):
                aside(e, "duplicate_payment",
                      "carries a payment id the bill already holds")
                continue
            # What one participant has recorded paying another, confirmed or
            # not, stays in range: section 14.4 sums the unconfirmed part of it.
            pair = (pay["from"], pay["to"])
            total = pair_total.get(pair, 0) + pay["amount"]
            if not _fits(total):
                aside(e, "amount_overflow", "would carry a total out of range")
                continue
            pair_total[pair] = total
            payments.append(pay)
            payment_authors[pay["id"]] = e["author"]

    # Confirmations, in a pass of their own once every payment is on the bill.
    known = {p["id"] for p in payments}
    confirmed = set()
    confirmed_by = {}
    for e in live:
        if e["kind"] != "confirmPayment":
            continue
        c = effective(e).get("confirmation", {})
        method = c.get("method")
        if not isinstance(method, str) or method not in CONFIRMATION_METHODS:
            aside(e, "bill_unknown_confirmation_method", f"method {method!r}")
            continue
        speaks_for, needs_ref, settles = CONFIRMATION_METHODS[method]
        if c.get("paymentId") not in known:
            aside(e, "unknown_payment", "vouches for a payment the bill does not hold")
            continue
        if e["author"] not in participants:
            aside(e, "unknown_participant", "written by somebody not on the bill")
            continue
        pay = next(p for p in payments if p["id"] == c["paymentId"])
        if speaks_for and e["author"] != pay[speaks_for]:
            aside(e, "unauthorized_confirmation",
                  f"{method} speaks for the payment's {speaks_for}")
            continue
        # A non-empty STRING, not merely something truthy. A number or a list
        # here is not a transaction id, and reading "present" three different
        # ways settles a debt on one device and leaves it open on another.
        ref = c.get("reference")
        if needs_ref and not (isinstance(ref, str) and ref):
            aside(e, "confirmation_missing_reference", f"{method} names no transaction")
            continue
        if settles:
            confirmed.add(c["paymentId"])
            confirmed_by.setdefault(c["paymentId"], []).append(e)

    # Confirmed payments move balances in the order the bill lists them
    # (section 5.1). One that would carry a balance out of range stays
    # unconfirmed, and every confirmation that settled it is set aside.
    for pay in payments:
        if pay["id"] not in confirmed:
            continue
        frm = running[pay["from"]] + pay["amount"]
        to = running[pay["to"]] - pay["amount"]
        if _fits(frm) and _fits(to):
            running[pay["from"]], running[pay["to"]] = frm, to
            continue
        confirmed.discard(pay["id"])
        for e in confirmed_by[pay["id"]]:
            aside(e, "amount_overflow", "would carry a balance out of range")

    return {
        "bill": {"v": BILL_VERSION, "id": create["id"], "name": create.get("name", ""),
                "currency": currency, "splitMode": mode,
                "participants": [participants[i] for i in _by_id(participants)],
                "expenses": expenses, "payments": payments,
                "confirmedPayments": sorted(confirmed),
                 **({"rate": rate} if rate else {})},
        "creator": creator,
        # Section 10.7, over the same entry set the bill was materialised
        # from. Without a verifier nothing can be decided, and nothing is
        # claimed.
        "identities": (
            {"bound": {}, "contested": []} if verify is None else
            {"bound": {k: identities[0][k] for k in sorted(identities[0])},
             "contested": sorted(identities[1])}),
        "replacedAddresses": replaced,
        "paymentAuthors": {k: payment_authors[k]
                           for k in sorted(payment_authors,
                                           key=lambda k: k.encode("utf-8"))},
        "withdrawn": sorted(voided),
        # Section 10.2. Total: rows sharing an id are ordered by code.
        "setAside": sorted(set_aside, key=lambda r: (r["id"].encode("utf-8"),
                                                    r["code"].encode("utf-8"))),
    }


# --- Section 10.6: what a signature covers ------------------------------------

ENTRY_SIGNING_DOMAIN = "splitz-entry-v1"


def signing_message(entry):
    """Section 10.6. The bytes an entry's signature covers.

    `sig` is excluded because it is the output, and `v` because an entry does
    not carry its own format version through an implementation's object model:
    a reader re-encodes with the version it writes, so a signature covering
    `v` would stop verifying for every existing entry the day it changed.

    Returned as text rather than as a verdict. A case asserting that a
    signature verified would pass in two implementations that disagree about
    the bytes, each checking its own.
    """
    body = {k: v for k, v in entry.items() if k not in ("sig", "v")}
    return ENTRY_SIGNING_DOMAIN + canonical_json(body)


# --- Section 10.7: who a participant is ---------------------------------------

def resolve_identities(entries, create, verify):
    """Section 10.7. Which key, if any, is bound to each participant id.

    `verify(entry, key)` is the host's curve operation; this fixes everything
    around it. The answer is a function of the entry set alone — never of
    arrival order, never of anything on disk.

    Returns (bound, contested), where `bound` maps a participant id to the key
    that speaks for it and `contested` is the set of ids two keys each claim.
    """
    creator = create["author"]
    creator_key = create.get("creatorKey")

    # The creator is bound by the invite, not by a join: the bill's id is the
    # digest of the entry that states their key, so it needs no prior
    # acquaintance. The signature requirement is not ornamental — absent it,
    # `creatorKey` is a number the author typed.
    bound = {}
    if creator_key and verify(create, creator_key):
        bound[creator] = creator_key

    # A key is bound by a self-claim: a joinBill whose author is the
    # participant it carries, stating a key, whose signature verifies against
    # that key. An entry naming somebody else proves nothing about them.
    claims = {}
    for e in entries:
        if e["kind"] != "joinBill":
            continue
        p = e.get("participant") or {}
        pid = p.get("id")
        key = p.get("identityKey")
        if pid is None or key is None or e["author"] != pid:
            continue
        if not verify(e, key):
            continue
        claims.setdefault(pid, set()).add(key)

    contested = set()
    for pid, keys in claims.items():
        if pid == creator:
            # A join claiming the creator's id is not a rival claim; §10.7
            # refuses it rather than contesting an identity the invite proves.
            continue
        if len(keys) > 1:
            # Nothing internal to the log says which is the person: `at` is
            # whatever its author wrote, so resolving by time hands the
            # identity to whoever backdates furthest.
            contested.add(pid)
        else:
            bound[pid] = next(iter(keys))

    return bound, contested


# --- Section 8.5: one payer's obligation --------------------------------------

def published_address(participant):
    """Section 9.1. The Zcash address a participant published, as written."""
    payouts = participant.get("payouts") or []
    if payouts:
        first = payouts[0]
        return first.get("address") if first.get("type") == "zec" else None
    return participant.get("payTo")


def payable_address(participant):
    """Section 9.1. The address a payment request can carry, if any.

    Returns the address rather than a flag, so a caller cannot reach for one
    that is not there. One section 8.3 does not admit is not returned: the
    renderer would refuse the whole request over it, past the caller's choice
    to report an unpayable recipient instead of refusing.
    """
    address = published_address(participant)
    return address if address and _ascii_alnum(address) else None


def delta_for(entries, they_have):
    """Section 14.5. What a peer has not seen, and whether it fits one square.

    Three answers, not two: a peer who holds everything and a peer who holds
    none of a log too long to encode are opposite states, and one value for
    both tells somebody their bill is up to date while entries on it have
    never reached them.

    Returns `{"state": "nothing"}`,
    `{"state": "square", "uri": ..., "entryCount": n}` or
    `{"state": "too_big", "entryCount": n, "code": ...}`.
    """
    known = set(they_have)
    missing = [e for e in order(entries) if e.get("id") not in known]
    if not missing:
        return {"state": "nothing", "entryCount": 0}
    try:
        uri = encode_payload(DELTA_PREFIX, {"v": 1, "log": missing})
    except Refused as r:
        return {"state": "too_big", "entryCount": len(missing), "code": r.code}
    return {"state": "square", "uri": uri, "entryCount": len(missing)}


def withholdings(plan, bill, payer, contested_ids=(), pay_anyway=(),
                 recorded_by=None):
    """Splits `payer`'s settlements into what a request may carry and what
    section 14 holds back.

    Pure: reads the bill and the identities the fold resolved, and decides
    nothing a wallet is entitled to decide. `pay_anyway` names the contested
    ids a payer has accepted after being shown them, which section 10.7
    permits and which is the only way through a contest. `recorded_by` maps
    a payment id to the author of its record, as the fold reports it; given,
    only a record the payer wrote withholds anything.
    """
    contested_ids = set(contested_ids)
    pay_anyway = set(pay_anyway)
    mine = [s for s in plan if s["from"] == payer]

    # Section 10.5: only a confirmed payment moves a balance, so a debt this
    # payer has already paid is still in the plan. Records to one id sum.
    confirmed = set(bill.get("confirmedPayments") or ())
    pending = {}
    for p in bill.get("payments") or ():
        if p["from"] != payer or p["id"] in confirmed:
            continue
        if recorded_by is not None and recorded_by.get(p["id"]) != payer:
            continue
        pending[p["to"]] = _in_range(pending.get(p["to"], 0) + p["amount"])

    pay_to = {p["id"]: p.get("payTo") for p in bill.get("participants") or ()}

    carried, awaiting, contested = [], [], []
    for s in mine:
        # The payee, and every creditor whose debt this settlement covers
        # (section 6.3): netting can reroute a debt already paid onto
        # somebody else.
        owed_to = {s["to"]} | {c["to"] for c in s.get("covers") or ()}
        in_flight = [pending[t] for t in sorted(owed_to) if t in pending]
        if in_flight:
            awaiting.append({"to": s["to"], "owed": s["amount"],
                             "paid": _checked_sum(in_flight)})
        elif s["to"] in contested_ids and s["to"] not in pay_anyway:
            contested.append({"to": s["to"], "amount": s["amount"],
                              "address": pay_to.get(s["to"])})
        else:
            carried.append(s)
    return {"carried": carried, "awaiting": awaiting, "contested": contested}


def render_obligation(settlements, participants, rate, currency,
                      skip_unpayable=False, include_fiat=False):
    """Section 8.5. One payer's whole obligation as a payment request.

    Reports three groups: the outputs the URI carries, the recipients it
    cannot carry and why, and the total each accounts for. A URI that silently
    covers three of a payer's four debts is indistinguishable, to the payer who
    sends it, from one that settles all four.
    """
    by_id = {p["id"]: p for p in participants}
    payments, unpayable = [], []
    carried = withheld = 0

    for s in settlements:
        who = by_id.get(s["to"])
        if who is None:
            # A merge or storage fault, needing a different remedy from a
            # missing address.
            raise Refused("unknown_participant")
        address = payable_address(who)
        if address is None:
            payouts = who.get("payouts") or []
            if published_address(who):
                reason, code = "bad_address", "zip321_bad_address"
            elif payouts:
                reason, code = "payout_not_zec", "zip321_no_address"
            else:
                reason, code = "no_address", "zip321_no_address"
            if not skip_unpayable:
                raise Refused(code)
            unpayable.append({"id": s["to"], "reason": reason,
                              "minorUnits": s["amount"]})
            withheld = _exact_i64(withheld + s["amount"])
            continue
        payments.append({
            "address": address,
            "zatoshi": fiat_to_zatoshi(s["amount"], rate, currency, "up"),
            "fiat": (currency, s["amount"]),
            "label": who.get("name"),
        })
        carried = _exact_i64(carried + s["amount"])

    uri = render_uri(payments, include_fiat) if payments else None
    return {
        "uri": uri,
        "payments": [{"address": p["address"], "zatoshi": p["zatoshi"]}
                     for p in payments],
        "unpayable": unpayable,
        "carriedMinorUnits": carried,
        "withheldMinorUnits": withheld,
        "isComplete": not unpayable,
    }


def stand_in(verifies):
    """The vectors' stand-in for the host's curve operation.

    An item names an entry id, and every copy of that entry verifies; or an id
    and a signature joined by `|`, and only that copy does. The key is not
    consulted: a case states which copies verify against the key the fold asks
    about.
    """
    ok = set(verifies)

    def verify(entry, key):
        del key
        if entry.get("id") in ok:
            return True
        sig = entry.get("sig")
        return isinstance(sig, str) and f"{entry.get('id')}|{sig}" in ok
    return verify

def non_canonical(text):
    """`text` with the lowest unused bit of its last character set.

    Decodes to the same bytes under a lenient decoder; section 9.4 refuses
    it. Only a text whose byte count is not a multiple of three has such a
    bit.
    """
    alphabet = ("ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz"
                "0123456789-_")
    assert len(text) % 4 in (2, 3), "no unused bits to set"
    last = alphabet.index(text[-1])
    assert last & 1 == 0, "already non-canonical"
    return text[:-1] + alphabet[last | 1]
