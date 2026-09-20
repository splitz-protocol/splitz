#!/usr/bin/env bash
# A Kotlin wallet, written against the generated binding and nothing else.
#
# A binding that compiles is not a binding that carries a callback. This builds
# the cdylib, generates Kotlin from it, and runs a consumer that implements all
# seven of SPEC.md §15's interfaces and drives one bill across two devices —
# opened, joined, shared by invite, split, priced, settled, confirmed and read
# back as a history — calling into Kotlin for storage, secrets, the clock,
# randomness, the relay and the send at every step.
#
# Needs a Kotlin compiler and JNA. Point KOTLINC and JNA_JAR at them, or let
# this find Android Studio's kotlinc and fetch JNA into the work directory.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
work="${FFI_WORK:-$(mktemp -d)}"
jna_version="5.17.0"

# Pinned, with the checksum the release publishes: this downloads a compiler
# and runs it, so which one is not left to whatever a runner happens to have.
kotlin_version="2.4.20"
kotlin_sha256="59e9ca74c7904ef2c122b12114937673ccce68de820a663f0ed66ccf8799e0b7"

kotlinc="${KOTLINC:-}"
if [ -z "$kotlinc" ]; then
  for candidate in \
    "$(command -v kotlinc || true)" \
    "/Applications/Android Studio.app/Contents/plugins/Kotlin/kotlinc/bin/kotlinc"; do
    if [ -n "$candidate" ] && [ -x "$candidate" ]; then kotlinc="$candidate"; break; fi
  done
fi
if [ -z "$kotlinc" ]; then
  zip="$work/kotlin-compiler.zip"
  curl -sfL -o "$zip" \
    "https://github.com/JetBrains/kotlin/releases/download/v$kotlin_version/kotlin-compiler-$kotlin_version.zip"
  # `sha256sum` on Linux, `shasum` on macOS. A download run as a compiler
  # is checked against the checksum its publisher states.
  if command -v sha256sum >/dev/null; then
    echo "$kotlin_sha256  $zip" | sha256sum -c - >/dev/null
  else
    echo "$kotlin_sha256  $zip" | shasum -a 256 -c - >/dev/null
  fi
  (cd "$work" && unzip -q -o "$zip")
  kotlinc="$work/kotlinc/bin/kotlinc"
  chmod +x "$kotlinc"
fi

stdlib="${KOTLIN_STDLIB:-$(dirname "$kotlinc")/../lib/kotlin-stdlib.jar}"
if [ ! -f "$stdlib" ]; then
  echo "no kotlin-stdlib.jar beside $kotlinc: set KOTLIN_STDLIB" >&2
  exit 2
fi

jna="${JNA_JAR:-$work/jna.jar}"
if [ ! -f "$jna" ]; then
  # 5.12 is the floor: the generated bindings use `com.sun.jna.internal.Cleaner`.
  curl -sfL -o "$jna" \
    "https://repo1.maven.org/maven2/net/java/dev/jna/jna/$jna_version/jna-$jna_version.jar"
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

(cd "$root/rust" && "${CARGO:-cargo}" run --quiet --bin uniffi-bindgen -p splitz-ffi -- \
  generate --library "$library" --language kotlin --no-format --out-dir "$work/kt")

"$kotlinc" -nowarn -classpath "$jna" \
  "$work/kt/uniffi/splitz_ffi/splitz_ffi.kt" "$root/tools/ffi/kotlin/Consumer.kt" \
  -d "$work/classes"

java -cp "$work/classes:$jna:$stdlib" -Djna.library.path="$lib" ConsumerKt
