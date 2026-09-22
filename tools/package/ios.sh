#!/usr/bin/env bash
# The crate as an iOS wallet reaches it: one .xcframework, device and
# simulator, with the generated Swift beside it.
#
# A wallet cannot link a host-only cdylib. Apple requires a static archive per
# platform, and the simulator slice must carry both architectures, so the two
# simulator triples are lipo'd into one archive before the framework is built.
# Device and simulator cannot be lipo'd together: both are arm64, and `lipo`
# refuses two slices of one architecture. That is the whole reason
# .xcframework exists.
#
#     tools/package/ios.sh            # -> dist/ios/splitz_ffi.xcframework
#
# Needs Xcode and the three Apple targets. Nothing here is published; the
# output is the artefact a wallet vendors.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
out="$root/dist/ios"
DEVICE=aarch64-apple-ios
SIMS=(aarch64-apple-ios-sim x86_64-apple-ios)

if ! command -v xcodebuild >/dev/null; then
  echo "no xcodebuild" >&2
  exit 2
fi

for t in "$DEVICE" "${SIMS[@]}"; do
  if ! rustup target list --installed | grep -qx "$t"; then
    echo "missing rust target $t: rustup target add $t" >&2
    exit 2
  fi
done

rm -rf "$out"
mkdir -p "$out/headers" "$out/sim"

# One archive per platform. --release, because a wallet ships release.
for t in "$DEVICE" "${SIMS[@]}"; do
  echo "building $t"
  (cd "$root/rust" && "${CARGO:-cargo}" build --quiet --release \
     -p splitz-ffi --target "$t")
done

# The simulator slices are two architectures of one platform, so they become
# one fat archive. The device slice stays alone.
sim_archives=()
for t in "${SIMS[@]}"; do
  sim_archives+=("$root/rust/target/$t/release/libsplitz_ffi.a")
done
lipo -create -output "$out/sim/libsplitz_ffi.a" "${sim_archives[@]}"

# The Swift binding and the C header the framework must carry.
#
# uniffi reads the metadata by loading the library, so it is generated from a
# HOST build, not from a cross-compiled one the running machine cannot load.
# The metadata is the same either way: it comes from the source, not the
# target triple.
(cd "$root/rust" && "${CARGO:-cargo}" build --quiet --release -p splitz-ffi)
(cd "$root/rust" && "${CARGO:-cargo}" run --quiet --bin uniffi-bindgen \
   -p splitz-ffi -- generate \
   --library "$root/rust/target/release/libsplitz_ffi.dylib" \
   --language swift --out-dir "$out/swift")

cp "$out/swift"/*.h "$out/headers/"
# Xcode looks for `module.modulemap` by that exact name inside the headers
# directory; uniffi names it after the module.
cp "$out/swift"/*.modulemap "$out/headers/module.modulemap"

xcodebuild -create-xcframework \
  -library "$root/rust/target/$DEVICE/release/libsplitz_ffi.a" \
  -headers "$out/headers" \
  -library "$out/sim/libsplitz_ffi.a" \
  -headers "$out/headers" \
  -output "$out/splitz_ffi.xcframework"

echo
echo "xcframework: $out/splitz_ffi.xcframework"
lipo -info "$root/rust/target/$DEVICE/release/libsplitz_ffi.a" | sed 's/^/  device: /'
lipo -info "$out/sim/libsplitz_ffi.a" | sed 's/^/  sim:    /'
