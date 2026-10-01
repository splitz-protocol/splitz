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
# What refuses is dart2js itself, on the integer literals it cannot represent:
# 9223372036854775807, i64::MAX, which bounds every amount (§2.2). Nothing else
# in the package keeps the refusal, so the lane requires dart2js to refuse it
# at `maxAmount`'s own declaration: the same digits elsewhere would keep the
# lane green after the bound was rewritten as arithmetic.
#
# Exit status is 1 when the package compiles, or when it fails anywhere but
# that declaration.

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

named="$(grep -oE "integer literal -?[0-9x]+ can't" "$work/log" | sort -u || true)"
# dart2js quotes the source line under each refusal. The one that matters is
# the declaration of the bound itself: the same digits elsewhere in the
# package would keep the lane green after maxAmount became arithmetic.
if ! grep -qx "const int maxAmount = 9223372036854775807;" "$work/log"; then
  echo "refused for $literals literal(s), but not at maxAmount's declaration,"
  echo "which bounds every amount:"
  printf '%s\n' "$named" | sed 's/^/  /'
  exit 1
fi
echo "refused, as expected: $literals integer literal(s) a JS number cannot hold"
printf '%s\n' "$named" | sed 's/^/  /'
exit 0
