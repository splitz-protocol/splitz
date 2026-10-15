"""The Worker's per-address and expiry bounds, against a running Worker
started with small ones:

    wrangler dev --var MAX_SOURCE_CHARS:8192 --var EXPIRE_DAYS:0.00005 \
        --var LEGACY_UNSTAMPED:1

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

sys.path.insert(0, os.path.join(os.path.dirname(os.path.abspath(__file__)), ".."))
from server import work_bits, work_done  # noqa: E402

origin = sys.argv[1]
run = os.urandom(8).hex()


def channel(name: str) -> str:
    return hashlib.sha256(f"{run}:{name}".encode()).hexdigest()


def stamp(name: str, blobs: list, bits: int, exact: bool = False) -> str:
    """The first nonce from zero with `bits` leading zero bits, or with
    exactly `bits` when `exact`."""
    n = 0
    while not (work_done(channel(name), blobs, str(n)) == bits if exact
               else work_done(channel(name), blobs, str(n)) >= bits):
        n += 1
    return str(n)


def push(name: str, *blobs: str, source: str = "192.0.2.1", work=None) -> int:
    payload = {"blobs": list(blobs)}
    if work is not None:
        payload["work"] = work
    body = json.dumps(payload).encode()
    request = urllib.request.Request(f"{origin}/c/{channel(name)}", data=body, method="POST",
                                     headers={"CF-Connecting-IP": source})
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
expect("more to one channel than 4096 is no bound", push("a", "b" * 3000), 200)
expect("the address past its day's bound", push("b", "c" * 3000), 429)
# One address filling its day on a channel stops nobody else writing to it.
expect("another address, the same channel", push("a", "d" * 3000, source="192.0.2.2"), 200)
expect("every accepted blob is held", len(fetch("a")), 3)
# An IPv6 sender is its /64: another address in it shares the day's bound.
expect("an IPv6 address inside its bound",
       push("f", "e" * 6000, source="2001:db8:1:2::1"), 200)
expect("another address in the same /64",
       push("g", "f" * 3000, source="2001:DB8:1:2:ffff::9"), 429)
expect("an address in the next /64", push("g", "f" * 3000, source="2001:db8:1:3::1"), 200)
# §15.5: a stamped push is held to no address's bound. 192.0.2.1 is past its
# day's bound above; 3000 characters are three units, 18 bits at base 16.
three = "h" * 3000
expect("a stamped push past the address's bound",
       push("h", three, work=stamp("h", [three], work_bits(3000, 16))), 200)
expect("a stamp made for other blobs", push("h", "i" * 3000, work=stamp("h", [three], 18)), 428)
expect("a stamp one bit short",
       push("j", "j" * 3000, work=stamp("j", ["j" * 3000], 17, exact=True)), 428)
expect("a stamp that is not a nonce", push("j", "j", work="12a"), 400)
expect("a nonce with a trailing newline", push("j", "j", work=stamp("j", ["j"], 16) + "\n"), 400)
for junk in ("a=", "a b", "\u00e9", ""):
    expect(f"a blob that is not base64url ({junk!r})",
           push("k", junk, work=stamp("k", [junk], 16)), 400)
expect("a channel to re-push", push("e", "x" * 100, "y" * 100, source="192.0.2.3"), 200)
time.sleep(6)
# Nothing has run expiry since the sleep. It runs before a push is compared
# with the channel, so a re-push of what the channel held keeps all of it.
expect("a re-push after expiry",
       push("e", "x" * 100, "y" * 100, "z" * 100, source="192.0.2.3"), 200)
expect("holds all it carried", len(fetch("e")), 3)
# And before a fetch is answered.
expect("a channel nobody pushed to since is dropped", fetch("a"), [])

sys.exit(1 if failures else 0)
