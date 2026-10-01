"""The Worker's per-channel, per-address and expiry bounds, against a running
Worker started with small ones:

    wrangler dev --var MAX_CHANNEL_CHARS:4096 --var MAX_SOURCE_CHARS:8192 \
                 --var EXPIRE_DAYS:0.00005

    python3 bounds.py http://127.0.0.1:<port>

EXPIRE_DAYS 0.00005 is 4.32 seconds. Each push below is the first to its
channel in this run, so channels are digests of a fresh random prefix.
"""

import hashlib
import json
import os
import sys
import time
import urllib.error
import urllib.request

origin = sys.argv[1]
run = os.urandom(8).hex()


def channel(name: str) -> str:
    return hashlib.sha256(f"{run}:{name}".encode()).hexdigest()


def push(name: str, *blobs: str) -> int:
    body = json.dumps({"blobs": list(blobs)}).encode()
    request = urllib.request.Request(f"{origin}/c/{channel(name)}", data=body, method="POST")
    try:
        with urllib.request.urlopen(request) as r:
            return r.status
    except urllib.error.HTTPError as e:
        return e.code


def fetch(name: str) -> list:
    with urllib.request.urlopen(f"{origin}/c/{channel(name)}") as r:
        return json.load(r)["blobs"]


failures = []


def expect(what: str, got, want) -> None:
    shown = repr(got)
    shown = shown if len(shown) <= 60 else f"{shown[:57]}..."
    print(f"{'ok  ' if got == want else 'FAIL'} {what}: {shown}")
    if got != want:
        failures.append(what)


expect("a push inside every bound", push("a", "a" * 3000), 200)
expect("the same blob again adds nothing", push("a", "a" * 3000), 200)
expect("a channel past its bound", push("a", "b" * 2000), 507)
expect("another channel, same address", push("b", "c" * 3000), 200)
expect("the address past its day's bound", push("c", "d" * 3000), 429)
expect("a held channel before it expires", len(fetch("a")), 1)
time.sleep(6)
# Expiry runs on a push that adds something.
expect("a push after the others went quiet", push("d", "e" * 100), 200)
expect("a channel nobody pushed to since is dropped", fetch("a"), [])
expect("the channel pushed to just now is kept", len(fetch("d")), 1)

sys.exit(1 if failures else 0)
