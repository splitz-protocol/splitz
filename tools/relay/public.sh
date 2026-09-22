#!/usr/bin/env bash
# The bill relay, reachable from a phone on any network, for the length of a
# run.
#
#   tools/relay/public.sh [port]
#
# Starts `server.py` on loopback and puts a Cloudflare quick tunnel in front of
# it, then prints the HTTPS origin and the `--dart-define` a build takes. HTTPS
# is the point: a LAN address is plain HTTP, which a wallet reaches only
# through a per-platform cleartext exception, and this needs none.
#
# The origin is public for as long as the script runs. The relay holds only
# SHA-256 channel names and ciphertext (server.py), and is bounded in what one
# request and the whole store may hold. Ctrl-C stops both processes; a restart
# gets a new origin and an empty store, and devices re-push on their next sync.
#
# Needs `cloudflared` (`brew install cloudflared`). A quick tunnel takes no
# account and has no uptime guarantee.
set -euo pipefail

port="${1:-39300}"
here="$(cd "$(dirname "$0")" && pwd)"
command -v cloudflared >/dev/null || {
  echo "cloudflared not found: brew install cloudflared" >&2
  exit 2
}

log="$(mktemp -t splitz-tunnel)"
relay_pid=""
tunnel_pid=""
stop() {
  [ -n "$tunnel_pid" ] && kill "$tunnel_pid" 2>/dev/null || true
  [ -n "$relay_pid" ] && kill "$relay_pid" 2>/dev/null || true
  rm -f "$log"
}
trap stop EXIT INT TERM

python3 "$here/server.py" --port "$port" &
relay_pid=$!

for _ in $(seq 1 50); do
  curl -fsS "http://127.0.0.1:$port/c/$(printf '0%.0s' $(seq 1 64))" >/dev/null 2>&1 && break
  sleep 0.1
done
curl -fsS "http://127.0.0.1:$port/c/$(printf '0%.0s' $(seq 1 64))" >/dev/null || {
  echo "the relay did not come up on $port" >&2
  exit 1
}

cloudflared tunnel --no-autoupdate --url "http://127.0.0.1:$port" >"$log" 2>&1 &
tunnel_pid=$!

origin=""
for _ in $(seq 1 300); do
  origin="$(grep -oE 'https://[a-z0-9-]+\.trycloudflare\.com' "$log" | head -1 || true)"
  [ -n "$origin" ] && break
  kill -0 "$tunnel_pid" 2>/dev/null || break
  sleep 0.1
done
if [ -z "$origin" ]; then
  echo "the tunnel gave no origin; cloudflared said:" >&2
  cat "$log" >&2
  exit 1
fi

# The origin exists before the edge routes it. Wait until a request through it
# reaches this relay, so the line below is only printed once it is true.
channel="$(printf '0%.0s' $(seq 1 64))"
for _ in $(seq 1 60); do
  curl -fsS "$origin/c/$channel" 2>/dev/null | grep -q '"blobs"' && break
  sleep 1
done
curl -fsS "$origin/c/$channel" 2>/dev/null | grep -q '"blobs"' || {
  echo "the tunnel at $origin never reached the relay" >&2
  exit 1
}

echo "relay:  $origin"
echo "build:  --dart-define=SPLITS_RELAY_URL=$origin"
wait "$relay_pid"
