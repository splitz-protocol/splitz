#!/usr/bin/env bash
# The two host implementations, on one operation list, diffed.
#
# The protocol's corpus cannot reach this layer: a vector carries no private
# key, so nothing in `vectors/` covers a curve operation, a derived identity
# or a key a wallet was handed. This generates the inputs instead and asks both
# implementations, which is the only check that would notice one of them being
# quietly wrong on its own.
#
# Usage: tools/differential/host.sh [seed] [count]
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
seed="${1:-1}"
count="${2:-400}"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

python3 "$root/tools/differential/host_generate.py" "$seed" "$count" > "$work/ops.jsonl"

(cd "$root/rust" && "${CARGO:-cargo}" run --quiet -p splitz-host --example host_differential) \
  < "$work/ops.jsonl" > "$work/rust.jsonl"
(cd "$root/splitz_host" && dart run tool/host_differential.dart) \
  < "$work/ops.jsonl" > "$work/dart.jsonl"

python3 - "$work/ops.jsonl" "$work/rust.jsonl" "$work/dart.jsonl" <<'PY'
import collections
import json
import sys

ops, rust, dart = (
    [json.loads(line) for line in open(path, encoding="utf-8") if line.strip()]
    for path in sys.argv[1:4]
)
if not (len(ops) == len(rust) == len(dart)):
    print(f"lengths differ: {len(ops)} ops, {len(rust)} rust, {len(dart)} dart")
    raise SystemExit(1)

seen = collections.Counter(op["op"] for op in ops)
bad = [(i, op, r, d) for i, (op, r, d) in enumerate(zip(ops, rust, dart)) if r != d]
for name, n in sorted(seen.items()):
    print(f"  {name:20} {n:5}")
if bad:
    print(f"\n{len(bad)} OPERATION(S) THE TWO IMPLEMENTATIONS ANSWER DIFFERENTLY")
    for i, op, r, d in bad[:5]:
        print(f"  #{i} {json.dumps(op)[:160]}")
        print(f"     rust {json.dumps(r)[:160]}")
        print(f"     dart {json.dumps(d)[:160]}")
    raise SystemExit(1)
# A run that compared nothing would print agreement and mean it about no one.
if not ops:
    print("no operations — this lane is checking nothing")
    raise SystemExit(1)
print(f"\n{len(ops)} operations, both host implementations agree")
PY
