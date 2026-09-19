#!/usr/bin/env bash
# Brings up a regtest chain, funds a wallet on it, and broadcasts a
# transaction built from a splitz payment request.
#
# Every other lane in this repository stops at the URI. This one carries it the
# rest of the way and reads the recipient's balance afterwards, which is the
# only check that answers "does the money arrive".
#
# Usage: tools/regtest/run.sh [up|down|prove]
set -euo pipefail

here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$here"

rpc() {
  curl -s --data "{\"jsonrpc\":\"2.0\",\"id\":1,\"method\":\"$1\",\"params\":${2:-[]}}" \
    -H 'Content-Type: application/json' http://127.0.0.1:18232/
}

height() { rpc getblockcount | sed 's/.*"result"://; s/}//'; }

case "${1:-up}" in
down)
  docker compose down -v
  ;;

up)
  echo "== deriving the address the chain mines to =="
  miner="$(cargo run --quiet --manifest-path prover/Cargo.toml -- miner-address)"
  echo "   $miner"
  sed "s|@MINER_ADDRESS@|$miner|" zebrad.toml.in > zebrad.toml

  echo "== starting the node =="
  docker compose up -d zebra
  for _ in $(seq 40); do
    rpc getblockcount | grep -q result && break
    sleep 2
  done
  echo "   height $(height)"

  # Coinbase matures after 100 blocks, so the wallet cannot spend before then.
  echo "== mining past coinbase maturity =="
  rpc generate '[110]' > /dev/null
  echo "   height $(height)"

  echo "== starting lightwalletd =="
  docker compose up -d lightwalletd
  ;;

prove)
  cargo run --quiet --manifest-path prover/Cargo.toml -- prove
  ;;

*)
  echo "usage: $0 [up|down|prove]" >&2
  exit 2
  ;;
esac
