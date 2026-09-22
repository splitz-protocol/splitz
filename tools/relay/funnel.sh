#!/usr/bin/env bash
# The bill relay at a stable HTTPS origin, for phones on any network.
#
#   tools/relay/funnel.sh [port]
#
# Starts `server.py` on loopback with a state file and exposes it through
# Tailscale Funnel on port 443. The origin is this machine's name in the
# tailnet (`<machine>.<tailnet>.ts.net`), so it is the same on every start: a
# phone built once with `SPLITS_RELAY_URL` keeps reaching it across restarts,
# and the state file keeps what it held.
#
# The phones need nothing from Tailscale; Funnel is reachable from the public
# internet. This machine needs the Tailscale app, logged in, with Funnel
# allowed for it in the tailnet's policy. Ctrl-C stops the relay and turns the
# funnel off.
#
#   SPLITZ_RELAY_STATE   the state file (default ~/.cache/splitz-relay/state.json)
set -euo pipefail

port="${1:-39300}"
here="$(cd "$(dirname "$0")" && pwd)"
state="${SPLITZ_RELAY_STATE:-$HOME/.cache/splitz-relay/state.json}"

# The app's binary before a `tailscale` on PATH: on macOS that is a shell
# launcher that runs the binary as a child, so the pid held below would be the
# launcher's and killing it would leave the funnel published.
if [ -x /Applications/Tailscale.app/Contents/MacOS/Tailscale ]; then
  ts=(env TAILSCALE_BE_CLI=1 /Applications/Tailscale.app/Contents/MacOS/Tailscale)
elif command -v tailscale >/dev/null; then
  ts=(tailscale)
else
  echo "tailscale not found: brew install --cask tailscale-app, then log in" >&2
  exit 2
fi

host="$("${ts[@]}" status --json | python3 -c \
  'import json,sys; print(json.load(sys.stdin)["Self"]["DNSName"].rstrip("."))')" || {
  echo "tailscale is not logged in: open the Tailscale app and sign in" >&2
  exit 2
}
[ -n "$host" ] || { echo "tailscale reported no DNS name for this machine" >&2; exit 2; }
origin="https://$host"

mkdir -p "$(dirname "$state")"
log="$(mktemp -t splitz-funnel)"
relay_pid=""
funnel_pid=""
stop() {
  if [ -n "$funnel_pid" ]; then
    pkill -P "$funnel_pid" 2>/dev/null || true
    kill "$funnel_pid" 2>/dev/null || true
  fi
  "${ts[@]}" funnel --https=443 off >/dev/null 2>&1 || true
  [ -n "$relay_pid" ] && kill "$relay_pid" 2>/dev/null || true
  rm -f "$log"
}
trap stop EXIT INT TERM

channel="$(printf '0%.0s' $(seq 1 64))"
python3 "$here/server.py" --port "$port" --state-file "$state" &
relay_pid=$!
for _ in $(seq 1 50); do
  curl -fsS "http://127.0.0.1:$port/c/$channel" >/dev/null 2>&1 && break
  sleep 0.1
done
curl -fsS "http://127.0.0.1:$port/c/$channel" >/dev/null || {
  echo "the relay did not come up on $port" >&2
  exit 1
}

# Foreground rather than --bg: a background funnel outlives this script and
# would keep publishing the port after the relay behind it has stopped.
"${ts[@]}" funnel --https=443 "localhost:$port" >"$log" 2>&1 &
funnel_pid=$!

# Printed only once a request through the public origin reaches this relay.
for _ in $(seq 1 60); do
  curl -fsS "$origin/c/$channel" 2>/dev/null | grep -q '"blobs"' && break
  kill -0 "$funnel_pid" 2>/dev/null || break
  sleep 1
done
curl -fsS "$origin/c/$channel" 2>/dev/null | grep -q '"blobs"' || {
  echo "the funnel at $origin never reached the relay; tailscale said:" >&2
  cat "$log" >&2
  exit 1
}

echo "relay:  $origin"
echo "state:  $state"
echo "build:  --dart-define=SPLITS_RELAY_URL=$origin"
wait "$relay_pid"
