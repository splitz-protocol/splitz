# A live tools/relay/server.py for a consumer lane, sourced rather than run.
#
#     . "$root/tools/ffi/relay.sh"
#     relay_up "$root" "$work"     # sets RELAY_ORIGIN and RELAY_DOWN_ORIGIN
#
# RELAY_ORIGIN is the running relay. RELAY_DOWN_ORIGIN is an origin nothing
# answers, for the lane's "relay is down" case. The relay binds RELAY_PORT,
# or a port the OS picks when that is unset, and its own first line names the
# port it bound. The caller kills RELAY_PID on exit, in its own EXIT trap.

relay_up() {
  local root="$1" work="$2" log="$2/relay.log"
  python3 "$root/tools/relay/server.py" --port "${RELAY_PORT:-0}" >"$log" 2>&1 &
  RELAY_PID=$!
  local i
  for i in $(seq 1 100); do
    RELAY_ORIGIN="$(sed -n 's#^relay on \(http://127\.0\.0\.1:[0-9]*\)$#\1#p' "$log")"
    [ -n "$RELAY_ORIGIN" ] && break
    if ! kill -0 "$RELAY_PID" 2>/dev/null; then
      echo "the relay exited before listening:" >&2
      cat "$log" >&2
      return 1
    fi
    sleep 0.1
  done
  if [ -z "$RELAY_ORIGIN" ]; then
    echo "the relay did not report a port:" >&2
    cat "$log" >&2
    return 1
  fi
  # A port the OS just handed out and nothing took, so a request to it is
  # refused at connect. Not a well-known unused port such as 1: the Fetch
  # standard blocks those, and a blocked port fails before any connection,
  # which is not the case being tested.
  RELAY_DOWN_ORIGIN="${RELAY_DOWN_ORIGIN:-http://127.0.0.1:$(python3 -c '
import socket
s = socket.socket()
s.bind(("127.0.0.1", 0))
print(s.getsockname()[1])
s.close()')}"
  export RELAY_ORIGIN RELAY_DOWN_ORIGIN
  echo "relay: $RELAY_ORIGIN (down: $RELAY_DOWN_ORIGIN)"
}
