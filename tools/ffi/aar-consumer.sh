#!/usr/bin/env bash
# An Android wallet depending on the packaged AAR, driving one whole bill.
#
#     tools/ffi/aar-consumer.sh
#
# `tools/ffi/kotlin.sh` compiles the generated Kotlin directly. This does not:
# the only splitz on the consumer's classpath is
# `dist/android/splitz/build/outputs/aar/splitz-release.aar`, declared the way
# a wallet declares one, so a symbol the package fails to carry is a compile
# error here.
#
# WHICH NATIVE BINARY RUNS. The bill runs as a JVM unit test, and a JVM loads
# the HOST cdylib — `rust/target/release/libsplitz_ffi.{dylib,so}`. The four
# `jni/<abi>/libsplitz_ffi.so` the AAR carries are Android ELF objects no
# desktop JVM can open; this lane lists them and their architectures but does
# not execute them. Running those needs a device or an emulator.
#
# Needs Gradle, a JDK, and an Android SDK (ANDROID_HOME or ~/Library/Android/sdk)
# with the platform AGP compiles against. Building the AAR itself additionally
# needs the NDK and the four Rust Android targets.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
work="${AAR_WORK:-$(mktemp -d)}"
gradle="${GRADLE:-gradle}"
aar="$root/dist/android/splitz/build/outputs/aar/splitz-release.aar"

if [ ! -f "$aar" ]; then
  echo "no AAR yet — packaging one"
  "$root/tools/package/android.sh"
  (cd "$root/dist/android/splitz" && "$gradle" --quiet assembleRelease)
fi
if [ ! -f "$aar" ]; then
  echo "no AAR at $aar: run tools/package/android.sh, then" >&2
  echo "  (cd $root/dist/android/splitz && gradle assembleRelease)" >&2
  exit 2
fi

echo "consuming: $aar"
echo "it carries, per ABI:"
for abi in arm64-v8a armeabi-v7a x86 x86_64; do
  entry="jni/$abi/libsplitz_ffi.so"
  if unzip -p "$aar" "$entry" > "$work/abi.so" 2>/dev/null && [ -s "$work/abi.so" ]; then
    printf '  %-12s %s\n' "$abi" "$(file -b "$work/abi.so" | cut -d, -f1-2)"
  else
    echo "$aar is missing $entry" >&2
    exit 1
  fi
done
rm -f "$work/abi.so"
echo "  none of those is loaded below: a JVM run loads the host library."

# The host cdylib the JVM run binds to. The AAR's Kotlin was generated from a
# release build of the same crate, so the checksums the binding asserts match.
(cd "$root/rust" && "${CARGO:-cargo}" build --quiet --release -p splitz-ffi)
libdir="$root/rust/target/release"
for name in libsplitz_ffi.dylib libsplitz_ffi.so splitz_ffi.dll; do
  [ -f "$libdir/$name" ] && host="$libdir/$name" && break
done
if [ -z "${host:-}" ]; then
  echo "no host splitz-ffi library under $libdir" >&2
  exit 1
fi
echo "host library: $host"

# The consumer builds outside the repository: its Gradle caches and outputs are
# not this tree's.
project="$work/consumer"
mkdir -p "$project"
cp -R "$root/tools/ffi/aar/." "$project/"
sdk="${ANDROID_HOME:-${ANDROID_SDK_ROOT:-$HOME/Library/Android/sdk}}"
if [ ! -d "$sdk" ]; then
  echo "no Android SDK at $sdk: set ANDROID_HOME" >&2
  exit 2
fi
printf 'sdk.dir=%s\n' "$sdk" > "$project/local.properties"

echo
echo "building the consumer against the AAR and running the bill"
(cd "$project" && "$gradle" --console=plain testDebugUnitTest \
   "-PsplitzAar=$aar" "-PsplitzLibDir=$libdir")
