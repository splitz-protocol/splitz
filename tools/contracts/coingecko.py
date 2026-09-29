#!/usr/bin/env python3
"""ZEC prices from CoinGecko's /simple/price, as the host packages read them.

    python3 tools/contracts/coingecko.py          # rewrite coingecko_cases.json
    python3 tools/contracts/coingecko.py --check  # exit 1 if the live answer moved

The cases pair an answer with a currency and the minor units the packages must
read from it. Expectations come from this file alone: the price is read as the
shortest decimal naming the parsed number, scaled by the currency's ISO 4217
exponent exactly, and rounded half up. The exponent register is the one
splitz_host ships (lib/src/currencies.dart), read from that file.

Source: GET https://api.coingecko.com/api/v3/simple/price?ids=zcash&vs_currencies=…
answers {"zcash": {"<code>": <number>, …}}, leaving out a code it does not
price. fixtures/coingecko_price.json is one such answer, captured as is.
"""

from __future__ import annotations

import json
import os
import pathlib
import re
import sys
import urllib.request
from decimal import Decimal, ROUND_HALF_UP

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[1]
CASES = HERE / "coingecko_cases.json"
FIXTURE = HERE / "fixtures" / "coingecko_price.json"
LIVE = "https://api.coingecko.com/api/v3/simple/price?ids=zcash&vs_currencies=usd,eur,jpy"
MAX = 2**53 - 1


def exponents() -> dict[str, int]:
    text = (ROOT / "splitz_host/lib/src/currencies.dart").read_text()
    return {c: int(e) for c, e in re.findall(r"^  '([A-Z]{3})': ([0-9]),$", text, re.M)}


EXP = exponents()


def read(body: str, currency: str):
    """Minor units per ZEC, None when not priced, or 'malformed'."""
    try:
        decoded = json.loads(body)
    except ValueError:
        return "malformed"
    if not isinstance(decoded, dict) or not isinstance(decoded.get("zcash"), dict):
        return "malformed"
    exponent = EXP.get(currency)
    if exponent is None:
        return None
    raw = decoded["zcash"].get(currency.lower())
    if raw is None:
        return None
    if isinstance(raw, bool) or not isinstance(raw, (int, float)):
        return "malformed"
    if isinstance(raw, float) and (raw != raw or raw in (float("inf"), float("-inf"))):
        return None
    value = Decimal(raw) if isinstance(raw, int) else Decimal(repr(raw))
    scaled = (value.scaleb(exponent)).to_integral_value(rounding=ROUND_HALF_UP)
    if scaled <= 0 or scaled > MAX:
        return None
    return int(scaled)


def cases() -> list[dict]:
    captured = FIXTURE.read_text().strip()
    out = []

    def add(name, body, currency):
        result = read(body, currency)
        case = {"name": name, "body": body, "currency": currency}
        if result == "malformed":
            case["error"] = "malformed"
        else:
            case["expect"] = result
        out.append(case)

    for code in ["USD", "EUR", "JPY", "KWD", "INR"]:
        add(f"captured_{code.lower()}", captured, code)
    add("captured_xau_has_no_exponent", captured, "XAU")
    add("captured_gbp_is_not_in_the_answer", captured, "GBP")
    add("half_a_cent_rounds_up", '{"zcash":{"usd":1.005}}', "USD")
    add("just_under_half_a_cent_rounds_down", '{"zcash":{"usd":1.0049}}', "USD")
    add("a_whole_number", '{"zcash":{"usd":42}}', "USD")
    add("an_exponent", '{"zcash":{"usd":1.5e2}}', "USD")
    add("a_tiny_price_rounds_to_nothing", '{"zcash":{"usd":0.004}}', "USD")
    add("a_tiny_price_rounds_to_a_cent", '{"zcash":{"usd":0.005}}', "USD")
    add("three_places_for_kwd", '{"zcash":{"kwd":0.0005}}', "KWD")
    add("no_places_for_jpy", '{"zcash":{"jpy":218475.5}}', "JPY")
    add("zero_is_not_a_price", '{"zcash":{"usd":0}}', "USD")
    add("negative_is_not_a_price", '{"zcash":{"usd":-5}}', "USD")
    add("at_the_bound", '{"zcash":{"jpy":9007199254740991}}', "JPY")
    add("past_the_bound", '{"zcash":{"usd":90071992547409.92}}', "USD")
    add("a_string_is_not_a_price", '{"zcash":{"usd":"1388.19"}}', "USD")
    add("no_zcash_member", '{"bitcoin":{"usd":1}}', "USD")
    add("not_an_object", "[1,2]", "USD")
    add("not_json", "<html>rate limited</html>", "USD")
    add("an_empty_zcash_member", '{"zcash":{}}', "USD")
    return out


def check() -> int:
    headers = {"User-Agent": "curl/8", "Accept": "application/json"}
    # A demo key, when one is configured, as the header the Demo API reads
    # (https://docs.coingecko.com/v3.0.1/reference/authentication). Without
    # one the keyless API is asked, which blocks some callers outright.
    key = os.environ.get("COINGECKO_API_KEY", "").strip()
    if key:
        headers["x-cg-demo-api-key"] = key
    request = urllib.request.Request(LIVE, headers=headers)
    with urllib.request.urlopen(request, timeout=60) as r:
        status, body = r.status, r.read().decode()
    problems = []
    if status != 200:
        problems.append(f"HTTP {status}")
    for code in ["USD", "EUR", "JPY"]:
        got = read(body, code)
        if not isinstance(got, int):
            problems.append(f"{code}: read {got!r} from {body[:200]}")
    if problems:
        print("\n".join(problems))
        return 1
    print(f"live answer reads: {body}")
    return 0


def main() -> int:
    if "--check" in sys.argv:
        return check()
    doc = {
        "description": "CoinGecko /simple/price answers and the minor units per ZEC the host packages read from them.",
        "count": 0,
        "cases": cases(),
    }
    doc["count"] = len(doc["cases"])
    CASES.write_text(json.dumps(doc, indent=2) + "\n")
    print(f"{CASES.relative_to(ROOT)}: {doc['count']} cases")
    return 0


if __name__ == "__main__":
    sys.exit(main())
