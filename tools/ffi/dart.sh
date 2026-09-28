#!/usr/bin/env bash
# A Dart wallet over the generated binding.
#
# The third language to drive a whole bill, and the one the Dart Zcash wallets
# need. It works because nothing in `splitz-ffi` calls back: the published Dart
# generator does not implement uniffi's callback ABI — its vtable fields are
# alphabetical where uniffi's are in declaration order, and it passes `char*`
# where uniffi passes a `RustBuffer` by value — so a binding with callback
# interfaces compiles and then calls the wrong method. Without them, its output
# is correct and this runs unpatched.
#
# Two requirements the generator does not state: the crate must build with
# uniffi's `scaffolding-ffi-buffer-fns` feature, and `--crate` must be given,
# or it looks up `ffi_uniffi_<name>_rustbuffer_*` where the library exports
# `ffi_<name>_rustbuffer_*`.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
work="${FFI_WORK:-$(mktemp -d)}"
generator_version="0.1.3"

if ! command -v dart >/dev/null; then
  echo "no dart" >&2
  exit 2
fi

(cd "$root/rust" && "${CARGO:-cargo}" build --quiet -p splitz-ffi)
# shellcheck source=../package/source.sh
. "$root/tools/package/source.sh"
lib="$(cargo_target_dir "$root")/debug"
for name in libsplitz_ffi.dylib libsplitz_ffi.so splitz_ffi.dll; do
  [ -f "$lib/$name" ] && library="$lib/$name" && break
done
if [ -z "${library:-}" ]; then
  echo "no splitz-ffi library under $lib" >&2
  exit 1
fi

generator="${UNIFFI_BINDGEN_DART:-$work/tools/bin/uniffi-bindgen-dart}"
if [ ! -x "$generator" ]; then
  "${CARGO:-cargo}" install uniffi-bindgen-dart \
    --version "$generator_version" --root "$work/tools" --quiet
fi

pkg="$work/consumer"
rm -rf "$pkg"
mkdir -p "$pkg/lib" "$pkg/bin"
cat > "$pkg/pubspec.yaml" <<'YAML'
name: splitz_dart_consumer
publish_to: none
environment:
  sdk: ^3.5.0
dependencies:
  ffi: ^2.1.0
YAML
(cd "$root/rust" && "$generator" generate --crate splitz_ffi --out-dir "$pkg/lib" "$library")
cp "$root/tools/ffi/dart/consumer.dart" "$root/tools/ffi/dart/doc.dart" "$pkg/bin/"
(cd "$pkg" && dart pub get >/dev/null && dart analyze)
(cd "$pkg" && dart run bin/consumer.dart "$library")
# The sample INTEGRATING.md quotes, run here so the document's code is code
# that ran.
(cd "$pkg" && dart run bin/doc.dart "$library")
