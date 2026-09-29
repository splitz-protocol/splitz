#!/usr/bin/env python3
"""ZEC's USD price from Binance's ticker, as the host packages read it.

    python3 tools/contracts/binance.py            # rewrite binance_cases.json
    python3 tools/contracts/binance.py --capture  # re-capture the fixture
    python3 tools/contracts/binance.py --check    # exit 1 if the live answer moved

The cases pair an answer with a currency and the minor units the packages must
read from it. Binance lists ZEC against stablecoins and crypto, not currencies,
so the ZECUSDT price is read as USD and every other currency is unpriced. The
price is a decimal string, scaled by USD's exponent exactly and rounded half up.

Source: https://developers.binance.com/docs/binance-spot-api-docs/rest-api/market-data-endpoints —
GET https://data-api.binance.vision/api/v3/ticker/price?symbol=ZECUSDT answers
{"symbol": "ZECUSDT", "price": "<decimal>"}. fixtures/binance_ticker.json is
one such answer, captured as is.
"""

from __future__ import annotations

import json
import pathlib
import re
import sys
import urllib.request
from decimal import Decimal, ROUND_HALF_UP

HERE = pathlib.Path(__file__).resolve().parent
ROOT = HERE.parents[1]
CASES = HERE / "binance_cases.json"
FIXTURE = HERE / "fixtures" / "binance_ticker.json"
LIVE = "https://data-api.binance.vision/api/v3/ticker/price?symbol=ZECUSDT"
SYMBOL = "ZECUSDT"
MAX = 2**53 - 1
DECIMAL = re.compile(r"^[0-9]+(\.[0-9]+)?$")


def read(body: str, currency: str):
    """Minor units of USD per ZEC, None when not priced, or 'malformed'."""
    try:
        decoded = json.loads(body)
    except ValueError:
        return "malformed"
    if not isinstance(decoded, dict) or decoded.get("symbol") != SYMBOL:
        return "malformed"
    raw = decoded.get("price")
    if not isinstance(raw, str) or not DECIMAL.match(raw):
        return "malformed"
    if currency != "USD":
        return None
    scaled = Decimal(raw).scaleb(2).to_integral_value(rounding=ROUND_HALF_UP)
    return None if scaled <= 0 or scaled > MAX else int(scaled)


def ticker(price, symbol: str = SYMBOL) -> str:
    return json.dumps({"symbol": symbol, "price": price})


def cases() -> list[dict]:
    captured = FIXTURE.read_text().strip()
    out = []

    def add(name, answer, currency):
        result = read(answer, currency)
        case = {"name": name, "body": answer, "currency": currency}
        if result == "malformed":
            case["error"] = "malformed"
        else:
            case["expect"] = result
        out.append(case)

    add("captured_usd", captured, "USD")
    add("captured_lower_case_is_not_a_code", captured, "usd")
    add("captured_eur_is_not_priced", captured, "EUR")
    add("half_a_cent_rounds_up", ticker("1.00500000"), "USD")
    add("just_under_half_a_cent_rounds_down", ticker("1.00490000"), "USD")
    add("a_whole_number", ticker("42"), "USD")
    add("a_tiny_price_rounds_to_nothing", ticker("0.00400000"), "USD")
    add("zero_is_not_a_price", ticker("0.00000000"), "USD")
    add("past_the_bound", ticker("90071992547409.92"), "USD")
    add("a_number_is_not_a_price", ticker(1390.54), "USD")
    add("an_exponent_is_not_a_price", ticker("1.5e2"), "USD")
    add("another_symbol", ticker("1", "ZECUSDC"), "USD")
    add("the_error_answer", '{"code":-1121,"msg":"Invalid symbol."}', "USD")
    add("not_json", "<html>blocked</html>", "USD")
    add("not_an_object", "[1,2]", "USD")
    return out


def fetch() -> tuple[int, str]:
    request = urllib.request.Request(LIVE, headers={"User-Agent": "curl/8", "Accept": "application/json"})
    with urllib.request.urlopen(request, timeout=60) as r:
        return r.status, r.read().decode()


def check() -> int:
    status, answer = fetch()
    got = read(answer, "USD")
    if status != 200 or not isinstance(got, int):
        print(f"HTTP {status}, read {got!r} from {answer[:200]}")
        return 1
    print(f"live answer reads: USD {got}")
    return 0


def main() -> int:
    if "--check" in sys.argv:
        return check()
    if "--capture" in sys.argv:
        status, answer = fetch()
        if status != 200:
            print(f"HTTP {status}")
            return 1
        FIXTURE.write_text(answer.strip() + "\n")
        print(f"{FIXTURE.relative_to(ROOT)}: captured")
    doc = {
        "description": "Binance ZECUSDT ticker answers and the minor units of USD per ZEC the host packages read from them.",
        "count": 0,
        "cases": cases(),
    }
    doc["count"] = len(doc["cases"])
    CASES.write_text(json.dumps(doc, indent=2) + "\n")
    print(f"{CASES.relative_to(ROOT)}: {doc['count']} cases")
    return 0


if __name__ == "__main__":
    sys.exit(main())
