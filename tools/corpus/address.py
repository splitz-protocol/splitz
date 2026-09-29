#!/usr/bin/env python3
"""Generates vectors/address.json from SPEC.md section 8.6.

Three sources of addresses, so the corpus is not only what this reference
itself can encode:

- strings published elsewhere: the Unified Address test vectors of
  zcash-test-vectors `unified_address.py` (as carried in zcash_address 0.13,
  `src/kind/unified/address/test_vectors.rs`), the fixed addresses of
  zcash_address 0.13 `src/encoding.rs` and ZIP 320, and the corpus's own
  mainnet Unified Addresses;
- addresses built here from fixed bytes, for every kind on every network;
- those same builds broken one rule at a time.

The encoders below exist only to build inputs. Every expectation comes from
`_spec.parse_address`.
"""
import hashlib
import json
import pathlib
import sys

sys.path.insert(0, str(pathlib.Path(__file__).parent))
from _spec import (  # noqa: E402
    parse_address, Refused, ADDRESSES, BECH32_CHARSET, BECH32_CONST,
    BECH32M_CONST, BASE58_ALPHABET, _bech32_polymod,
)

# --- encoders, for building inputs only --------------------------------------


def bech32_encode(hrp, data, constant, pad_bits=None):
    """`hrp` + "1" + data regrouped into 5-bit values + checksum.

    `pad_bits` overrides the value of the trailing bits of the last group, to
    build an encoding whose padding is not zero.
    """
    acc = bits = 0
    values = []
    for byte in data:
        acc = (acc << 8) | byte
        bits += 8
        while bits >= 5:
            bits -= 5
            values.append((acc >> bits) & 31)
    if bits:
        tail = (acc << (5 - bits)) & 31
        if pad_bits is not None:
            tail |= pad_bits & ((1 << (5 - bits)) - 1)
        values.append(tail)
    expanded = [ord(c) >> 5 for c in hrp] + [0] + [ord(c) & 31 for c in hrp]
    poly = _bech32_polymod(expanded + values + [0] * 6) ^ constant
    check = [(poly >> 5 * (5 - i)) & 31 for i in range(6)]
    return hrp + "1" + "".join(BECH32_CHARSET[v] for v in values + check)


def base58check(payload):
    raw = payload + hashlib.sha256(hashlib.sha256(payload).digest()).digest()[:4]
    n = int.from_bytes(raw, "big")
    out = ""
    while n:
        n, r = divmod(n, 58)
        out = BASE58_ALPHABET[r] + out
    return "1" * (len(raw) - len(raw.lstrip(b"\x00"))) + out


def f4jumble(m):
    """ZIP 316, "Jumbling"."""
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

    x = xor(b, g(0, a))
    y = xor(a, h(0, x))
    d = xor(x, g(1, y))
    c = xor(y, h(1, d))
    return bytes(c + d)


def compact(n):
    if n < 253:
        return bytes([n])
    if n <= 0xFFFF:
        return b"\xfd" + n.to_bytes(2, "little")
    if n <= 0xFFFFFFFF:
        return b"\xfe" + n.to_bytes(4, "little")
    return b"\xff" + n.to_bytes(8, "little")


def item(typecode, data, typecode_bytes=None):
    return (typecode_bytes or compact(typecode)) + compact(len(data)) + data


def unified(hrp, items, padding_hrp=None, constant=BECH32M_CONST, tail=b""):
    body = b"".join(items) + tail
    pad = (padding_hrp if padding_hrp is not None else hrp).encode().ljust(16, b"\x00")
    return bech32_encode(hrp, f4jumble(body + pad), constant)


def fixed(tag, n):
    """`n` deterministic bytes, distinct per tag."""
    out = b""
    counter = 0
    while len(out) < n:
        out += hashlib.sha256(f"{tag}/{counter}".encode()).digest()
        counter += 1
    return out[:n]


PKH = fixed("p2pkh", 20)
SH = fixed("p2sh", 20)
SAP = fixed("sapling", 43)
ORC = fixed("orchard", 43)

P2PKH = item(0, PKH)
P2SH = item(1, SH)
SAPLING = item(2, SAP)
ORCHARD = item(3, ORC)

TRANSPARENT_LEAD = {
    ("main", "p2pkh"): b"\x1c\xb8", ("main", "p2sh"): b"\x1c\xbd",
    ("test", "p2pkh"): b"\x1d\x25", ("test", "p2sh"): b"\x1c\xba",
}


def flip(text, at):
    """`text` with the character at `at` replaced by another of its alphabet."""
    c = text[at]
    alphabet = BECH32_CHARSET if c in BECH32_CHARSET and not c.isupper() else BASE58_ALPHABET
    other = alphabet[(alphabet.index(c) + 1) % len(alphabet)]
    return text[:at] + other + text[at + 1:]


# --- published addresses -----------------------------------------------------

# zcash-test-vectors `unified_address.py`, as carried in zcash_address 0.13
# `src/kind/unified/address/test_vectors.rs`: the first vector of each
# receiver set that file holds, with the typecodes its fields name.
ZIP316_VECTORS = [
    ("u1l8xunezsvhq8fgzfl7404m450nwnd76zshscn6nfys7vyz2ywyh4cc5daaq0c7q2su5lqfh23sp7fkf3kt27ve5948mzpfdvckzaect2jtte308mkwlycj2u0eac077wu70vqcetkxf",
     [0, 2]),
    ("u1pg2aaph7jp8rpf6yhsza25722sg5fcn3vaca6ze27hqjw7jvvhhuxkpcg0ge9xh6drsgdkda8qjq5chpehkcpxf87rnjryjqwymdheptpvnljqqrjqzjwkc2ma6hcq666kgwfytxwac8eyex6ndgr6ezte66706e3vaqrd25dzvzkc69kw0jgywtd0cmq52q5lkw6uh7hyvzjse8ksx",
     [0, 2, 3]),
    ("u1ay3aawlldjrmxqnjf5medr5ma6p3acnet464ht8lmwplq5cd3ugytcmlf96rrmtgwldc75x94qn4n8pgen36y8tywlq6yjk7lkf3fa8wzjrav8z2xpxqnrnmjxh8tmz6jhfh425t7f3vy6p4pd3zmqayq49efl2c4xydc0gszg660q9p",
     [2, 3]),
    ("u1snf9yr883aj2hm8pksp9aymnqdwzy42rpzuffevj35hhxeckays5pcpeq7vy2mtgzlcuc4mnh9443qnuyje0yx6h59angywka4v2ap6kchh2j96ezf9w0c0auyz3wwts2lx5gmk2sk9",
     [0, 3]),
    ("u1en8ysypun4gdkdnu8zqqg6k73ankr9ffwfzg08wtzg9z939w0wupewemfrc8a630e8gc4uqucym0l4v44fszy3et4veyypt3jsyp0whfpfsn2lw30kj8nepe6wvvasf00wklh85u9v8glqndupmamk9z2ja9sanf70pp4yxvkt3dmyzxa0kkhv2c9pxmkghrxqk0590azvya3nzrtevj449nu3laskrhf7c7nj9cyw7ty38mccg4znrr876guu6pzndx7ngwzhmlsn8d89saf5araaacrhr9958xr6z23mj4qtzzn98whdpu8u7n8fhf5d2vypljda62q73du44sf0e0kxmq3gvgkta0qqgq9w6r403gc5jz2any02etmwlttkv84hgh95czhdf2jugk3u36ke0kchcthg240",
     [0, 3, 65532]),
    ("u1sem2gcey0emntrvxyjv8hyhq0w5fr4sxaj3cppgrfqgg6laydh8m78gy2cw2p54zzak3alnnsx4xjuhazpkrfcd90wl0c7ldj6y095hh5j6j2evry9vg5jqp4dyqpwqeryu7pes4sxyyyqwn6egs5daxk4473v9xpgzrwv5n0tvs93nlj4xpphq4vs2w8um9ph7zkte08t7fa509mnrt9apuhr22xq34mp2svjnq6rvfn0hg6lkehxtlj39vgjxjlkjfhx8rw2f02ckq8k5szcxsnhkgr2cqlmf2udl2gqdqr5t6",
     [2, 65533]),
    ("u1ddnjsdcpm36r6aq79n3s68shjweksnmwtdltrh046s8m6xcws9ygyawalxx8n6hg6vegk0wh8zjnafxgh6msppjsljvyt0ynece3lvm0",
     [3]),
    ("u1xdrenc94696j8clxa2xnkdg8xd5t3y8s24urctyxu87vggv0u46qr4lkpnh7gqqdev9wwugt6xkv8c8du8ufhfl8nfjnzusf6cw20wpm85hlshmnmj2lkyhka9rua7qw7kr0xeajk7y2rlsuwl6z6l5l3wq3v6rrqt9e8zy7sc7pww45jznrj4xy6h9rp4kjy5xtl5upr30u4cyk58kv3t80k3p8w97k3e345h7avmjylxakx6sgyk5ss8th5kqay50ewav62eeep7tghzejaflsdstpwz55haex398jqpq27007me2",
     [2, 3, 65532]),
    ("u1tqx832p4wsfe9pd67ggm3qsmfuvdhqvw2259y7uwug7y0lpeu87fmgpqh3zmamex3fzs0d4ct4hhsg2csj5z0q5f3f7n656ap8e4nlng9c4440rz9s7ekxanfw6g84f7vu82fumtmlz3vstl2a9ufa0970k4knsz2wpsjt2xycqeay76pt4fx3ak9y7mps2q6qe2n2h7wkakxr7xu6vd36zhhzgln7ttmrzc0f9ye3jmyu2pp8l8rect87lfxj2fgckcwz3svdx70a947fz04kgu7e907enzrk676zdkdmuyw2kyrclkmj62kmyy2rjetpus7knmxfuu7z0m63uwfhdynhuu3yrjqu5y089v8zwnh60mw5ngc0kszdjmc339fk9mjn396m5ekv7h7td7fa0u9097xph3y5vth9af4sw6ykxdms84wr544mxxqtmgj027d9e8rnlrazge0kwyydyhder3chwhmaqjk9skuxgxzternw4xx962qed",
     [0, 3, 65531]),
    ("u1uehkuaq6rpfgt4ed5zpvhczg9apgpmyk5eq9qg23j8w7jxkhdnqzacte6gu8zgzfzgxy48ryzus3wnkhfxrxmlhs34xde3f34uxcnv3y6dsgj288vu56xs9f6ghvqsgkhuwtz4kkfxj8pa27v5p3ttlst340zvwx9nj6s0zw8p3wwk3zh37dwc7znqz52gj2fpaapzxzyagah0aeyxwa9fxxvyyj6w989v96ymsgf7s8s6ej9346p60fcjzzynvf9rmxevumdvt8l9mvhdfz4u5j4h7e0zjr2sde7fu7z9s02447qg6qzllm22egnx6ej6qczkkk2ygvpy08un9ggp853sddp6vskrlar6sygxec5f6c2t2eu9zmc728esy4sj9z853gxuplr6hw7lpcwzk20d85vuflnhlfv8nr3020r0v9z83ryudsyjv66rttxq2cscqlrdxakrmpjptzcf",
     [3, 65535]),
    ("u187vrwl4ampyxd5m6aj38n4ndkmj8v6gs97hkt23aps3sn5k89a0gk2smluexgdprcrtm56ezc5c7tjwlrnnl79tjtrxmqd42c5mpyz7g",
     [2]),
]


def build_cases():
    accepted = []   # (name, address, what the source says it is)
    refused = []    # (name, address)

    for ua, codes in ZIP316_VECTORS:
        label = "_".join({0: "p2pkh", 1: "p2sh", 2: "sapling", 3: "orchard"}.get(c, str(c))
                         for c in codes)
        accepted.append((f"zip316_vector_{label}", ua, ("main", "unified", codes)))

    for i, ua in enumerate(ADDRESSES):
        accepted.append((f"corpus_unified_address_{i}", ua, ("main", "unified", None)))

    # zcash_address 0.13 `src/encoding.rs` tests, and ZIP 320's example.
    published = [
        ("published_sapling_main",
         "zs1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqpq6d8g", "main", "sapling"),
        ("published_sapling_test",
         "ztestsapling1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqfhgwqu", "test", "sapling"),
        ("published_sapling_regtest",
         "zregtestsapling1qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqknpr3m", "regtest", "sapling"),
        ("published_unified_test",
         "utest10c5kutapazdnf8ztl3pu43nkfsjx89fy3uuff8tsmxm6s86j37pe7uz94z5jhkl49pqe8yz75rlsaygexk6jpaxwx0esjr8wm5ut7d5s", "test", "unified"),
        ("published_unified_regtest",
         "uregtest15xk7vj4grjkay6mnfl93dhsflc2yeunhxwdh38rul0rq3dfhzzxgm5szjuvtqdha4t4p2q02ks0jgzrhjkrav70z9xlvq0plpcjkd5z3", "regtest", "unified"),
        ("published_p2pkh_main", "t1Hsc1LR8yKnbbe3twRp88p6vFfC5t7DLbs", "main", "p2pkh"),
        ("published_p2pkh_test", "tm9iMLAuYMzJ6jtFLcA7rzUmfreGuKvr7Ma", "test", "p2pkh"),
        ("published_p2sh_main", "t3JZcvsuaXE6ygokL4XUiZSTrQBUoPYFnXJ", "main", "p2sh"),
        ("published_p2sh_test", "t26YoyZ1iPgiMEWL4zGUm74eVWfhyDMXzY2", "test", "p2sh"),
        ("published_tex_main", "tex1s2rt77ggv6q989lr49rkgzmh5slsksa9khdgte", "main", "tex"),
        ("published_tex_test", "textest1qyqszqgpqyqszqgpqyqszqgpqyqszqgpfcjgfy", "test", "tex"),
    ]
    for name, a, net, kind in published:
        accepted.append((name, a, (net, kind, None)))

    # --- built from fixed bytes: every kind on every network -----------------
    for net, hrp in (("main", "zs"), ("test", "ztestsapling"), ("regtest", "zregtestsapling")):
        accepted.append((f"sapling_{net}", bech32_encode(hrp, SAP, BECH32_CONST), (net, "sapling", None)))
    for net, hrp in (("main", "tex"), ("test", "textest"), ("regtest", "texregtest")):
        accepted.append((f"tex_{net}", bech32_encode(hrp, PKH, BECH32M_CONST), (net, "tex", None)))
    for (net, kind), lead in TRANSPARENT_LEAD.items():
        accepted.append((f"{kind}_{net}", base58check(lead + (PKH if kind == "p2pkh" else SH)),
                         (net, kind, None)))
    for net, hrp in (("main", "u"), ("test", "utest"), ("regtest", "uregtest")):
        accepted.append((f"unified_orchard_{net}", unified(hrp, [ORCHARD]), (net, "unified", [3])))
        accepted.append((f"unified_p2sh_sapling_orchard_{net}", unified(hrp, [P2SH, SAPLING, ORCHARD]),
                         (net, "unified", [1, 2, 3])))
        accepted.append((f"unified_p2pkh_sapling_{net}", unified(hrp, [P2PKH, SAPLING]),
                         (net, "unified", [0, 2])))

    # Unknown typecodes are kept by number, and do not count as shielded.
    accepted.append(("unified_keeps_an_unassigned_typecode",
                     unified("u", [ORCHARD, item(0x04, b"\x01" * 5)]), ("main", "unified", [3, 4])))
    accepted.append(("unified_keeps_non_must_understand_metadata",
                     unified("u", [SAPLING, item(0xC0, b"\x00" * 4)]), ("main", "unified", [2, 0xC0])))
    accepted.append(("unified_keeps_an_experimental_typecode",
                     unified("u", [SAPLING, item(0xFFFA, fixed("x", 10))]), ("main", "unified", [2, 0xFFFA])))
    accepted.append(("unified_keeps_the_largest_typecode",
                     unified("u", [SAPLING, item(0x2000000, b"\x07")]), ("main", "unified", [2, 0x2000000])))
    accepted.append(("unified_carries_a_long_unknown_item",
                     unified("u", [ORCHARD, item(0x05, fixed("long", 300))]), ("main", "unified", [3, 5])))

    # --- refusals --------------------------------------------------------------
    sap = bech32_encode("zs", SAP, BECH32_CONST)
    tex = bech32_encode("tex", PKH, BECH32M_CONST)
    ua = unified("u", [P2PKH, SAPLING, ORCHARD])
    t1 = base58check(b"\x1c\xb8" + PKH)

    refused += [
        ("the_empty_string", ""),
        ("a_leading_space", " " + sap),
        ("a_trailing_newline", sap + "\n"),
        ("a_character_outside_the_grammar", sap[:10] + "-" + sap[11:]),
        ("a_non_ascii_letter", sap[:10] + "é" + sap[11:]),
        ("a_prefix_alone", "zs1"),

        ("a_sapling_checksum_that_fails", flip(sap, len(sap) - 1)),
        ("a_sapling_data_character_that_fails", flip(sap, 10)),
        ("a_tex_checksum_that_fails", flip(tex, len(tex) - 1)),
        ("a_unified_checksum_that_fails", flip(ua, len(ua) - 1)),
        ("a_unified_data_character_that_fails", flip(ua, 40)),
        ("a_transparent_checksum_that_fails", flip(t1, len(t1) - 1)),
        ("a_character_base58_leaves_out", t1[:5] + "0" + t1[6:]),

        ("sapling_under_bech32m", bech32_encode("zs", SAP, BECH32M_CONST)),
        ("tex_under_bech32", bech32_encode("tex", PKH, BECH32_CONST)),
        ("unified_under_bech32", unified("u", [ORCHARD], constant=BECH32_CONST)),

        ("a_sapling_address_one_byte_short", bech32_encode("zs", SAP[:42], BECH32_CONST)),
        ("a_sapling_address_one_byte_long", bech32_encode("zs", SAP + b"\x00", BECH32_CONST)),
        ("a_tex_address_one_byte_short", bech32_encode("tex", PKH[:19], BECH32M_CONST)),
        ("a_tex_address_one_byte_long", bech32_encode("tex", PKH + b"\x00", BECH32M_CONST)),
        ("a_transparent_payload_one_byte_short", base58check(b"\x1c\xb8" + PKH[:19])),
        ("a_transparent_payload_one_byte_long", base58check(b"\x1c\xb8" + PKH + b"\x00")),
        ("an_orchard_receiver_one_byte_short", unified("u", [item(3, ORC[:42])])),
        ("a_p2pkh_receiver_one_byte_long", unified("u", [item(0, PKH + b"\x00"), ORCHARD])),

        ("sapling_padding_bits_not_zero", bech32_encode("zs", SAP, BECH32_CONST, pad_bits=1)),
        # 20 bytes are 32 whole groups, so a TEX address has no padding bits.
        ("unified_padding_bits_not_zero", _unified_pad_bits()),
        ("sapling_with_a_whole_extra_group", _extra_group("zs", SAP, BECH32_CONST)),

        ("sapling_in_upper_case", sap.upper()),
        ("unified_in_upper_case", unified("u", [ORCHARD]).upper()),
        ("tex_in_upper_case", tex.upper()),
        ("sapling_in_mixed_case", sap[:5].upper() + sap[5:]),
        ("unified_in_mixed_case", ua[:-1] + ua[-1].upper()),

        ("unified_with_only_p2pkh", unified("u", [P2PKH])),
        ("unified_with_only_p2sh", unified("u", [P2SH])),
        ("unified_with_only_an_unknown_typecode", unified("u", [item(0x04, fixed("u4", 40))])),
        ("unified_with_p2pkh_and_an_unknown_typecode", unified("u", [P2PKH, item(0x04, fixed("u4", 40))])),
        ("unified_with_p2pkh_and_p2sh", unified("u", [P2PKH, P2SH, ORCHARD])),
        ("unified_with_a_duplicate_typecode", unified("u", [SAPLING, SAPLING])),
        ("unified_with_receivers_out_of_order", unified("u", [ORCHARD, SAPLING])),
        ("unified_with_a_must_understand_typecode", unified("u", [SAPLING, item(0xE0, b"\x00" * 4)])),
        ("unified_with_the_last_must_understand_typecode", unified("u", [SAPLING, item(0xFC, b"\x00")])),
        ("unified_with_a_typecode_past_the_limit", unified("u", [SAPLING, item(0x2000001, b"\x07")])),
        ("unified_with_a_non_canonical_typecode",
         unified("u", [item(2, SAP, typecode_bytes=b"\xfd\x02\x00")])),
        ("unified_with_a_non_canonical_length",
         unified("u", [b"\x03\xfd\x2b\x00" + ORC])),
        ("unified_with_a_length_past_the_end", unified("u", [ORCHARD], tail=b"\x05\x09\x00")),
        ("unified_with_a_trailing_byte", unified("u", [ORCHARD], tail=b"\x05")),
        ("unified_with_testnet_padding", unified("u", [ORCHARD], padding_hrp="utest")),
        ("unified_with_empty_padding", unified("u", [ORCHARD], padding_hrp="")),
        ("unified_shorter_than_f4jumble_accepts", _short_unified()),
        ("unified_with_no_items", unified("u", [])),
        # Nothing to unjumble: F4Jumble's inverse is never reached.
        ("unified_with_nothing_to_unjumble", bech32_encode("u", b"", BECH32M_CONST)),

        ("a_revision_2_shielded_prefix", unified("zu", [ORCHARD])),
        ("a_revision_2_transparent_prefix", unified("tu", [P2PKH, ORCHARD])),
        ("a_unified_viewing_key_prefix", unified("uview", [ORCHARD])),
        ("an_unknown_unified_prefix",
         "uinvalid1ck5navqwcng43gvsxwrxsplc22p7uzlcag6qfa0zh09e87efq6rq8wsnv25umqjjravw70rl994n5ueuhza2fghge5gl7zrl2qp6cwmp"),
        ("a_sprout_address_main",
         "zc8E5gYid86n4bo2Usdq1cpr7PpfoJGzttwBHEEgGhGkLUg7SPPVFNB2AkRFXZ7usfphup5426dt1buMmY3fkYeRrQGLa8y"),
        ("a_sprout_address_test",
         "ztJ1EWLKcGwF2S4NA17pAJVdco8Sdkz4AQPxt1cLTEfNuyNswJJc2BbBqYrsRZsp31xbVZwhF7c7a2L9jsF3p3ZwRWpqqyS"),
        ("a_bitcoin_address", "1A1zP1eP5QGefi2DMPTfTL5SLmv7DivfNa"),
        ("a_regtest_sapling_prefix_on_tex_bytes", bech32_encode("zregtestsapling", PKH, BECH32_CONST)),
        ("a_tex_prefix_on_sapling_bytes", bech32_encode("tex", SAP, BECH32M_CONST)),
    ]
    return accepted, refused


def _extra_group(hrp, data, constant):
    """A Bech32 string for `data` followed by one more all-zero 5-bit group:
    the leftover bits then number more than four."""
    good = bech32_encode(hrp, data, constant)
    values = [BECH32_CHARSET.index(c) for c in good[len(hrp) + 1:-6]] + [0]
    expanded = [ord(c) >> 5 for c in hrp] + [0] + [ord(c) & 31 for c in hrp]
    poly = _bech32_polymod(expanded + values + [0] * 6) ^ constant
    check = [(poly >> 5 * (5 - i)) & 31 for i in range(6)]
    return hrp + "1" + "".join(BECH32_CHARSET[v] for v in values + check)


def _unified_pad_bits():
    """A Unified Address whose jumbled bytes leave padding bits, set to one."""
    raw = f4jumble(ORCHARD + b"u".ljust(16, b"\x00"))
    assert (len(raw) * 8) % 5, "needs a length that leaves padding bits"
    return bech32_encode("u", raw, BECH32M_CONST, pad_bits=1)


def _short_unified():
    """A Bech32m string under `u` whose bytes number 47: one short of what
    F4Jumble's inverse accepts, so the jumble is never undone."""
    return bech32_encode("u", fixed("short", 47), BECH32M_CONST)


def main():
    accepted, refused = build_cases()
    out = []
    names = set()
    for name, address, (net, kind, codes) in accepted:
        assert name not in names, name
        names.add(name)
        got = parse_address(address)
        assert (got["network"], got["kind"]) == (net, kind), (name, got)
        if codes is not None:
            assert got["receivers"] == codes, (name, got)
        out.append({"name": name, "address": address, "expect": got})
    for name, address in refused:
        assert name not in names, name
        names.add(name)
        try:
            got = parse_address(address)
        except Refused as r:
            out.append({"name": name, "address": address, "error": r.code})
            continue
        raise SystemExit(f"{name}: expected a refusal, got {got}")

    doc = {"description": "Zcash addresses. SPEC.md section 8.6.",
           "count": len(out), "cases": out}
    p = pathlib.Path(__file__).resolve().parents[2] / "vectors" / "address.json"
    p.write_text(json.dumps(doc, indent=2, ensure_ascii=False) + "\n")
    kinds = sorted({(c["expect"]["kind"], c["expect"]["network"]) for c in out if "expect" in c})
    print(f"{len(out)} cases -> {p.name}: "
          f"{sum('expect' in c for c in out)} accepted, {sum('error' in c for c in out)} refused")
    print("kinds covered: " + " ".join(f"{k}/{n}" for k, n in kinds))


if __name__ == "__main__":
    main()
