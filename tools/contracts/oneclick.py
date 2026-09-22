#!/usr/bin/env python3
"""The part of the 1Click API schema this tree speaks, pinned.

    python3 tools/contracts/oneclick.py          # rewrite oneclick.json
    python3 tools/contracts/oneclick.py --check  # exit 1 if upstream moved

Source: https://1click.chaindefuser.com/docs/v0/openapi.yaml. Only the schemas
the swap client sends or reads are kept, with everything they reference, so a
change anywhere else in a 46,000-line file does not fail the check.

The YAML is read through Ruby's standard library, which both this machine and
a hosted Ubuntu runner carry; Python has no YAML reader without a package.
"""

from __future__ import annotations

import json
import pathlib
import subprocess
import sys
import urllib.request

SOURCE = "https://1click.chaindefuser.com/docs/v0/openapi.yaml"
HERE = pathlib.Path(__file__).resolve().parent
PINNED = HERE / "oneclick.json"

# The endpoints the client calls, and the schemas that describe them.
ROOTS = ["TokenResponse", "QuoteRequest", "QuoteResponse",
         "GetExecutionStatusResponse", "BadRequestResponse"]
ENDPOINTS = {"GET /v0/tokens": {"response": "TokenResponse[]"},
             "POST /v0/quote": {"request": "QuoteRequest",
                                "response": "QuoteResponse"},
             "GET /v0/status": {"response": "GetExecutionStatusResponse"}}


def fetch() -> dict:
    # The host refuses Python's default User-Agent with a 403.
    request = urllib.request.Request(SOURCE, headers={"User-Agent": "curl/8"})
    with urllib.request.urlopen(request, timeout=60) as r:
        text = r.read()
    out = subprocess.run(
        ["ruby", "-ryaml", "-rjson", "-e",
         "puts JSON.generate(YAML.safe_load(STDIN.read, aliases: true))"],
        input=text, capture_output=True, check=True)
    return json.loads(out.stdout)


def refs(node, found: set[str]) -> None:
    if isinstance(node, dict):
        ref = node.get("$ref")
        if isinstance(ref, str) and ref.startswith("#/components/schemas/"):
            found.add(ref.rsplit("/", 1)[1])
        for v in node.values():
            refs(v, found)
    elif isinstance(node, list):
        for v in node:
            refs(v, found)


def extract(doc: dict) -> dict:
    all_schemas = doc["components"]["schemas"]
    keep: dict[str, object] = {}
    todo = list(ROOTS)
    while todo:
        name = todo.pop()
        if name in keep:
            continue
        keep[name] = all_schemas[name]
        found: set[str] = set()
        refs(all_schemas[name], found)
        todo.extend(found - keep.keys())
    return {"source": SOURCE, "endpoints": ENDPOINTS,
            "schemas": dict(sorted(keep.items()))}


def render(pinned: dict) -> str:
    return json.dumps(pinned, indent=2, sort_keys=True, ensure_ascii=False) + "\n"


def main() -> int:
    current = render(extract(fetch()))
    if "--check" in sys.argv:
        if PINNED.read_text(encoding="utf-8") == current:
            print("the pinned 1Click schema matches upstream")
            return 0
        print("the 1Click schema moved upstream; run "
              "tools/contracts/oneclick.py, then fix what the tests say")
        return 1
    PINNED.write_text(current, encoding="utf-8")
    print(f"wrote {PINNED.relative_to(HERE.parent.parent)}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
