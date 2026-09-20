#!/usr/bin/env bash
# The generated binding, opened inside Flutter's test harness.
#
# `tools/ffi/dart.sh` drives the binding under `dart run`; this opens the same
# library from `flutter test`, which runs on the Flutter tester rather than the
# standalone VM. A wallet that cannot load the library there cannot hold the
# crate, so this is the precondition for a Flutter package wrapping
# `splitz-ffi` — and the cheapest place for it to break.
#
# The host library only. Cross-compiling the cdylib for a phone and bundling it
# is a wallet's build system, and nothing here does it.
#
# Needs Flutter. Set FLUTTER to pick one; `fvm flutter` is used when present.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
work="${FFI_WORK:-$(mktemp -d)}"
generator_version="0.1.3"

flutter="${FLUTTER:-}"
if [ -z "$flutter" ]; then
  if command -v fvm >/dev/null && fvm flutter --version >/dev/null 2>&1; then
    flutter="fvm flutter"
  elif command -v flutter >/dev/null; then
    flutter="flutter"
  else
    echo "no flutter" >&2
    exit 2
  fi
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

generator="${UNIFFI_BINDGEN_DART:-$work/tools/bin/uniffi-bindgen-dart}"
if [ ! -x "$generator" ]; then
  "${CARGO:-cargo}" install uniffi-bindgen-dart \
    --version "$generator_version" --root "$work/tools" --quiet
fi

pkg="$work/flutter-consumer"
rm -rf "$pkg"
mkdir -p "$pkg/lib" "$pkg/test"
cat > "$pkg/pubspec.yaml" <<'YAML'
name: splitz_flutter_consumer
publish_to: none
environment:
  sdk: ^3.11.4
  flutter: ">=3.41.0"
dependencies:
  flutter:
    sdk: flutter
  ffi: ^2.1.0
dev_dependencies:
  flutter_test:
    sdk: flutter
YAML
# `--crate` is required: without it the generator looks up
# `ffi_uniffi_<name>_rustbuffer_*` where the library exports
# `ffi_<name>_rustbuffer_*`.
(cd "$root/rust" && "$generator" generate --crate splitz_ffi --out-dir "$pkg/lib" "$library")
cp "$root/tools/ffi/flutter/binding_test.dart" "$pkg/test/"
(cd "$pkg" && $flutter pub get > /dev/null)
# `flutter test` passes no arguments to a test, so the path arrives as a define.
(cd "$pkg" && $flutter test --dart-define=SPLITZ_LIBRARY="$library")
