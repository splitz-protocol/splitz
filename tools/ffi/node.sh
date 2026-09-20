#!/usr/bin/env bash
# A JavaScript wallet, written against the generated Node package.
#
# A third language over the same foreign library: it implements all seven of
# SPEC.md §15's interfaces and drives one bill from opening it to a confirmed
# payment. Kotlin proves the binding carries a callback; this proves the
# generator is not the only one that can, and that the surface reads the same
# from a language with no static types.
#
# Needs node and npm. `uniffi-bindgen-node-js` is installed into the work
# directory rather than the machine, pinned: it and every other third-party
# generator target uniffi 0.31, which is why this crate pins that version.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
work="${FFI_WORK:-$(mktemp -d)}"
generator_version="0.0.16"

if ! command -v node >/dev/null || ! command -v npm >/dev/null; then
  echo "no node or npm" >&2
  exit 2
fi

(cd "$root/rust" && "${CARGO:-cargo}" build --quiet -p splitz-ffi)
lib="$root/rust/target/debug"
for name in libsplitz_ffi.dylib libsplitz_ffi.so splitz_ffi.dll; do
  [ -f "$lib/$name" ] && library="$lib/$name" && break
done
if [ -z "${library:-}" ]; then
  echo "no splitz-ffi library under $lib" >&2
  exit 1
fi

generator="${UNIFFI_BINDGEN_NODE_JS:-$work/tools/bin/uniffi-bindgen-node-js}"
if [ ! -x "$generator" ]; then
  "${CARGO:-cargo}" install uniffi-bindgen-node-js \
    --version "$generator_version" --root "$work/tools" --quiet
fi

rm -rf "$work/node"
# Run from the workspace: the generator reads `cargo metadata` from its own
# working directory to resolve the crate.
(cd "$root/rust" && "$generator" generate --out-dir "$work/node" "$library")
cp "$root/tools/ffi/node/consumer.mjs" "$work/node/"
(cd "$work/node" && npm install --silent --no-fund --no-audit)

node "$work/node/consumer.mjs" "$library"
