#!/usr/bin/env python3
"""ZEC prices from Coinbase's /v2/exchange-rates, as the host packages read them.

    python3 tools/contracts/coinbase.py            # rewrite coinbase_cases.json
    python3 tools/contracts/coinbase.py --capture  # re-capture the fixture
    python3 tools/contracts/coinbase.py --check    # exit 1 if the live answer moved

The cases pair an answer with a currency and the minor units the packages must
read from it. Expectations come from this file alone: the rate is a decimal
string, scaled by the currency's ISO 4217 exponent exactly and rounded half up.
The exponent register is the one splitz_host ships (lib/src/currencies.dart).

Source: https://docs.cdp.coinbase.com/coinbase-app/track-apis/exchange-rates —
GET https://api.coinbase.com/v2/exchange-rates?currency=ZEC answers
{"data": {"currency": "ZEC", "rates": {"<CODE>": "<decimal>", …}}}, with no
authentication. fixtures/coinbase_rates.json is one such answer, captured as is.
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
CASES = HERE / "coinbase_cases.json"
FIXTURE = HERE / "fixtures" / "coinbase_rates.json"
LIVE = "https://api.coinbase.com/v2/exchange-rates?currency=ZEC"
MAX = 2**53 - 1
DECIMAL = re.compile(r"^[0-9]+(\.[0-9]+)?$")


def exponents() -> dict[str, int]:
    text = (ROOT / "splitz_host/lib/src/currencies.dart").read_text()
    return {c: int(e) for c, e in re.findall(r"^  '([A-Z]{3})': ([0-9]),$", text, re.M)}


EXP = exponents()


def scale(decimal: str, exponent: int):
    scaled = Decimal(decimal).scaleb(exponent).to_integral_value(rounding=ROUND_HALF_UP)
    return None if scaled <= 0 or scaled > MAX else int(scaled)


def read(body: str, currency: str):
    """Minor units per ZEC, None when not priced, or 'malformed'."""
    try:
        decoded = json.loads(body)
    except ValueError:
        return "malformed"
    data = decoded.get("data") if isinstance(decoded, dict) else None
    if not isinstance(data, dict) or data.get("currency") != "ZEC":
        return "malformed"
    rates = data.get("rates")
    if not isinstance(rates, dict):
        return "malformed"
    exponent = EXP.get(currency)
    if exponent is None:
        return None
    raw = rates.get(currency.upper())
    if raw is None:
        return None
    if not isinstance(raw, str) or not DECIMAL.match(raw):
        return "malformed"
    return scale(raw, exponent)


def body(rates: dict, currency: str = "ZEC") -> str:
    return json.dumps({"data": {"currency": currency, "rates": rates}})


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

    for code in ["USD", "EUR", "JPY", "KES", "INR", "KWD"]:
        add(f"captured_{code.lower()}", captured, code)
    add("captured_xau_has_no_exponent", captured, "XAU")
    add("captured_lower_case_is_not_a_code", captured, "usd")
    add("half_a_cent_rounds_up", body({"USD": "1.005"}), "USD")
    add("just_under_half_a_cent_rounds_down", body({"USD": "1.0049"}), "USD")
    add("a_whole_number", body({"USD": "42"}), "USD")
    add("many_places", body({"EUR": "1230.45257433600946192"}), "EUR")
    add("a_tiny_price_rounds_to_nothing", body({"USD": "0.004"}), "USD")
    add("three_places_for_kwd", body({"KWD": "0.0005"}), "KWD")
    add("no_places_for_jpy", body({"JPY": "219354.5"}), "JPY")
    add("zero_is_not_a_price", body({"USD": "0"}), "USD")
    add("at_the_bound", body({"JPY": "9007199254740991"}), "JPY")
    add("past_the_bound", body({"USD": "90071992547409.92"}), "USD")
    add("a_number_is_not_a_rate", body({"USD": 1393.12}), "USD")
    add("an_exponent_is_not_a_rate", body({"USD": "1.5e2"}), "USD")
    add("a_negative_rate_is_malformed", body({"USD": "-5"}), "USD")
    add("a_code_it_does_not_price", body({"USD": "1"}), "EUR")
    add("rates_for_another_base", body({"USD": "1"}, "BTC"), "USD")
    add("no_data", '{"rates":{"USD":"1"}}', "USD")
    add("no_rates", '{"data":{"currency":"ZEC"}}', "USD")
    add("not_json", "<html>blocked</html>", "USD")
    add("the_error_answer", '{"error":"base currency not recognized","code":3}', "USD")
    return out


def fetch() -> tuple[int, str]:
    request = urllib.request.Request(LIVE, headers={"User-Agent": "curl/8", "Accept": "application/json"})
    with urllib.request.urlopen(request, timeout=60) as r:
        return r.status, r.read().decode()


def check() -> int:
    status, answer = fetch()
    problems = [] if status == 200 else [f"HTTP {status}"]
    for code in ["USD", "EUR", "JPY", "KES", "INR"]:
        got = read(answer, code)
        if not isinstance(got, int):
            problems.append(f"{code}: read {got!r} from {answer[:200]}")
    if problems:
        print("\n".join(problems))
        return 1
    print("live answer reads: " + ", ".join(f"{c} {read(answer, c)}" for c in ["USD", "EUR", "KES"]))
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
        "description": "Coinbase /v2/exchange-rates?currency=ZEC answers and the minor units per ZEC the host packages read from them.",
        "count": 0,
        "cases": cases(),
    }
    doc["count"] = len(doc["cases"])
    CASES.write_text(json.dumps(doc, indent=2) + "\n")
    print(f"{CASES.relative_to(ROOT)}: {doc['count']} cases")
    return 0


if __name__ == "__main__":
    sys.exit(main())
