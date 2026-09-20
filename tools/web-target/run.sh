#!/usr/bin/env bash
# Asserts that this package still refuses to compile for a JavaScript target.
#
# Dart's `int` is a 64-bit integer on the VM and an IEEE-754 double under
# dart2js, exact only to 2^53-1 = 9007199254740991, which is sixteen digits.
# This library's amounts are 64-bit by design: SPEC.md §2.2 bounds them at
# i64::MAX, nineteen digits, and §8.4 admits a fiat count of eighteen. Neither
# survives the conversion.
#
# Compiling anyway would not fail loudly. It would silently round every amount
# above 2^53, so a bill would settle to a number nobody agreed to. Refusing to
# build is the correct behaviour, and this lane exists to notice if that ever
# stops happening.
#
# Exit status is 1 when the package compiles, or when it fails for a reason
# other than the integer literals.

set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"; rm -f "$root/dart/tool/_web_target_probe.dart"' EXIT

dart_bin="${DART:-dart}"

cat > "$root/dart/tool/_web_target_probe.dart" <<'PROBE'
// Written by tools/web-target/run.sh and deleted after it runs.
import 'package:splitz_core/splitz_core.dart';

void main() {
  print(allocate(100, [1, 1, 1]));
  print(maxAmount);
  print(maxConvertibleMinorUnits);
}
PROBE

set +e
(cd "$root/dart" && "$dart_bin" compile js -o "$work/out.js" tool/_web_target_probe.dart) \
  > "$work/log" 2>&1
status=$?
set -e

if [ "$status" -eq 0 ]; then
  echo "COMPILED, and it must not: this package's amounts are 64-bit and a"
  echo "JavaScript number is exact only to 2^53-1. Every amount above that"
  echo "would round silently."
  exit 1
fi

literals="$(grep -c "can't be represented exactly in JavaScript" "$work/log" || true)"
if [ "$literals" -eq 0 ]; then
  echo "refused, but not for the reason this lane checks:"
  sed -n '1,12p' "$work/log"
  exit 1
fi

echo "refused, as expected: $literals integer literal(s) a JS number cannot hold"
grep -o "The integer literal [0-9]* can't" "$work/log" | sort -u | sed 's/^/  /'
exit 0
