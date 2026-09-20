#!/usr/bin/env python3
"""Serves development seed phrases on loopback, for the length of one run.

A phrase passed to a build as `--dart-define` is compiled into the binary,
printed by every tool that dumps a build's defines, and kept in whatever log
or screenshot the run produced. A phrase fetched from 127.0.0.1 while the run
is happening is in memory and nowhere else, and this process can be stopped
the moment the run ends.

    python3 tool/seed-driver.py <seed-file> --port 39200

The seed file is one phrase per line, blank lines ignored; line N is index N.
It is read once at startup and never written, copied or logged. Nothing here
prints a phrase: the log line for a request names the index and the account,
which is what a person reading a run needs and all they need.

Two routes, matching `SeedDriver` in lib/src/testing/seed_driver.dart:

    GET /health        -> {"ok": true, "wallets": <count>}
    GET /seed/<index>  -> {"seed": "<phrase>", "name": "<label>"}

It binds to 127.0.0.1 only. A simulator reaches loopback on the host; a
physical device does not, and that is the intended limit — a driver reachable
off the machine is a seed phrase on a network.
"""

from __future__ import annotations

import argparse
import json
import sys
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

# A label per index, for a person reading a run. The index is the identifier;
# a caller that wants its own names holds them itself, so no wallet of anyone's
# is described in this repository. `--names A,B,C` overrides, in index order.
NAMES: dict[int, str] = {}


def load(path: Path) -> list[tuple[str, str]]:
    """The phrases in `path`, each with the name it is filed under.

    Two shapes are accepted, because both are in use: one phrase per line, and
    a shell-style `NAME=phrase` file. In the second, a line whose value is not
    a phrase — an address, a height, a port — is left out rather than served,
    since handing one to a wallet's import produces a message about word
    counts that names the wrong thing.

    A phrase is recognised by shape, not by its name: BIP 39 admits 12, 15, 18,
    21 or 24 words, so anything else is not one.
    """
    if not path.is_file():
        sys.exit(f"no seed file at {path}")
    if path.stat().st_mode & 0o077:
        # Readable by somebody else on this machine. Said once, loudly: the
        # file holds spending authority for every wallet a run touches.
        print(
            f"warning: {path} is readable beyond its owner; chmod 600 it",
            file=sys.stderr,
        )

    found: list[tuple[str, str]] = []
    for number, raw in enumerate(path.read_text(encoding="utf-8").splitlines(), 1):
        line = raw.strip()
        if not line or line.startswith("#"):
            continue
        if line.startswith("export "):
            line = line[len("export "):].strip()

        name, _, value = line.partition("=")
        if not _:
            name, value = f"line {number}", line
        value = value.strip().strip("'").strip('"')

        if len(value.split()) in (12, 15, 18, 21, 24):
            found.append((name.strip(), value))

    if not found:
        sys.exit(f"{path} holds no phrases")
    return found


class Driver(BaseHTTPRequestHandler):
    phrases: list[tuple[str, str]] = []

    def _json(self, status: int, body: dict) -> None:
        payload = json.dumps(body).encode("utf-8")
        self.send_response(status)
        self.send_header("Content-Type", "application/json")
        self.send_header("Content-Length", str(len(payload)))
        # A phrase must not survive in a cache, an intermediary or a history.
        self.send_header("Cache-Control", "no-store")
        self.end_headers()
        self.wfile.write(payload)

    def do_GET(self) -> None:  # noqa: N802 — the base class names it
        if self.path == "/health":
            self._json(
                200,
                {
                    "ok": True,
                    "wallets": len(self.phrases),
                    # Names only. Which wallet is at which index is the one
                    # thing a run needs to check, and it is not a secret.
                    "keys": [key for key, _ in self.phrases],
                },
            )
            return

        if self.path.startswith("/seed/"):
            raw = self.path[len("/seed/") :]
            if not raw.isdigit():
                self._json(400, {"error": "index is a number"})
                return
            index = int(raw)
            if index >= len(self.phrases):
                self._json(404, {"error": f"no wallet at index {index}"})
                return
            key, phrase = self.phrases[index]
            self._json(
                200,
                {
                    "seed": phrase,
                    "name": NAMES.get(index, f"#{index}"),
                    # What the phrase is filed under, so a run can be checked
                    # against the file without anybody opening it.
                    "key": key,
                },
            )
            return

        self._json(404, {"error": "not a route"})

    def log_message(self, fmt: str, *args) -> None:
        # The default logs the request line, which carries the index. That is
        # fine; what must never reach a log is the phrase, and nothing here
        # writes one.
        sys.stderr.write(f"seed-driver: {fmt % args}\n")


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("seed_file", type=Path)
    parser.add_argument("--port", type=int, default=39200)
    args = parser.parse_args()

    Driver.phrases = load(args.seed_file)
    server = ThreadingHTTPServer(("127.0.0.1", args.port), Driver)
    print(
        f"seed-driver: {len(Driver.phrases)} wallets on "
        f"http://127.0.0.1:{args.port} — stop it when the run ends",
        file=sys.stderr,
    )
    try:
        server.serve_forever()
    except KeyboardInterrupt:
        pass
    finally:
        server.server_close()
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
