#!/usr/bin/env python3
"""A relay for bill sync, for the length of a run.

§11.3 fixes what a sealed entry looks like and the channel it belongs to.
Moving the blobs is the wallet's, and this is the smallest server that does
it — two routes, a dictionary, and no knowledge of what it holds:

    POST /c/<channel>   {"blobs": [...]}  -> {"ok": true}
    GET  /c/<channel>                     -> {"blobs": [...]}

**It cannot read anything it stores.** A channel is SHA-256 of the bill id and
a blob is ciphertext under a key only the participants hold, so a relay that
kept everything forever would still learn nothing but how many blobs a channel
has and when they arrived. Nothing here logs a blob.

    python3 tools/relay/server.py --port 39300

Binds 127.0.0.1 unless `--host` says otherwise. A simulator reaches loopback
on the host; a phone does not, so a run on phones puts this behind HTTPS —
`tools/relay/funnel.sh` at a stable Tailscale origin, or `tools/relay/public.sh`
at a quick tunnel whose origin changes every start — rather than opening it on
the LAN, which would need a cleartext exception in the wallet on each platform.

**Bounded, because it may be reachable by strangers.** One request body is at
most `MAX_BODY_BYTES`; everything held is at most `MAX_HELD_CHARS`. A body
over the first is refused before it is read and the connection closed; a push
that would cross the second is refused whole. A connection that sends nothing
for `--idle-timeout` seconds is closed, so a client that opens connections and
never finishes a request cannot hold every thread.

**It authenticates nobody.** Anyone who can reach it can push, so anyone can
fill it to `MAX_HELD_CHARS`, after which every push is refused with 507 until
it restarts. Nothing here can tell a stranger's blob from a participant's —
that is what not being able to read them means. A bill still works with no
relay (§15.5); a relay somebody has filled is one to restart, or to replace
with one that knows its users.

Without `--state-file` nothing is written to disk: a restart forgets every
channel, and devices re-push their whole log on the next sync. With it, the
store is written by a background thread at most once a `SAVE_INTERVAL` after a
push adds a blob, and when the process is asked to stop — to a temporary file,
then renamed over the old one, so a crash mid-write leaves the previous copy —
and read back at start. A push never waits on the disk, and a crash loses at
most the last interval's pushes, which devices send again on their next sync. The file holds what the relay holds: channel digests
and ciphertext. A file that breaks a bound the running relay keeps is refused
at start rather than loaded in part.
"""

from __future__ import annotations

import argparse
import json
import os
import re
import signal
import socket
import sys
import threading
import time
from collections import OrderedDict
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer

# Mirrored from `HttpSplitsRelay.maxBlobChars`. A bound only one side keeps is
# not a bound: a client that refuses to send one this long should not be able
# to find a server that accepts it.
MAX_BLOB_CHARS = 64 * 1024

# One request. A sync pushes a bill's whole sealed log every time, so this is
# the largest bill the relay carries: 512 blobs at the blob ceiling, tens of
# thousands at an ordinary entry's size.
MAX_BODY_BYTES = 32 * 1024 * 1024

# Everything held, across every channel. A push that would cross it is refused
# whole rather than half-stored.
MAX_HELD_CHARS = 512 * 1024 * 1024

# A channel is a hex digest. Anything else is not one, and letting it through
# would make the store a scratchpad for whoever asked.
CHANNEL = re.compile(r"^[0-9a-f]{64}$")

CHANNELS: "OrderedDict[str, list[str]]" = OrderedDict()
HELD_CHARS = 0
LOCK = threading.Lock()
STATE_FILE: str | None = None

# How long, and how much, an oversize body is read after its refusal (see
# `Relay._linger`).
LINGER_SECONDS = 2.0
LINGER_BYTES = 4 * 1024 * 1024


def load_state(path: str) -> None:
    """Fills the store from `path`, or exits naming the bound it breaks."""
    global HELD_CHARS
    with open(path, encoding="utf-8") as f:
        loaded = json.load(f)
    if not isinstance(loaded, dict):
        raise SystemExit(f"{path}: not an object of channels")
    held = 0
    for channel, blobs in loaded.items():
        if not CHANNEL.match(channel):
            raise SystemExit(f"{path}: {channel[:16]!r} is not a channel")
        if not isinstance(blobs, list) or any(
            not isinstance(b, str) or len(b) > MAX_BLOB_CHARS for b in blobs
        ):
            raise SystemExit(f"{path}: channel {channel[:12]}… holds a bad blob")
        held += sum(len(b) for b in blobs)
    if held > MAX_HELD_CHARS:
        raise SystemExit(f"{path}: {held} characters, over --max-held")
    CHANNELS.update(loaded)
    HELD_CHARS = held


# How often, at most, the state file is rewritten. Each rewrite is the whole
# store, so it is taken off the request path.
SAVE_INTERVAL = 1.0
DIRTY = threading.Event()


def saver() -> None:
    """Writes the store whenever a push has changed it, at most once an interval."""
    while True:
        DIRTY.wait()
        time.sleep(SAVE_INTERVAL)
        flush()


def flush() -> None:
    """Writes the store now, if a push changed it since the last write."""
    if STATE_FILE is None or not DIRTY.is_set():
        return
    with LOCK:
        DIRTY.clear()
        snapshot = {c: list(b) for c, b in CHANNELS.items()}
    tmp = f"{STATE_FILE}.tmp"
    with open(tmp, "w", encoding="utf-8") as f:
        json.dump(snapshot, f)
    os.replace(tmp, STATE_FILE)


class TooLarge(Exception):
    """A request body over `MAX_BODY_BYTES`."""


class Relay(BaseHTTPRequestHandler):
    protocol_version = "HTTP/1.1"

    def _send(self, status: int, payload: dict) -> None:
        body = json.dumps(payload).encode()
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(body)))
        self.end_headers()
        self.wfile.write(body)

    def _linger(self) -> None:
        """Lets a client still sending an oversize body read the refusal.

        Closing with request bytes unread makes the kernel reset the
        connection, and a client mid-write then sees the reset rather than the
        413 already sent. So the write side is shut first and what the client
        is still sending is read and discarded — at most `LINGER_BYTES`, for at
        most `LINGER_SECONDS` — before the connection closes. The caps keep a
        stranger from holding a thread by never finishing a body.
        """
        try:
            self.wfile.flush()
            self.connection.shutdown(socket.SHUT_WR)
            self.connection.settimeout(LINGER_SECONDS)
            discarded = 0
            while discarded < LINGER_BYTES:
                chunk = self.connection.recv(65536)
                if not chunk:
                    break
                discarded += len(chunk)
        except OSError:
            pass

    def _channel(self) -> str | None:
        parts = self.path.strip("/").split("/")
        if len(parts) != 2 or parts[0] != "c" or not CHANNEL.match(parts[1]):
            return None
        return parts[1]

    def do_GET(self) -> None:  # noqa: N802
        channel = self._channel()
        if channel is None:
            self._send(404, {"error": "not a channel"})
            return
        with LOCK:
            blobs = list(CHANNELS.get(channel, []))
        self._send(200, {"blobs": blobs})

    def _read_body(self) -> bytes:
        """The request body, however the client framed it, or `TooLarge`.

        A client that sets no `Content-Length` sends chunked, which
        `BaseHTTPRequestHandler` does not decode. Reading zero bytes there and
        calling the result malformed would refuse every push from a client
        that streams — which `dart:io` does by default. The body is always
        drained, even on a refusal: answering with bytes still unread leaves a
        keep-alive connection out of step and the next request reads this
        reply. A body over `MAX_BODY_BYTES` is the exception: it is not drained,
        and the caller closes the connection instead.
        """
        if self.headers.get("Transfer-Encoding", "").lower() == "chunked":
            chunks = []
            total = 0
            while True:
                size = int(self.rfile.readline().split(b";")[0] or b"0", 16)
                if size == 0:
                    self.rfile.readline()
                    break
                total += size
                if total > MAX_BODY_BYTES:
                    raise TooLarge
                chunks.append(self.rfile.read(size))
                self.rfile.readline()
            return b"".join(chunks)
        length = int(self.headers.get("Content-Length", "0") or 0)
        if length > MAX_BODY_BYTES:
            raise TooLarge
        return self.rfile.read(length)

    def do_POST(self) -> None:  # noqa: N802
        global HELD_CHARS
        try:
            raw = self._read_body()
        except TooLarge:
            self.close_connection = True
            self._send(413, {"error": f"a body over {MAX_BODY_BYTES} bytes"})
            self._linger()
            return
        channel = self._channel()
        if channel is None:
            self._send(404, {"error": "not a channel"})
            return
        try:
            body = json.loads(raw or b"{}")
            blobs = body["blobs"]
            if not isinstance(blobs, list) or any(
                not isinstance(b, str) for b in blobs
            ):
                raise ValueError("blobs is a list of strings")
        except Exception:
            self._send(400, {"error": "not a push"})
            return
        for blob in blobs:
            if len(blob) > MAX_BLOB_CHARS:
                self._send(413, {"error": f"a blob over {MAX_BLOB_CHARS} characters"})
                return
        with LOCK:
            held = CHANNELS.get(channel, [])
            # A peer that pushes what it already pushed is the ordinary case,
            # not an error: sync is idempotent and a relay that grew on every
            # retry would punish a flaky connection.
            seen = set(held)
            fresh = []
            for blob in blobs:
                if blob not in seen:
                    seen.add(blob)
                    fresh.append(blob)
            grow = sum(len(b) for b in fresh)
            if HELD_CHARS + grow > MAX_HELD_CHARS:
                self._send(507, {"error": "the relay is full"})
                return
            CHANNELS[channel] = held + fresh
            HELD_CHARS += grow
            held = CHANNELS[channel]
            added = len(fresh)
            if added and STATE_FILE is not None:
                DIRTY.set()
        # The channel and the counts, never a blob.
        print(f"{channel[:12]}… +{added} of {len(blobs)}, holds {len(held)}",
              flush=True)
        self._send(200, {"ok": True})

    def log_message(self, *args) -> None:  # noqa: D102
        pass


def main() -> None:
    global MAX_BODY_BYTES, MAX_HELD_CHARS, STATE_FILE
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--port", type=int, default=39300)
    parser.add_argument("--host", default="127.0.0.1")
    parser.add_argument("--max-body", type=int, default=MAX_BODY_BYTES)
    parser.add_argument("--max-held", type=int, default=MAX_HELD_CHARS)
    parser.add_argument("--state-file")
    parser.add_argument("--idle-timeout", type=float, default=30.0)
    args = parser.parse_args()
    # Read by the handler's setup: a connection silent this long is closed.
    Relay.timeout = args.idle_timeout
    MAX_BODY_BYTES, MAX_HELD_CHARS = args.max_body, args.max_held
    STATE_FILE = args.state_file
    if STATE_FILE is not None and os.path.exists(STATE_FILE):
        load_state(STATE_FILE)
        print(f"loaded {len(CHANNELS)} channels from {STATE_FILE}", flush=True)
    server = ThreadingHTTPServer((args.host, args.port), Relay)
    # The port actually bound: `--port 0` asks the OS for a free one, and a
    # caller learns which from this line rather than guessing and probing.
    print(f"relay on http://{args.host}:{server.server_address[1]}", flush=True)
    if STATE_FILE is not None:
        threading.Thread(target=saver, daemon=True).start()
    # Asked to stop, it stops through `finally`, which writes what the last
    # interval's pushes added.
    signal.signal(signal.SIGTERM, lambda *_: sys.exit(0))
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        flush()


if __name__ == "__main__":
    main()
