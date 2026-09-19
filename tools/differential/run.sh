#!/usr/bin/env bash
# Runs the same operation list through every implementation and diffs their
# answers against each other.
#
# This is the lane the vector corpus cannot be: the corpus is generated from
# one reference, so it can only catch divergence on inputs that reference was
# written to emit. Anything decided by a standard library — string trimming,
# collation, canonical encoding of an untyped value — is invisible to it.
#
# Rust runs in both profiles. "Both implementations agree" means nothing at a
# numeric boundary unless the overflow-checked and unchecked builds both run.
#
# Usage: tools/differential/run.sh [seed] [count]
# Exit status is 1 when any two disagree, so it can gate a commit.

set -euo pipefail

seed="${1:-1}"
count="${2:-1200}"

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

echo "seed $seed, $count operations"
python3 "$root/tools/differential/generate.py" "$seed" "$count" > "$work/ops.jsonl"

dart_bin="${DART:-dart}"
cargo_bin="${CARGO:-cargo}"

(cd "$root/dart" && "$dart_bin" run tool/differential.dart) \
  < "$work/ops.jsonl" > "$work/dart.jsonl"

(cd "$root/rust" && "$cargo_bin" run --quiet --example differential) \
  < "$work/ops.jsonl" > "$work/rust-debug.jsonl"

(cd "$root/rust" && "$cargo_bin" run --quiet --release --example differential) \
  < "$work/ops.jsonl" > "$work/rust-release.jsonl"

python3 "$root/tools/differential/compare.py" \
  "$work/ops.jsonl" \
  "dart=$work/dart.jsonl" \
  "rust-debug=$work/rust-debug.jsonl" \
  "rust-release=$work/rust-release.jsonl"
