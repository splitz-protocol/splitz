#!/usr/bin/env bash
# The generated Swift, compiled.
#
# A module that does not build is a binding no Swift wallet can reach, and the
# generator will produce one: a record field named for something the target
# language already puts on that type compiles in Rust and not in Swift or
# Kotlin. This catches that class before a wallet does.
#
# It does not drive the seam — `tools/ffi/kotlin.sh` does that. Needs swiftc.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

if ! command -v swiftc >/dev/null; then
  echo "no swiftc" >&2
  exit 2
fi

(cd "$root/rust" && "${CARGO:-cargo}" build --quiet -p splitz-ffi)
lib="$root/rust/target/debug"
for name in libsplitz_ffi.dylib libsplitz_ffi.so; do
  [ -f "$lib/$name" ] && library="$lib/$name" && break
done
if [ -z "${library:-}" ]; then
  echo "no splitz-ffi library under $lib" >&2
  exit 1
fi

(cd "$root/rust" && "${CARGO:-cargo}" run --quiet --bin uniffi-bindgen -p splitz-ffi -- \
  generate --library "$library" --language swift --out-dir "$work")

(cd "$work" && swiftc -emit-module -module-name splitz_ffi \
  -Xcc -fmodule-map-file=splitz_ffiFFI.modulemap -I . \
  -L "$lib" -lsplitz_ffi splitz_ffi.swift -o "$work/splitz_ffi.swiftmodule")

echo "the generated Swift module builds"
