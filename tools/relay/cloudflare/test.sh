#!/usr/bin/env bash
# The hosted relay held to the Python relay's answers.
#
# Runs splitz_host/test/relay_origin_test.dart twice: against
# tools/relay/server.py, then against this Worker under `wrangler dev`, which
# runs it locally with no account. Both must pass the same tests. Then
# bounds.py holds the Worker to the per-address and expiry bounds only it
# keeps.
#
#     tools/relay/cloudflare/test.sh
#     SPLITZ_RELAY_ORIGIN=https://splitz-relay.<account>.workers.dev \
#       tools/relay/cloudflare/test.sh      # the deployed one, instead of local
#
# Needs Node for `npx wrangler`. Not run by CI: the runner has no wrangler.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# One exact version, so a run is not whatever npm published that morning.
wrangler="4.145.0"
root="$(cd "$here/../../.." && pwd)"
work="$(mktemp -d)"
pids=()
port=""
# `npx` runs wrangler as node processes under it, which killing `npx` leaves
# running; they are found by the port they serve.
stop() {
  kill ${pids[@]+"${pids[@]}"} 2>/dev/null || true
  if [ -n "$port" ]; then pkill -f "wrangler.* dev --port $port" 2>/dev/null || true; fi
  rm -rf "$work"
}
trap stop EXIT

# Both local relays answer 16-byte pages, so every fetch the test makes spans
# pages and the two are held to the same page edges.
page=16
origin_test() {
  (cd "$root/splitz_host" && SPLITZ_RELAY_ORIGIN="$1" SPLITZ_RELAY_PAGE_BYTES="${2:-}" \
    dart test test/relay_origin_test.dart)
}

if [ -n "${SPLITZ_RELAY_ORIGIN:-}" ]; then
  echo "== deployed relay at $SPLITZ_RELAY_ORIGIN"
  origin_test "$SPLITZ_RELAY_ORIGIN"
  exit 0
fi

python3 "$root/tools/relay/server.py" --port 0 --page-bytes "$page" >"$work/py.log" 2>&1 &
pids+=($!)
for _ in $(seq 50); do
  py="$(sed -n 's#^relay on \(http://127\.0\.0\.1:[0-9]*\)$#\1#p' "$work/py.log")"
  [ -n "$py" ] && break
  sleep 0.1
done
echo "== server.py at $py"
origin_test "$py" "$page"

port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')"
(cd "$here" && npx --yes "wrangler@$wrangler" dev --port "$port" --ip 127.0.0.1 \
  --persist-to "$work/state" --var LEGACY_UNSTAMPED:0 --var "PAGE_BYTES:$page") >"$work/worker.log" 2>&1 &
pids+=($!)
for _ in $(seq 120); do
  grep -q "Ready on" "$work/worker.log" && break
  sleep 1
done
grep -q "Ready on" "$work/worker.log" || { cat "$work/worker.log" >&2; exit 1; }
echo "== the Worker at http://127.0.0.1:$port"
origin_test "http://127.0.0.1:$port" "$page"
# With LEGACY_UNSTAMPED off, a push with no work is 428 and names the base.
unstamped="$(curl -s -o /dev/stdout -w ' %{http_code}' -X POST \
  --data '{"blobs":["x"]}' "http://127.0.0.1:$port/c/$(printf 'a%.0s' $(seq 64))")"
echo "unstamped: $unstamped"
[ "$unstamped" = '{"error":"a push carries work","work":16} 428' ] || exit 1
pkill -f "wrangler.* dev --port $port" 2>/dev/null || true

# The bounds a public origin adds, with small figures so a run reaches them.
port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')"
(cd "$here" && npx --yes "wrangler@$wrangler" dev --port "$port" --ip 127.0.0.1 \
  --persist-to "$work/bounded" \
  --var MAX_SOURCE_CHARS:8192 --var EXPIRE_DAYS:0.00005 \
  --var LEGACY_UNSTAMPED:1) >"$work/bounded.log" 2>&1 &
pids+=($!)
for _ in $(seq 120); do
  grep -q "Ready on" "$work/bounded.log" && break
  sleep 1
done
grep -q "Ready on" "$work/bounded.log" || { cat "$work/bounded.log" >&2; exit 1; }
echo "== the Worker's own bounds at http://127.0.0.1:$port"
python3 "$here/bounds.py" "http://127.0.0.1:$port"
echo "== both relays give the same answers"
