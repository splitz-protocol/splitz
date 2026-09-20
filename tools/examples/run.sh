#!/usr/bin/env bash
# Runs every program under dart/example/ and reads its exit code.
#
# `dart analyze` type-checks these files, which is why the integration guide's
# own consumer check compiled for as long as it did while throwing on its
# first fold: a sample that is only compiled proves the names exist, not that
# the calls in it are ones a caller may make in that order with those values.
#
# Each of these is a claim addressed to somebody outside this repository — the
# walkthrough in the README, the adapter a wallet writes against the seam, the
# sequence INTEGRATING.md sets out. A claim that does not run is not a claim
# this tree can back.
#
# Exit status is 1 when any example exits non-zero, so it can gate a commit.
#
# Usage: tools/examples/run.sh

set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
dart_bin="${DART:-dart}"

shopt -s nullglob
examples=("$root"/dart/example/*.dart)
if [ "${#examples[@]}" -eq 0 ]; then
  echo "no examples under dart/example/ — this lane is checking nothing"
  exit 1
fi

status=0
ran=0
for path in "${examples[@]}"; do
  name="$(basename "$path" .dart)"
  out="$(cd "$root/dart" && "$dart_bin" run "example/$name.dart" 2>&1)"
  code=$?
  if [ "$code" -eq 0 ]; then
    printf '  ok      %-16s %s\n' "$name" "$(echo "$out" | tail -1 | cut -c1-60)"
    ran=$((ran + 1))
  else
    status=1
    printf '  FAILED  %-16s exit %s\n' "$name" "$code"
    echo "$out" | sed -n '1,6p' | sed 's/^/            /'
  fi
done

# The Rust side has one too. `differential` is left out deliberately: it reads
# an operation list on stdin and is the differential lane's, not a sample.
cargo_bin="${CARGO:-cargo}"
for name in seam; do
  out="$(cd "$root/rust" && "$cargo_bin" run --quiet --example "$name" 2>&1)"
  code=$?
  if [ "$code" -eq 0 ]; then
    printf '  ok      %-16s %s\n' "rust:$name" "$(echo "$out" | tail -1 | cut -c1-60)"
    ran=$((ran + 1))
  else
    status=1
    printf '  FAILED  %-16s exit %s\n' "rust:$name" "$code"
    echo "$out" | sed -n '1,6p' | sed 's/^/            /'
  fi
done

if [ "$status" -eq 0 ]; then
  echo "$ran examples ran, all exited 0"
fi
exit "$status"
