#!/usr/bin/env bash
# A Swift wallet over the generated binding, driving one whole bill.
#
# It builds the consumer against the PACKAGE, the way an integrator reaches it
# — `.package(path:)` and `import SplitzFFI` — rather than against the source
# tree. A module that compiles is not an integration: this one opens a bill,
# joins it, adds an expense, prices it, settles a debt, records the payment and
# has the payee confirm it.
#
#     tools/package/ios.sh     # builds dist/ios/SplitzFFI first
#     tools/ffi/swift.sh
#
# Runs on macOS, against the xcframework's macOS slice. Needs Xcode.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
pkg="$root/dist/ios/SplitzFFI"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

if ! command -v swift >/dev/null; then
  echo "no swift" >&2
  exit 2
fi

if [ ! -d "$pkg/splitz_ffiFFI.xcframework" ]; then
  echo "no package at $pkg — run tools/package/ios.sh first" >&2
  exit 2
fi

# A consumer is its own package depending on ours by path, which is what a
# wallet writes. Building inside our package instead would prove nothing about
# whether the manifest exports what it claims.
mkdir -p "$work/Consumer/Sources/Consumer"
cp "$root/tools/ffi/swift/Consumer.swift" "$work/Consumer/Sources/Consumer/main.swift"

cat > "$work/Consumer/Package.swift" <<SWIFT
// swift-tools-version:5.9
import PackageDescription

let package = Package(
    name: "Consumer",
    platforms: [.macOS(.v11)],
    dependencies: [.package(path: "$pkg")],
    targets: [
        .executableTarget(
            name: "Consumer",
            dependencies: [.product(name: "SplitzFFI", package: "SplitzFFI")],
            path: "Sources/Consumer"
        )
    ]
)
SWIFT

(cd "$work/Consumer" && swift run --quiet Consumer)
