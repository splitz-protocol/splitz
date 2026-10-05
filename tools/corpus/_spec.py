"""A reference for the computational sections of SPEC.md, written from the
specification text.

It exists to generate vectors. It is not one of the shipped implementations,
and no shipped implementation may be used to produce an expectation: a corpus
derived from an implementation cannot contain a case that implementation is
self-consistently wrong about.
"""

import re

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

# Section 2.2: the largest magnitude one expense may carry, the largest amount
# section 7.1 can price: I64_MAX // 100000000.
MAX_ENTRY_AMOUNT = 92_233_720_368


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
    # Step 4: products are exact (Python integers are unbounded).
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


def _ids(value):
    """An id list as written, or none when it is not a list."""
    return value if isinstance(value, list) else []


def _int_map(value):
    """A map of id to integer, or none when it is not a map. Every value is
    an integer (bill_type_error), checked before any is compared."""
    if not isinstance(value, dict):
        return {}
    return {k: _int(v) for k, v in value.items()}


def split(total, spec):
    """Section 4. Returns {id: owed minor units}."""
    _check_id_lists(spec)
    kind = spec.get("type")

    if kind == "equal":
        among = _sorted_ids(_ids(spec.get("among")))
        if not among:
            raise Refused("empty_split")
        return dict(zip(among, allocate_evenly(total, len(among))))

    if kind == "exact":
        amounts = _int_map(spec.get("amounts"))
        ids = _sorted_ids(amounts)
        if any(_against(amounts[i], total) for i in ids):
            raise Refused("negative_share")
        if _checked_sum([amounts[i] for i in ids]) != total:
            raise Refused("exact_total_mismatch")
        return {i: amounts[i] for i in ids}

    if kind == "percentage":
        bp = _int_map(spec.get("basisPoints"))
        ids = _sorted_ids(bp)
        if any(bp[i] < 0 for i in ids):
            raise Refused("negative_weight")
        # The overflow check precedes the full-scale check (section 4.3).
        if _checked_sum([bp[i] for i in ids]) != 10000:
            raise Refused("percentage_not_full_scale")
        return dict(zip(ids, allocate(total, [bp[i] for i in ids])))

    if kind == "shares":
        counts = _int_map(spec.get("shareCounts"))
        ids = _sorted_ids(counts)
        return dict(zip(ids, allocate(total, [counts[i] for i in ids])))

    if kind == "itemized":
        items = _ids(spec.get("items"))
        if not items:
            raise Refused("itemized_no_items")
        # An item is an object (section 4.5). Typed before it is indexed:
        # every check below reads a member of it.
        if any(not isinstance(it, dict) for it in items):
            raise Refused("bill_type_error")
        if any(not _ids(it.get("sharedBy")) for it in items):
            raise Refused("itemized_unassigned_item")
        extra = spec.get("extraMinorUnits")
        extra = 0 if extra is None else _int(extra)
        if _against(extra, total):
            raise Refused("negative_share")
        if any(_against(_int(it.get("minorUnits")), total) for it in items):
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
        # Section 8.3: a memo goes only to an address that decodes (8.6) and
        # can receive one.
        if p.get("memo") is not None and not parse_address(addr)["canReceiveMemo"]:
            raise Refused("zip321_memo_undeliverable")

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



# --- Section 8.7: reading a request back -----------------------------------

REQUEST_PARAMS = {"address", "amount", "fiat", "memo", "label", "message"}


def _not_canonical():
    raise Refused("zip321_not_canonical")


def _read_amount(text):
    m = re.fullmatch(r"([0-9]{1,8})(?:\.([0-9]{1,8}))?", text)
    if not m:
        _not_canonical()
    return int(m.group(1)) * ZAT_PER_ZEC + int((m.group(2) or "").ljust(8, "0"))


def _read_fiat(text):
    m = re.fullmatch(r"([A-Z]{3}):([0-9]{1,18})", text)
    if not m:
        _not_canonical()
    return (m.group(1), int(m.group(2)))


def _read_memo(text):
    import base64
    if not re.fullmatch(r"[A-Za-z0-9_-]*", text) or len(text) % 4 == 1:
        _not_canonical()
    out = base64.urlsafe_b64decode(text + "=" * (-len(text) % 4))
    # Section 8.7: the bits end on a byte. A decoder that drops stray bits
    # reads a memo that is not canonical, and a later check then refuses the
    # request with some other code than the one every reader gives.
    if base64.urlsafe_b64encode(out).decode().rstrip("=") != text:
        _not_canonical()
    return out


def _unqchar(text):
    out = bytearray()
    i = 0
    while i < len(text):
        c = text[i]
        if c == "%":
            if i + 2 >= len(text):
                _not_canonical()
            pair = text[i + 1:i + 3]
            if not re.fullmatch(r"[0-9A-Fa-f]{2}", pair):
                _not_canonical()
            out.append(int(pair, 16))
            i += 3
        elif ord(c) < 0x80:
            out.append(ord(c))
            i += 1
        else:
            _not_canonical()
    try:
        return out.decode("utf-8")
    except UnicodeDecodeError:
        _not_canonical()


def read_request(uri):
    """Section 8.7. Reads exactly what render_uri writes, and nothing else."""
    if not uri.startswith("zcash:"):
        _not_canonical()
    rest = uri[len("zcash:"):]
    q = rest.find("?")
    if q < 0:
        _not_canonical()
    path_address = rest[:q]
    by_index = {}
    for part in rest[q + 1:].split("&"):
        eq = part.find("=")
        if eq <= 0:
            _not_canonical()
        key, value = part[:eq], part[eq + 1:]
        dot = key.find(".")
        name = key if dot < 0 else key[:dot]
        index = 0
        if dot >= 0:
            digits = key[dot + 1:]
            if not re.fullmatch(r"[1-9][0-9]{0,3}", digits):
                _not_canonical()
            index = int(digits)
        if name not in REQUEST_PARAMS:
            _not_canonical()
        params = by_index.setdefault(index, {})
        if name in params:
            _not_canonical()
        params[name] = value

    payments = []
    for i in range(len(by_index)):
        p = by_index.get(i)
        if p is None:
            _not_canonical()
        if i == 0 and path_address:
            if "address" in p:
                _not_canonical()
            address = path_address
        else:
            if "address" not in p:
                _not_canonical()
            address = p["address"]
        if "amount" not in p:
            _not_canonical()
        payment = {"address": address, "zatoshi": _read_amount(p["amount"])}
        if "fiat" in p:
            payment["fiat"] = _read_fiat(p["fiat"])
        if "memo" in p:
            payment["memo"] = _read_memo(p["memo"])
        if "label" in p:
            payment["label"] = _unqchar(p["label"])
        if "message" in p:
            payment["message"] = _unqchar(p["message"])
        payments.append(payment)

    rendered = render_uri(payments, include_fiat=any("fiat" in p for p in payments))
    if rendered != uri:
        _not_canonical()
    return payments


def check_proposal(uri, outputs):
    """Section 14.6. Each requested payment matched to one equal output."""
    pool = list(outputs)
    missing = []
    for p in read_request(uri):
        for j, o in enumerate(pool):
            if o is not None and o["address"] == p["address"] and o["zatoshi"] == p["zatoshi"]:
                pool[j] = None
                break
        else:
            missing.append(p)
    return missing, [o for o in pool if o is not None]
# --- Section 8.6: Zcash addresses -------------------------------------------

BECH32_CHARSET = "qpzry9x8gf2tvdw0s3jn54khce6mua7l"
BECH32_CONST = 1
BECH32M_CONST = 0x2BC830A3

# Bech32 (ZIP 173) for Sapling; Bech32m (BIP 350) for TEX (ZIP 320) and
# Unified (ZIP 316, revision 0).
SAPLING_HRPS = {"zs": "main", "ztestsapling": "test", "zregtestsapling": "regtest"}
TEX_HRPS = {"tex": "main", "textest": "test", "texregtest": "regtest"}
UNIFIED_HRPS = {"u": "main", "utest": "test", "uregtest": "regtest"}

# Base58Check lead bytes. Regtest uses testnet's, so these answer "test".
TRANSPARENT_PREFIXES = {
    (0x1C, 0xB8): ("main", "p2pkh"),
    (0x1C, 0xBD): ("main", "p2sh"),
    (0x1D, 0x25): ("test", "p2pkh"),
    (0x1C, 0xBA): ("test", "p2sh"),
}
BASE58_ALPHABET = "123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz"
BASE58CHECK_BYTES = 26         # two lead bytes, a 20-byte hash, a checksum

TYPECODE_P2PKH, TYPECODE_P2SH, TYPECODE_SAPLING, TYPECODE_ORCHARD = 0, 1, 2, 3
RECEIVER_LENGTHS = {0: 20, 1: 20, 2: 43, 3: 43}
MAX_COMPACT_SIZE = 0x2000000
UA_PADDING = 16
F4_MIN, F4_MAX = 48, 4194368


def _bech32_polymod(values):
    gen = [0x3B6A57B2, 0x26508E6D, 0x1EA119FA, 0x3D4233DD, 0x2A1462B3]
    chk = 1
    for v in values:
        top = chk >> 25
        chk = ((chk & 0x1FFFFFF) << 5) ^ v
        for i in range(5):
            if (top >> i) & 1:
                chk ^= gen[i]
    return chk


def _bech32_decode(text, hrps, constant):
    """(network, bytes) for a string under one of `hrps`, or None.

    Lower case only, since the prefixes and the alphabet are compared as
    written: ZIP 173 has encoders write lower case, and the decoder wallets
    use (zcash_address 0.13) refuses upper. The 5-bit groups regroup into
    bytes and the leftover bits number at most four and are zero (ZIP 173,
    "Decoding").
    """
    sep = text.rfind("1")
    if sep < 1:
        return None
    hrp, data = text[:sep], text[sep + 1:]
    if hrp not in hrps or len(data) < 6:
        return None
    if any(c not in BECH32_CHARSET for c in data):
        return None
    values = [BECH32_CHARSET.index(c) for c in data]
    expanded = [ord(c) >> 5 for c in hrp] + [0] + [ord(c) & 31 for c in hrp]
    if _bech32_polymod(expanded + values) != constant:
        return None
    acc = bits = 0
    out = bytearray()
    for v in values[:-6]:
        acc = (acc << 5) | v
        bits += 5
        if bits >= 8:
            bits -= 8
            out.append((acc >> bits) & 0xFF)
    if bits > 4 or (acc & ((1 << bits) - 1)) != 0:
        return None
    return hrp, bytes(out)


def _base58check(text):
    import hashlib
    n = 0
    for c in text:
        i = BASE58_ALPHABET.find(c)
        if i < 0:
            return None
        n = n * 58 + i
        if n >> (8 * BASE58CHECK_BYTES):
            # Lead, payload and checksum are 26 bytes; stopping here keeps
            # the work linear in the length of the text.
            return None
    body = n.to_bytes((n.bit_length() + 7) // 8, "big") if n else b""
    zeros = len(text) - len(text.lstrip("1"))
    raw = b"\x00" * zeros + body
    if len(raw) < 4:
        return None
    payload, check = raw[:-4], raw[-4:]
    if hashlib.sha256(hashlib.sha256(payload).digest()).digest()[:4] != check:
        return None
    return payload


def _f4jumble_inv(m):
    """ZIP 316, "Jumbling": the inverse of the 4-round Feistel over BLAKE2b."""
    import hashlib
    left_len = min(64, len(m) // 2)
    a, b = bytearray(m[:left_len]), bytearray(m[left_len:])

    def h(i, u):
        return hashlib.blake2b(bytes(u), digest_size=left_len,
                               person=b"UA_F4Jumble_H" + bytes([i, 0, 0])).digest()

    def g(i, u):
        out = b""
        for j in range((len(b) + 63) // 64):
            out += hashlib.blake2b(bytes(u), digest_size=64,
                                   person=b"UA_F4Jumble_G" + bytes([i, j & 0xFF, j >> 8])).digest()
        return out[:len(b)]

    def xor(x, y):
        return bytearray(p ^ q for p, q in zip(x, y))

    # c = a, d = b on entry: y = c ^ H1(d); x = d ^ G1(y); a = y ^ H0(x); b = x ^ G0(a)
    y = xor(a, h(1, b))
    x = xor(b, g(1, y))
    a2 = xor(y, h(0, x))
    b2 = xor(x, g(0, a2))
    return bytes(a2 + b2)


def _compact_size(raw, at):
    """(value, next) for a canonical compactSize at `at`, or None."""
    if at >= len(raw):
        return None
    flag = raw[at]
    if flag < 253:
        value, width = flag, 1
    else:
        size = {253: 2, 254: 4, 255: 8}[flag]
        if at + 1 + size > len(raw):
            return None
        value = int.from_bytes(raw[at + 1:at + 1 + size], "little")
        if value < {253: 253, 254: 0x10000, 255: 0x100000000}[flag]:
            return None
        width = 1 + size
    if value > MAX_COMPACT_SIZE:
        return None
    return value, at + width


def _unified_receivers(hrp, raw):
    """The typecodes of a revision 0 Unified Address, in encoding order."""
    if not F4_MIN <= len(raw) <= F4_MAX:
        return None
    plain = _f4jumble_inv(raw)
    padding = hrp.encode("ascii").ljust(UA_PADDING, b"\x00")
    if plain[-UA_PADDING:] != padding:
        return None
    body = plain[:-UA_PADDING]
    at, codes = 0, []
    while at < len(body):
        tc = _compact_size(body, at)
        if tc is None:
            return None
        typecode, at = tc
        ln = _compact_size(body, at)
        if ln is None:
            return None
        length, at = ln
        if at + length > len(body):
            return None
        if RECEIVER_LENGTHS.get(typecode, length) != length:
            return None
        at += length
        codes.append(typecode)
    # Ascending, so a repeat or a reordering is refused by one comparison.
    if any(b <= a for a, b in zip(codes, codes[1:])):
        return None
    if TYPECODE_P2PKH in codes and TYPECODE_P2SH in codes:
        return None
    if any(0xE0 <= c <= 0xFC for c in codes):
        return None
    if TYPECODE_SAPLING not in codes and TYPECODE_ORCHARD not in codes:
        return None
    return codes


def parse_address(text):
    """Section 8.6. What a Zcash address is, or `address_invalid`."""
    if not isinstance(text, str):
        raise Refused("address_invalid")

    got = _bech32_decode(text, UNIFIED_HRPS, BECH32M_CONST)
    if got is not None:
        hrp, raw = got
        codes = _unified_receivers(hrp, raw)
        if codes is None:
            raise Refused("address_invalid")
        return {"network": UNIFIED_HRPS[hrp], "kind": "unified",
                "receivers": codes,
                "canReceiveMemo": TYPECODE_SAPLING in codes or TYPECODE_ORCHARD in codes}

    got = _bech32_decode(text, SAPLING_HRPS, BECH32_CONST)
    if got is not None:
        hrp, raw = got
        if len(raw) != 43:
            raise Refused("address_invalid")
        return {"network": SAPLING_HRPS[hrp], "kind": "sapling",
                "receivers": [], "canReceiveMemo": True}

    got = _bech32_decode(text, TEX_HRPS, BECH32M_CONST)
    if got is not None:
        hrp, raw = got
        if len(raw) != 20:
            raise Refused("address_invalid")
        return {"network": TEX_HRPS[hrp], "kind": "tex",
                "receivers": [], "canReceiveMemo": False}

    payload = _base58check(text)
    if payload is None or len(payload) != 22:
        raise Refused("address_invalid")
    found = TRANSPARENT_PREFIXES.get((payload[0], payload[1]))
    if found is None:
        raise Refused("address_invalid")
    network, kind = found
    return {"network": network, "kind": kind, "receivers": [],
            "canReceiveMemo": False}


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
        # '%' then exactly two hexadecimal digits; `int` alone accepts a sign
        # and surrounding space.
        pair = raw[i + 1:i + 3]
        if raw[i] == 0x25 and len(pair) == 2 and all(b in b"0123456789abcdefABCDEF" for b in pair):
            out.append(int(pair.decode("ascii"), 16))
            i += 3
            continue
        out.append(raw[i])
        i += 1
    return out.decode("utf-8", "replace")


def _b64url_decodes(text):
    """Section 11.1. `text` decodes as unpadded base64url: the alphabet, a
    length that is not 1 more than a multiple of 4, and no unused bit set in
    its last character, so it is the canonical encoding of its bytes."""
    import base64
    if not text or any(c not in B64URL_ALPHABET for c in text):
        return False
    if len(text) % 4 == 1:
        return False
    raw = base64.urlsafe_b64decode(text + "=" * (-len(text) % 4))
    return b64url(raw) == text


def parse_invite(text):
    """Section 11.1. An exact grammar, never a general URI library."""
    s = strip_scan_padding(text)

    # An https link carries the invite as its fragment, whole.
    if s.startswith(INVITE_LINK_SCHEME):
        hash_at = s.find("#")
        if hash_at < 0:
            raise Refused("invite_not_an_invite")
        s = s[hash_at + 1:]

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
    # Section 11.1: the 32 bytes of a bill key (section 11.3).
    if not _b64url_len(key, KEY_BYTES):
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


INVITE_EXTRA = set("!*'()")
INVITE_LINK_SCHEME = "https://"


def render_invite_link(base, bill_id, key, name="", expiry=None):
    """Section 11.1. `base`, a `#`, and the invite URI.

    `base` is `https://`, then at least one character that is not `/`, and
    nothing outside printable ASCII, no space and no `#`.
    """
    rest = base[len(INVITE_LINK_SCHEME):] if base.startswith(INVITE_LINK_SCHEME) else None
    if (rest is None or not rest or rest.startswith("/")
            or any(not ("!" <= c <= "~") or c == "#" for c in rest)):
        raise Refused("invite_bad_link")
    return f"{base}#{render_invite(bill_id, key, name, expiry)}"


def invite_escape(text):
    """Section 11.1. Every byte outside the unreserved set plus `!*'()`, as an
    upper-case escape. Not ZIP 321's qchar, which leaves more literal."""
    out = []
    for byte in text.encode("utf-8"):
        ch = chr(byte)
        if ch in UNRESERVED or ch in INVITE_EXTRA:
            out.append(ch)
        else:
            out.append(f"%{byte:02X}")
    return "".join(out)


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
    if not key or not _b64url_len(key, KEY_BYTES):
        raise Refused("invite_missing_key")
    parts = [f"v={INVITE_VERSION}", f"b={invite_escape(bill_id)}",
             f"k={invite_escape(key)}"]
    if name:
        parts.append(f"n={invite_escape(name)}")
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

PAYLOAD_CAP = 2322          # 2331, a version-40 QR code in byte mode at EC level M, less "splitzd1:"

# Sections 10.1 and 11.2. Stated here rather than inherited from a JSON
# library: one
# reader's parser gives up at its own depth and another does not, and the cap
# is no defence because a level of nesting costs two bytes. The deepest a
# conforming document reaches is the sharedBy array inside an itemised split,
# at eight.
MAX_DOCUMENT_DEPTH = 64
# Section 10.1: an entry sits two levels inside the payload that carries it
# (the body and its `log`), so its own bound is two less.
MAX_ENTRY_DEPTH = MAX_DOCUMENT_DEPTH - 2


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


def _strict_json(text):
    """RFC 8259 JSON: no NaN or Infinity literal, and no number a double
    cannot hold (`1e400`), each refused rather than read as infinite."""
    import json as _json
    import math

    def constant(name):
        raise ValueError(f"not JSON: {name}")

    def number(text):
        value = float(text)
        if not math.isfinite(value):
            raise ValueError(f"out of range: {text}")
        return value

    def integer(text):
        # An integer literal is held as one when it can be, but one past what
        # a double holds is refused like `1e400`: no reader holds it.
        if not math.isfinite(float(text)):
            raise ValueError(f"out of range: {text}")
        return int(text)

    return _json.loads(text, parse_constant=constant, parse_float=number,
                       parse_int=integer)



BILL_KEY_DIGEST_DOMAIN = "splitz-bill-key-v1"


def bill_key_digest(key):
    """Section 9.4: what a createBill states as keyDigest for `key`, or None."""
    import base64
    import hashlib
    if not _b64url_len(key, KEY_BYTES):
        return None
    raw = base64.urlsafe_b64decode(key + "=" * (-len(key) % 4))
    digest = hashlib.sha256(BILL_KEY_DIGEST_DOMAIN.encode() + raw).digest()
    return base64.urlsafe_b64encode(digest).decode().rstrip("=")


def read_scan(text):
    """What a scanned bill code opens as, by section 9.4's key check.

    Returns {"kind": "bill", "entryCount": n} for a bill code whose invite
    key is the one its bill was made with, or whose create commits to none;
    raises Refused("invite_key_mismatch") for one carrying another key. The
    bill's own create is the one whose id derives from it.
    """
    payload = decode_payload(text)
    invite = payload.get("invite")
    entries = [e for e in payload["log"] if isinstance(e, dict)]
    if isinstance(invite, dict):
        b, k = invite.get("b"), invite.get("k")
        # Section 11.1 decides whether it is an invite at all: one it refuses
        # (a key that is not its bytes' canonical encoding, say) carries no
        # key, and the bill is read without one.
        try:
            parse_invite(render_invite(b, k))
        except (Refused, TypeError):
            b = k = None
        if isinstance(b, str) and isinstance(k, str):
            for e in entries:
                # Only the bill's own create, whose id derives, speaks for
                # its key: anybody holding it can write one stating the id.
                if e.get("kind") == "createBill" and e.get("id") == b \
                        and derive_bill_id(e) == b \
                        and isinstance(e.get("keyDigest"), str) \
                        and e["keyDigest"] != bill_key_digest(k):
                    raise Refused("invite_key_mismatch")
    return {"kind": "bill", "entryCount": len(entries)}

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
        body = _strict_json(raw.decode("utf-8"))
    except Exception:
        raise Refused("payload_damaged")
    if not isinstance(body, dict):
        raise Refused("payload_damaged")
    # Section 2.3 over the whole body, before any entry is read: a string
    # that is not Unicode scalar values has no UTF-8 encoding, and a document
    # carrying one is damaged as a whole, as a strict JSON reader finds it.
    try:
        _check_scalar_values(body)
    except Refused:
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
    r"([0-9]{4})-([0-9]{2})-([0-9]{2})[Tt]([0-9]{2}):([0-9]{2}):([0-9]{2})(\.[0-9]+)?[Zz]"
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
    # Whole-text match: `$` also matches before a final newline.
    m = _INSTANT.fullmatch(text)
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
    # Section 9.3: a number's code follows from its value. One past the 64-bit
    # range, or one no double holds, is amount_overflow however it was
    # written; any other non-integer is canonical_json_float.
    if isinstance(value, bool):
        raise Refused(code)
    if isinstance(value, float):
        if value != value or abs(value) >= 2**63:
            raise Refused("amount_overflow")
        raise Refused("canonical_json_float")
    if not isinstance(value, int):
        raise Refused(code)
    if abs(value) >= 2**63:
        # Section 2.2's range is symmetric.
        raise Refused("amount_overflow")
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


def _check_numbers(value):
    """Sections 2.2 and 9.3. Every number in an entry is an integer a signed
    64-bit value holds.

    One outside that range is refused with `amount_overflow` whether it was
    written as an integer or not: a reader whose parser holds large integers as
    doubles cannot tell `9223372036854775808` from `9.223372036854775808e18`,
    so the code has to follow from the value. Any other non-integer is
    `canonical_json_float`, which is what section 9.3's encoding refuses.
    """
    import math
    if isinstance(value, bool):
        return
    if isinstance(value, int):
        if not I64_MIN <= value <= I64_MAX:
            raise Refused("amount_overflow")
    elif isinstance(value, float):
        if not math.isfinite(value) or abs(value) >= 2.0 ** 63:
            raise Refused("amount_overflow")
        raise Refused("canonical_json_float")
    elif isinstance(value, dict):
        for v in value.values():
            _check_numbers(v)
    elif isinstance(value, list):
        for v in value:
            _check_numbers(v)


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
    # Section 9.1. An empty id is not a name anyone can be settled to, and
    # one holding `:` mints no expense or payment id (section 10.3).
    if not pid or ":" in pid:
        raise Refused("bill_bad_participant_id")
    p = {"id": pid, "name": _str(raw.get("name", ""))}
    if "payTo" in raw:
        p["payTo"] = _str(raw["payTo"])
    if "identityKey" in raw:
        # Section 10.7. A key a participant id is derived from, so it is one:
        # 32 bytes, canonical unpadded base64url.
        if not _b64url_len(_str(raw["identityKey"]), KEY_BYTES):
            raise Refused("bill_type_error")
        p["identityKey"] = raw["identityKey"]
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
    if abs(e["amount"]) > MAX_ENTRY_AMOUNT:
        raise Refused("amount_too_large")
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
        # A rate, decoded as one: section 7's members, each checked.
        decode_rate(r)
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

    def listed(member):
        # Absent or null is none; anything else that is not a list is refused
        # rather than read as empty.
        value = doc.get(member)
        if value is None:
            return []
        if not isinstance(value, list):
            raise Refused("bill_type_error")
        return value

    participants = []
    seen = set()
    for raw in listed("participants"):
        p = decode_participant(raw)
        if p["id"] in seen:
            raise Refused("duplicate_participant")
        seen.add(p["id"])
        participants.append(p)

    expenses = [decode_expense(raw, currency, seen)
                for raw in listed("expenses")]

    payments = [decode_payment(raw, currency, seen)
                for raw in listed("payments")]

    rate = None
    if "rate" in doc:
        if not isinstance(doc["rate"], dict):
            raise Refused("bill_type_error")
        decode_rate(doc["rate"])
        rate = doc["rate"]

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
    if rate is not None:
        bill["rate"] = rate
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


def _balance_in_range(value):
    """Section 2.2. A balance, or `amount_overflow` outside +-(2^63 - 1).

    Symmetric: the most negative 64-bit value has no positive counterpart, so
    a balance holding it has no magnitude section 5.1's residual or section 6
    can form.
    """
    if not -I64_MAX <= value <= I64_MAX:
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
        net[e["paidBy"]] = _balance_in_range(net[e["paidBy"]] + e["amount"])
        for pid, owed in shares.items():
            net[pid] = _balance_in_range(net[pid] - owed)
    for p in _confirmed(bill):
        net[p["from"]] = _balance_in_range(net[p["from"]] + p["amount"])
        net[p["to"]] = _balance_in_range(net[p["to"]] - p["amount"])
    # The residual is a property of the set, not of an accumulation order: a
    # running total can exceed a signed 64-bit integer at some orderings of a
    # set whose total is zero, and the order a map yields is the
    # implementation's, not the document's.
    if not _residual_is_zero(net.values()):
        raise Refused("balances_nonzero_residual")
    return net


def _by_id(ids):
    return sorted(ids, key=lambda s: s.encode("utf-8"))


def _sorted_map(m):
    return {k: m[k] for k in _by_id(m)}


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


PAYMENT_DIGEST_DOMAIN = "splitz-payment-v1"


def payment_digest(payment):
    """Section 10.5. What a confirmation binds: the digest of the payment
    payload its record carries, as written, less the `id` the confirmation
    names beside it."""
    return _derive_id(PAYMENT_DIGEST_DOMAIN, payment)


PARTICIPANT_ID_DOMAIN = "splitz-participant-v1"


def participant_id(key):
    """Section 10.7. The participant id a key speaks as:
    base64url( SHA-256( "splitz-participant-v1" || key bytes )[0..16] ).

    None for a text that is not a canonical 32-byte key. Two keys cannot
    derive one id, so no second key can claim a participant this binds.
    """
    import base64, hashlib
    if not _b64url_len(key, KEY_BYTES):
        return None
    raw = base64.urlsafe_b64decode(key + "=" * (-len(key) % 4))
    digest = hashlib.sha256(
        PARTICIPANT_ID_DOMAIN.encode("utf-8") + raw).digest()[:16]
    return base64.urlsafe_b64encode(digest).decode("ascii").rstrip("=")


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
            for member in ("targetId", "basis"):
                if member in e and e[member] in remap:
                    e[member] = remap[e[member]]
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
    if not _depth(entry, MAX_ENTRY_DEPTH):
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
        if isinstance(v, bool) or not isinstance(v, int) or not 1 <= v <= I64_MAX:
            raise Refused("bill_type_error")
    # Section 2.3, at entry ingress: the section 10.2 order depends on it.
    _check_scalar_values(entry)
    # Section 2.2 and 9.3, at entry ingress, before any member is read.
    _check_numbers(entry)

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
    if "basis" in entry and not isinstance(entry["basis"], str):
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
        # Section 10.1's order among a create's members: name, currency,
        # split mode, then creatorKey and nonce, then the id. The fold copies
        # the first three into the bill document without re-reading them, so
        # they are decided here rather than at decode, where the whole bill
        # would be unopenable instead of this entry refused.
        if "name" in entry and not isinstance(entry["name"], str):
            raise Refused("bill_type_error")
        check_currency(entry.get("currency"))
        mode = entry.get("splitMode", "equal")
        if not isinstance(mode, str):
            raise Refused("bill_type_error")
        if mode not in SPLIT_MODES:
            raise Refused("bill_unknown_split_mode")
        # Section 9.4: both fields, or the entry is unbound.
        if not _b64url_len(entry.get("creatorKey", ""), KEY_BYTES) \
                or not _b64url_len(entry.get("nonce", ""), NONCE_LEN):
            raise Refused("create_unbound")
        # Section 9.4: the digest of the bill key it was made with, when stated.
        if "keyDigest" in entry and not _b64url_len(entry["keyDigest"], KEY_BYTES):
            raise Refused("bill_type_error")
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
    """Section 10.2. The instant `at` names, then `at` as written, then
    author, then id, then canonical encoding.

    By the instant rather than the text: section 9.3 admits `t` and `z` and
    any number of fractional digits, and the text of an earlier instant can
    sort after a later one. The text then breaks ties between spellings of one
    instant. The order is total: a key that ties for two entries that are not
    equal leaves them to the host's sort, and a sort stable in one language
    and not in another then gives one input two different bill documents.
    """
    return sorted(entries, key=lambda e: (parse_instant(e["at"]).encode("utf-8"),
                                          e["at"].encode("utf-8"),
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



def owns_id(author, value):
    """SPEC.md 10.3 step 5: whether `value` is an id its `author` minted.

    The author's participant id, `:`, then anything. Every expense and payment
    id is minted by its entry's author, so exactly one author can write under
    a given id. An author whose id holds `:` mints nothing.
    """
    return (isinstance(author, str) and ":" not in author
            and isinstance(value, str) and value.startswith(author + ":"))

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
    bound = {} if verify is None else resolve_identities(copies, create, verify)

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
                key = bound.get(author)
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
        elif kind == "setRate":
            # A rate prices every request on the bill, and the latest by `at`
            # decides, so one dated far ahead outranks every later correction
            # its author does not withdraw.
            allowed = {target["author"], creator}
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

    # Restating an expense, section 10.8. An `addExpense` carrying `targetId`
    # replaces that expense: one restatement of a target applies, the first by
    # section 10.2's order among those allowed, and only while the target is
    # on the bill as the restatement read it -- the amendment it names as
    # `basis` is still the one applied to the target. A restatement that does
    # not apply is set aside and its target stays.
    restating = {}
    for e in entries:
        if e["kind"] == "addExpense" and "targetId" in e:
            restating.setdefault(e["targetId"], []).append(e)
    restatement_ids = {e["id"] for group in restating.values() for e in group}
    winner = {}

    def decide(target_id):
        target = by_id.get(target_id)
        on_bill = (target is not None and target_id not in voided
                   and (target_id not in restatement_ids
                        or winner.get(target["targetId"]) is target))
        current = amendments.get(target_id)
        basis = current["id"] if current is not None else None
        chosen = None
        for r in restating.get(target_id, []):
            if r["id"] in voided:
                continue
            if target is None:
                aside(r, "unknown_entry", "restates an entry the log lacks")
            elif target["kind"] != "addExpense":
                aside(r, "amend_kind_mismatch", "restates what is no expense")
            elif r["author"] not in (target["author"], creator):
                aside(r, "unauthorized_entry", "may not withdraw its target")
            elif not on_bill or r.get("basis") != basis:
                aside(r, "restatement_stale", "its target changed under it")
            elif chosen is not None:
                aside(r, "restatement_superseded", "another restated it first")
            else:
                chosen = r
        winner[target_id] = chosen

    for start in restating:
        # A restatement's target may be a restatement itself, decided first.
        # Ids are digests of their entries, so the chain ends.
        chain = [start]
        while True:
            t = by_id.get(chain[-1])
            if (t is None or chain[-1] not in restatement_ids
                    or t["targetId"] in winner or t["targetId"] in chain):
                break
            chain.append(t["targetId"])
        for target_id in reversed(chain):
            if target_id not in winner:
                decide(target_id)
    unapplied = restatement_ids - {
        r["id"] for r in winner.values() if r is not None}
    for target_id, r in winner.items():
        if r is not None:
            voided.add(target_id)

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
                     if o["id"] not in voided and o["kind"] != "voidEntry"
                     and o["id"] not in unapplied]
        named = False
        for other in surviving:
            # The amendment and the entry it corrects are both read: the
            # amendment may yet be set aside when it is applied, and the entry
            # then applies as written.
            for eff in (amendments.get(other["id"], other), other):
                if other["kind"] == "addExpense":
                    # Total readers, not indexing: this pass runs before the
                    # expense is decoded, so `split` and everything under it
                    # is whatever a peer wrote. Section 10.1 types the payload
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

    live = [e for e in entries if e["id"] not in voided
            and e["kind"] != "voidEntry" and e["id"] not in unapplied]

    def applied(e, attempt):
        """Section 10.4. `attempt` applied to the entry as amended, then as
        written. An amendment that cannot be applied is set aside and the
        entry it corrects applies as written: correcting an entry into one
        that cannot be applied does not take the entry off the bill.

        Returns (result, version applied), or None when neither applies.
        """
        amendment = amendments.get(e["id"])
        if amendment is not None:
            try:
                return attempt(amendment), amendment
            except Refused as r:
                aside(amendment, r.code, "an amendment that cannot be applied")
        try:
            return attempt(e), e
        except Refused as r:
            aside(e, r.code, "an entry that cannot be applied")
            return None

    # Participants, in a pass of their own.
    participants, replaced = {}, []
    for e in live:
        if e["kind"] != "joinBill":
            continue

        def participant(version, e=e):
            p = _as_dict(version.get("participant"))
            pid = p.get("id")
            if pid is None or pid == "":
                raise Refused("bill_missing_entry_payload")
            if pid in bound and e["author"] != pid:
                # Section 10.7. A bound participant's record is theirs to
                # create as well as to change.
                raise Refused("unauthorized_entry")
            if pid in participants and e["author"] != pid:
                raise Refused("unauthorized_entry")
            # The decoder decides what a participant is, here rather than
            # once the document is assembled: a member it would refuse sets
            # this entry aside (section 10.3) instead of making the whole
            # bill undecodable.
            decoded = decode_participant(p)
            # Section 10.7. A record stating a key names the participant that
            # key derives. Any other id would let a second key speak for
            # somebody a first one already binds.
            if ("identityKey" in decoded and pid != creator
                    and participant_id(decoded["identityKey"]) != pid):
                raise Refused("participant_id_not_derived")
            return p

        result = applied(e, participant)
        if result is None:
            continue
        p, version = result
        pid = p["id"]
        # Section 10.3 step 4. The destination this record replaces: the one
        # held for the participant, or, for the first record, the one the
        # join was written with before an amendment changed it. A destination
        # the decoder never read is taken as none.
        if pid in participants:
            before = _destination(participants[pid])
        else:
            before = _destination(_as_dict(e.get("participant")))
        if before != _destination(p):
            replaced.append({"id": pid, "from": before, "to": _destination(p)})
        participants[pid] = p

    # Section 10.1. The latest live setRate by a participant decides, by
    # section 10.2's order, so the answer is a function of the log and not of
    # which device last spoke. Decided after the participants, because only
    # they may set it.
    rate = rate_entry = rate_author = None
    for e in live:
        if e["kind"] != "setRate":
            continue

        def set_rate(version, e=e):
            if e["author"] not in participants:
                raise Refused("unknown_participant")
            # Section 10.7. A rate prices every request on the bill, so a
            # fold that verifies takes it only from a participant whose key
            # it has bound: an unsigned join is enough to put anybody holding
            # the invite on the bill.
            if verify is not None and e["author"] not in bound:
                raise Refused("unauthorized_entry")
            payload = version.get("rate")
            decode_rate(payload)
            # Section 10.1: a rate prices this bill's amounts, so it is in
            # this bill's currency. Another one would refuse every request.
            if payload["currency"] != currency:
                raise Refused("rate_currency_mismatch")
            return payload

        result = applied(e, set_rate)
        # SPEC.md 10.1: the creator's latest rate stands over anybody else's,
        # which decides only while the creator has set none.
        if result is not None and not (rate_author == creator and e["author"] != creator):
            rate = result[0]
            rate_entry, rate_author = e["id"], e["author"]

    expenses, payments = [], []
    # Who wrote each applied payment record, by its id: section 14.4 withholds
    # only for a record the payer wrote.
    payment_authors = {}
    # What each applied record says, as a confirmation binds it (10.5).
    payment_digests = {}
    # The entry that introduced each applied expense and payment, by the
    # expense's or payment's own id, and who wrote it: what an amendment or a
    # withdrawal targets, and whose entry it is. Reported so a reader takes
    # them from the fold rather than from a log the fold has set aside parts
    # of.
    expense_entries, expense_authors, payment_entries = {}, {}, {}
    # Section 5.1's balances, formed as this pass applies each entry and in
    # the order section 5.1 forms them, so a bill this fold returns always has
    # balances section 2.2 can hold. An entry whose effect would carry one out
    # of range is set aside, deterministically and in log order, rather than
    # left to make section 5 refuse the whole bill.
    running = {pid: 0 for pid in participants}
    # What each author has recorded one participant paying another. Section
    # 14.4 sums a payer's own records, so the bound is per author: a record
    # the other party wrote cannot carry the payer's out of range.
    pair_total = {}
    def _fits(v):
        # Section 2.2. A balance has a magnitude, so the range is symmetric:
        # the most negative 64-bit value has no positive counterpart.
        return -I64_MAX <= v <= I64_MAX

    for e in live:
        if e["kind"] == "addExpense":

            def expense(version, e=e):
                ex = dict(version["expense"])
                # Section 9.1 falls back only when the member is ABSENT. A
                # present value that is not a currency is an entry that cannot
                # be applied, and section 10.3 sets those aside rather than
                # raising: one such entry must not take the bill with it.
                if "currency" not in ex:
                    ex["currency"] = currency      # denominated by the fold
                elif not is_currency(ex["currency"]):
                    raise Refused("bill_bad_currency")
                elif ex["currency"] != currency:
                    raise Refused("currency_mismatch")
                if ex.get("paidBy") not in participants:
                    raise Refused("unknown_participant")
                decode_expense(ex, currency, set(participants))
                # SPEC.md 10.3 step 5: an expense id is its author's own, so
                # a copy by anybody else never competes with it by `at`.
                if not owns_id(e["author"], ex["id"]):
                    raise Refused("id_not_minted")
                # One id names one expense. An amendment or a withdrawal is
                # written against the expense a reader shows, and two under
                # one id leave it to guess which. Both are one author's, and
                # the first stands.
                if ex["id"] in expense_entries:
                    raise Refused("duplicate_expense")
                # Section 4 is what turns an expense into what each person
                # owes, and section 5 runs it downstream of this fold. An
                # expense whose split section 4 refuses cannot be applied, so
                # it is set aside here rather than raising out of `balances`
                # once the bill is already built.
                shares = split(_int(ex.get("amount")), ex.get("split"))
                moved = dict(running)
                moved[ex["paidBy"]] += ex["amount"]
                ok = _fits(moved[ex["paidBy"]])
                for pid, owed in shares.items():
                    moved[pid] -= owed
                    ok = ok and _fits(moved[pid])
                if not ok:
                    raise Refused("amount_overflow")
                return ex, moved

            result = applied(e, expense)
            if result is None:
                continue
            (ex, moved), _ = result
            running = moved
            expenses.append(ex)
            expense_entries[ex["id"]] = e["id"]
            expense_authors[ex["id"]] = e["author"]
        elif e["kind"] == "recordPayment":

            def payment(version, e=e):
                pay = dict(version["payment"])
                if e["author"] not in (pay.get("from"), pay.get("to")):
                    raise Refused("unauthorized_payment")
                if (pay.get("from") not in participants
                        or pay.get("to") not in participants):
                    raise Refused("unknown_participant")
                if pay.get("from") == pay.get("to"):
                    raise Refused("self_payment")
                if "currency" not in pay:
                    pay["currency"] = currency
                elif not is_currency(pay["currency"]):
                    raise Refused("bill_bad_currency")
                elif pay["currency"] != currency:
                    raise Refused("currency_mismatch")
                decode_payment(pay, currency, set(participants))
                # SPEC.md 10.5: a confirmation names one record, and a method
                # that speaks for the payment's `to` is checked against that
                # record's `to`. Two records under one id name a payee
                # ambiguously, so one recipient's confirmation would settle a
                # debt another never vouched for. The id is its author's own
                # (10.3 step 5), so two records under it are one author's, and
                # the first stands.
                if not owns_id(e["author"], pay["id"]):
                    raise Refused("id_not_minted")
                if any(p["id"] == pay["id"] for p in payments):
                    raise Refused("duplicate_payment")
                # What one participant has recorded paying another, confirmed
                # or not, stays in range: section 14.4 sums the unconfirmed
                # part of it.
                pair = (pay["from"], pay["to"], e["author"])
                total = pair_total.get(pair, 0) + pay["amount"]
                if not _fits(total):
                    raise Refused("amount_overflow")
                return pay, pair, total

            result = applied(e, payment)
            if result is None:
                continue
            (pay, pair, total), version = result
            pair_total[pair] = total
            payments.append(pay)
            payment_authors[pay["id"]] = e["author"]
            payment_entries[pay["id"]] = e["id"]
            payment_digests[pay["id"]] = payment_digest(version["payment"])

    # Confirmations, in a pass of their own once every payment is on the bill.
    known = {p["id"] for p in payments}
    confirmed = set()
    confirmed_by = {}
    for e in live:
        if e["kind"] != "confirmPayment":
            continue

        def confirmation(version, e=e):
            c = _as_dict(version.get("confirmation"))
            method = c.get("method")
            if not isinstance(method, str) or method not in CONFIRMATION_METHODS:
                raise Refused("bill_unknown_confirmation_method")
            speaks_for, needs_ref, settles = CONFIRMATION_METHODS[method]
            if c.get("paymentId") not in known:
                raise Refused("unknown_payment")
            # Section 10.5. A confirmation binds what the record said when it
            # was given. A record withdrawn and written again under the same
            # id, or amended since, is a payment nobody confirmed.
            if c.get("record") != payment_digests[c["paymentId"]]:
                raise Refused("unknown_payment")
            if e["author"] not in participants:
                raise Refused("unknown_participant")
            pay = next(p for p in payments if p["id"] == c["paymentId"])
            if speaks_for and e["author"] != pay[speaks_for]:
                raise Refused("unauthorized_confirmation")
            # A non-empty STRING, not merely something truthy. A number or a
            # list here is not a transaction id, and reading "present" three
            # different ways settles a debt on one device and leaves it open
            # on another.
            ref = c.get("reference")
            if needs_ref and not (isinstance(ref, str) and ref):
                raise Refused("confirmation_missing_reference")
            return c["paymentId"], settles

        result = applied(e, confirmation)
        if result is None:
            continue
        (paid, settles), version = result
        if settles:
            confirmed.add(paid)
            confirmed_by.setdefault(paid, []).append(version)

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
        "identities": {"bound": {k: bound[k] for k in sorted(bound)}},
        "replacedAddresses": replaced,
        "paymentAuthors": {k: payment_authors[k]
                           for k in sorted(payment_authors,
                                           key=lambda k: k.encode("utf-8"))},
        "paymentDigests": {k: payment_digests[k]
                           for k in sorted(payment_digests,
                                           key=lambda k: k.encode("utf-8"))},
        "expenseEntries": _sorted_map(expense_entries),
        "expenseAuthors": _sorted_map(expense_authors),
        "paymentEntries": _sorted_map(payment_entries),
        "rateEntry": rate_entry,
        "rateAuthor": rate_author,
        "withdrawn": sorted(voided),
        # Section 10.2. Total: rows sharing an id are ordered by code.
        "setAside": sorted(set_aside, key=lambda r: (r["id"].encode("utf-8"),
                                                    r["code"].encode("utf-8"))),
    }


# --- Section 10.6: what a signature covers ------------------------------------

ENTRY_SIGNING_DOMAIN = "splitz-entry-v2"


def signing_message(entry, bill_id):
    """Section 10.6. The bytes an entry's signature covers, on `bill_id`.

    The bill is part of the message because an entry does not name it: a
    participant's id and key are the same on every bill, so without it a
    signature from one bill verifies on any other.

    `sig` is excluded because it is the output, and `v` because an entry does
    not carry its own format version through an implementation's object model:
    a reader re-encodes with the version it writes, so a signature covering
    `v` would stop verifying for every existing entry the day it changed.

    Returned as text rather than as a verdict. A case asserting that a
    signature verified would pass in two implementations that disagree about
    the bytes, each checking its own.
    """
    body = {k: v for k, v in entry.items() if k not in ("sig", "v")}
    return ENTRY_SIGNING_DOMAIN + canonical_json({"bill": bill_id, "entry": body})


# --- Section 10.7: who a participant is ---------------------------------------

def resolve_identities(entries, create, verify):
    """Section 10.7. Which key, if any, is bound to each participant id.

    `verify(entry, key)` is the host's curve operation; this fixes everything
    around it. The answer is a function of the entry set alone — never of
    arrival order, never of anything on disk.

    Returns `bound`, mapping a participant id to the key that speaks for it.
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
    # participant it carries, whose id is the one that participant's key
    # derives, and whose signature verifies against that key. An entry naming
    # somebody else proves nothing about them, and a key cannot claim an id it
    # does not derive, so no second key can claim a bound participant.
    for e in entries:
        if e["kind"] != "joinBill":
            continue
        p = _as_dict(e.get("participant"))
        pid = p.get("id")
        key = p.get("identityKey")
        if not isinstance(pid, str) or e["author"] != pid or pid == creator:
            continue
        if participant_id(key) != pid or not verify(e, key):
            continue
        bound[pid] = key

    return bound


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


def copy_key(entry):
    """Section 14.5: how a peer names one copy it holds.

    The entry's id, and `|` and its `sig` when it carries one. The union keeps
    copies by id and signature, so a peer holding a copy whose signature fails
    holds the id and still lacks the entry.
    """
    sig = entry.get("sig")
    return f"{entry.get('id')}|{sig}" if isinstance(sig, str) else f"{entry.get('id')}"


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
    missing = [e for e in order(entries) if copy_key(e) not in known]
    if not missing:
        return {"state": "nothing", "entryCount": 0}
    try:
        uri = encode_payload(DELTA_PREFIX, {"v": 1, "log": missing})
    except Refused as r:
        return {"state": "too_big", "entryCount": len(missing), "code": r.code}
    return {"state": "square", "uri": uri, "entryCount": len(missing)}


def withholdings(plan, bill, payer, recorded_by=None):
    """Splits `payer`'s settlements into what a request may carry and what
    section 14.4 holds back.

    Pure: reads the bill, and decides nothing a wallet is entitled to decide.
    `recorded_by` maps a payment id to the author of its record, as the fold
    reports it; given, only a record the payer wrote withholds anything.
    """
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

    carried, awaiting = [], []
    for s in mine:
        # The payee, and every creditor whose debt this settlement covers
        # (section 6.3): netting can reroute a debt already paid onto
        # somebody else.
        owed_to = {s["to"]} | {c["to"] for c in s.get("covers") or ()}
        paid_to = _by_id(t for t in owed_to if t in pending)
        if paid_to:
            # Who the unconfirmed money went to, which is not the payee when
            # netting rerouted the debt: the payment to confirm, or to take
            # back, is theirs.
            awaiting.append({"to": s["to"], "owed": s["amount"],
                             "paid": _checked_sum([pending[t] for t in paid_to]),
                             "paidTo": paid_to})
        else:
            carried.append(s)
    return {"carried": carried, "awaiting": awaiting}


UNPRICEABLE_CODES = ("rate_amount_too_large", "zip321_amount_too_large",
                     "zip321_fiat_too_many_digits")


def choose_payouts(participants, via):
    """Section 14.8. The participants with the payer's chosen payouts first.

    `via` maps a participant id to the index of one of their declared payouts.
    That payout moves to the front and the rest keep their order; a
    participant `via` does not name is unchanged. Ids are checked in section
    2.3's order, so the first refusal is the same everywhere.
    """
    by_id = {p["id"]: p for p in participants}
    for pid in _sorted_ids(via):
        who = by_id.get(pid)
        if who is None:
            raise Refused("unknown_participant")
        payouts = who.get("payouts") or []
        index = via[pid]
        if (not isinstance(index, int) or isinstance(index, bool)
                or index < 0 or index >= len(payouts)):
            raise Refused("payout_not_declared")
    out = []
    for p in participants:
        if p["id"] in via:
            payouts = list(p.get("payouts") or [])
            chosen = payouts.pop(via[p["id"]])
            p = {**p, "payouts": [chosen] + payouts}
        out.append(p)
    return out


def bill_memo(bill_id):
    """Section 8.5: what a request carries to every output that takes a memo."""
    return f"splitz:{bill_id}".encode("utf-8")


def _takes_memo(address):
    try:
        return parse_address(address)["canReceiveMemo"]
    except Refused:
        return False


def render_obligation(settlements, participants, rate, currency,
                      skip_unpayable=False, include_fiat=False, bill_id="b"):
    """Section 8.5. One payer's whole obligation as a payment request.

    Reports three groups: the outputs the URI carries, the recipients it
    cannot carry and why, and the total each accounts for. A URI that silently
    covers three of a payer's four debts is indistinguishable, to the payer who
    sends it, from one that settles all four.
    """
    if len({s["from"] for s in settlements}) > 1:
        raise Refused("obligation_mixed_payers")
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
        # Each output is priced, and checked against what section 8 renders,
        # on its own: one debt past what a request can carry is that debt's
        # to report, not a reason to carry none of the others.
        try:
            zatoshi = fiat_to_zatoshi(s["amount"], rate, currency, "up")
            render_amount(zatoshi)
            if include_fiat:
                render_fiat(currency, s["amount"])
        except Refused as r:
            # Only the refusals one output's size produces. One about the
            # rate itself refuses every output alike and is raised.
            if not skip_unpayable or r.code not in UNPRICEABLE_CODES:
                raise
            unpayable.append({"id": s["to"], "reason": "unpriceable",
                              "minorUnits": s["amount"]})
            withheld = _exact_i64(withheld + s["amount"])
            continue
        payments.append({
            "address": address,
            "zatoshi": zatoshi,
            "fiat": (currency, s["amount"]),
            # Section 8.5: what ties the send to this bill.
            "memo": bill_memo(bill_id) if _takes_memo(address) else None,
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
    and a signature joined by `|`, and only that copy does. Either may end in
    `@` and a key, and then verifies against that key alone — which is what
    lets a case require the fold to ask about the author's own key rather
    than any key it has.
    """
    ok = set(verifies)

    def verify(entry, key):
        eid = entry.get("id")
        sig = entry.get("sig")
        names = [eid] + ([f"{eid}|{sig}"] if isinstance(sig, str) else [])
        return any(n in ok or f"{n}@{key}" in ok for n in names)
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


# --- Section 9.1 and 9.2: what a host never writes ---------------------------

# Unicode's White_Space property: the one set every implementation reads as
# blank. Python's str.isspace also counts U+001C..U+001F, and Dart's trim
# U+FEFF, so neither is used.
_WHITE_SPACE = frozenset(
    [*range(0x09, 0x0E), 0x20, 0x85, 0xA0, 0x1680, *range(0x2000, 0x200B),
     0x2028, 0x2029, 0x202F, 0x205F, 0x3000])


def _blank(text):
    return not isinstance(text, str) or all(ord(c) in _WHITE_SPACE for c in text)


def check_written_payout(payout):
    """Section 9.1. A payout nobody could be paid by is never written."""
    kind = payout.get("type")
    fields = {"zec": ("address",), "swap": ("asset", "chain", "address")}
    if any(_blank(payout.get(f)) for f in fields.get(kind, ())):
        raise Refused("payout_incomplete")


def check_written_payment(payment):
    """Section 9.2. A payment of nothing, or a swap with no reference."""
    amount = payment.get("amount")
    if not isinstance(amount, int) or isinstance(amount, bool) or amount <= 0:
        raise Refused("payment_not_positive")
    if payment.get("method") == "swap" and _blank(payment.get("reference")):
        raise Refused("swap_missing_reference")
