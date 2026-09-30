#!/usr/bin/env bash
# The hosted relay held to the Python relay's answers.
#
# Runs splitz_host/test/relay_origin_test.dart twice: against
# tools/relay/server.py, then against this Worker under `wrangler dev`, which
# runs it locally with no account. Both must pass the same tests.
#
#     tools/relay/cloudflare/test.sh
#     SPLITZ_RELAY_ORIGIN=https://splitz-relay.<account>.workers.dev \
#       tools/relay/cloudflare/test.sh      # the deployed one, instead of local
#
# Needs Node for `npx wrangler`. Not run by CI: the runner has no wrangler.
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
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

origin_test() {
  (cd "$root/splitz_host" && SPLITZ_RELAY_ORIGIN="$1" dart test test/relay_origin_test.dart)
}

if [ -n "${SPLITZ_RELAY_ORIGIN:-}" ]; then
  echo "== deployed relay at $SPLITZ_RELAY_ORIGIN"
  origin_test "$SPLITZ_RELAY_ORIGIN"
  exit 0
fi

python3 "$root/tools/relay/server.py" --port 0 >"$work/py.log" 2>&1 &
pids+=($!)
for _ in $(seq 50); do
  py="$(sed -n 's#^relay on \(http://127\.0\.0\.1:[0-9]*\)$#\1#p' "$work/py.log")"
  [ -n "$py" ] && break
  sleep 0.1
done
echo "== server.py at $py"
origin_test "$py"

port="$(python3 -c 'import socket; s=socket.socket(); s.bind(("127.0.0.1",0)); print(s.getsockname()[1])')"
(cd "$here" && npx --yes wrangler@4 dev --port "$port" --ip 127.0.0.1 \
  --persist-to "$work/state") >"$work/worker.log" 2>&1 &
pids+=($!)
for _ in $(seq 120); do
  grep -q "Ready on" "$work/worker.log" && break
  sleep 1
done
grep -q "Ready on" "$work/worker.log" || { cat "$work/worker.log" >&2; exit 1; }
echo "== the Worker at http://127.0.0.1:$port"
origin_test "http://127.0.0.1:$port"
echo "== both relays give the same answers"
