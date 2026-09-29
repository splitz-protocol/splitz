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
# Taken before the build: the source this package is compiled from.
. "$root/tools/package/source.sh"
target_dir="$(cargo_target_dir "$root")"
stamp="$(rust_source_stamp "$root")"
scripts="$(script_source_stamp "$root" ios)"
DEVICE=aarch64-apple-ios
SIMS=(aarch64-apple-ios-sim x86_64-apple-ios)
# macOS too: this wallet ships a desktop build, and a package that declares
# the platform without carrying a slice for it fails at link time rather than
# at resolution, which is the worst moment to find out.
MACS=(aarch64-apple-darwin x86_64-apple-darwin)

if ! command -v xcodebuild >/dev/null; then
  echo "no xcodebuild" >&2
  exit 2
fi

for t in "$DEVICE" "${SIMS[@]}" "${MACS[@]}"; do
  if ! rustup target list --installed | grep -qx "$t"; then
    echo "missing rust target $t: rustup target add $t" >&2
    exit 2
  fi
done

rm -rf "$out"
pkg="$out/SplitzFFI"
mkdir -p "$out/headers" "$out/sim" "$out/mac" "$pkg"

# One archive per platform. --release, because a wallet ships release.
for t in "$DEVICE" "${SIMS[@]}" "${MACS[@]}"; do
  echo "building $t"
  (cd "$root/rust" && "${CARGO:-cargo}" build --quiet --release \
     -p splitz-ffi --target "$t")
done

# The simulator slices are two architectures of one platform, so they become
# one fat archive. The device slice stays alone.
sim_archives=()
for t in "${SIMS[@]}"; do
  sim_archives+=("$target_dir/$t/release/libsplitz_ffi.a")
done
lipo -create -output "$out/sim/libsplitz_ffi.a" "${sim_archives[@]}"

mac_archives=()
for t in "${MACS[@]}"; do
  mac_archives+=("$target_dir/$t/release/libsplitz_ffi.a")
done
lipo -create -output "$out/mac/libsplitz_ffi.a" "${mac_archives[@]}"

# The Swift binding and the C header the framework must carry.
#
# uniffi reads the metadata by loading the library, so it is generated from a
# HOST build, not from a cross-compiled one the running machine cannot load.
# The metadata is the same either way: it comes from the source, not the
# target triple.
(cd "$root/rust" && "${CARGO:-cargo}" build --quiet --release -p splitz-ffi)
(cd "$root/rust" && "${CARGO:-cargo}" run --quiet --bin uniffi-bindgen \
   -p splitz-ffi -- generate \
   --library "$target_dir/release/libsplitz_ffi.dylib" \
   --language swift --out-dir "$out/swift")

cp "$out/swift"/*.h "$out/headers/"
# Xcode looks for `module.modulemap` by that exact name inside the headers
# directory; uniffi names it after the module.
cp "$out/swift"/*.modulemap "$out/headers/module.modulemap"

xcodebuild -create-xcframework \
  -library "$target_dir/$DEVICE/release/libsplitz_ffi.a" \
  -headers "$out/headers" \
  -library "$out/sim/libsplitz_ffi.a" \
  -headers "$out/headers" \
  -library "$out/mac/libsplitz_ffi.a" \
  -headers "$out/headers" \
  -output "$pkg/splitz_ffiFFI.xcframework"

# The generated Swift becomes the package's only source. Its `import
# splitz_ffiFFI` is what fixes the binary target's name: the modulemap inside
# the xcframework declares that module, and a binary target named anything
# else would not satisfy the import.
mkdir -p "$pkg/Sources/SplitzFFI"
cp "$out/swift/splitz_ffi.swift" "$pkg/Sources/SplitzFFI/"
# The §15.5 relay client, in the same module, so `import SplitzFFI` reaches it.
cp "$root/tools/package/relay/SplitzRelay.swift" "$pkg/Sources/SplitzFFI/"

cat > "$pkg/Package.swift" <<'SWIFT'
// swift-tools-version:5.9
import PackageDescription

// The splitz protocol and its wallet plumbing, as a Swift wallet reaches it.
//
// `splitz_ffiFFI` is a binary target rather than a source one: the library is
// Rust, cross-compiled per platform, and the xcframework carries a device
// slice and a fat simulator slice. Both are arm64, so they cannot be one
// archive, which is the whole reason this format exists.
let package = Package(
    name: "SplitzFFI",
    platforms: [.iOS(.v13), .macOS(.v11)],
    products: [
        .library(name: "SplitzFFI", targets: ["SplitzFFI"])
    ],
    targets: [
        .binaryTarget(
            name: "splitz_ffiFFI",
            path: "splitz_ffiFFI.xcframework"
        ),
        .target(
            name: "SplitzFFI",
            dependencies: ["splitz_ffiFFI"],
            path: "Sources/SplitzFFI"
        )
    ]
)
SWIFT

echo
echo "swift package: $pkg"
(cd "$pkg" && swift package describe --type json >/dev/null &&
   echo "  Package.swift parses")
lipo -info "$target_dir/$DEVICE/release/libsplitz_ffi.a" | sed 's/^/  device: /'
lipo -info "$out/sim/libsplitz_ffi.a" | sed 's/^/  sim:    /'
lipo -info "$out/mac/libsplitz_ffi.a" | sed 's/^/  mac:    /'
echo "  a consumer depends on it with:"
echo "    .package(path: \"$pkg\")"

write_source_stamp "$out" "$stamp" "$root" "$scripts"
echo "  source:    rust tree $stamp, scripts $scripts (dist/ios/SOURCE)"
