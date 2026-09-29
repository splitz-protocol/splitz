#!/usr/bin/env bash
# The same bill, executed on an iOS simulator.
#
# `tools/ffi/swift.sh` runs on macOS, against the xcframework's macOS slice.
# That proves the binding's surface and the protocol behind it, and says
# nothing about the iOS slices — which are the ones a wallet ships. This lane
# runs the identical flow on a simulator, so the arm64 iOS code is executed
# rather than inspected with `lipo`.
#
#     tools/package/ios.sh          # builds dist/ios/SplitzFFI first
#     tools/ffi/swift-simulator.sh  # or pass a simulator udid as $1
#
# Needs Xcode and a booted simulator. Not run by CI: a hosted runner carries
# no simulator.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
pkg="$root/dist/ios/SplitzFFI"
work="$(mktemp -d)"
RELAY_PID=
trap 'rm -rf "$work"; [ -z "$RELAY_PID" ] || kill "$RELAY_PID" 2>/dev/null || true' EXIT

if [ ! -d "$pkg/splitz_ffiFFI.xcframework" ]; then
  echo "no package at $pkg — run tools/package/ios.sh first" >&2
  exit 2
fi

udid="${1:-$(xcrun simctl list devices booted -j |
  python3 -c 'import json,sys
d=json.load(sys.stdin)["devices"]
for rt,devs in d.items():
    if "iOS" not in rt: continue
    for x in devs:
        if x.get("state")=="Booted": print(x["udid"]); raise SystemExit
' )}"
if [ -z "$udid" ]; then
  echo "no booted iOS simulator; boot one or pass a udid" >&2
  exit 2
fi

src="$work/SimConsumer"
mkdir -p "$src/Sources/SimConsumer" "$src/Tests/SimConsumerTests"

# A target exists only so the package generates a scheme; a test-only package
# generates none, and xcodebuild cannot be pointed at it.
echo "// Present so the package generates a scheme." \
  > "$src/Sources/SimConsumer/Empty.swift"

# The two devices sync through a live tools/relay/server.py, as in swift.sh.
# A simulator shares the Mac's network, so 127.0.0.1 reaches it.
# shellcheck source=relay.sh
. "$root/tools/ffi/relay.sh"
relay_up "$root" "$work"

# The macOS consumer, as a test case: its entry point is replaced by one that
# runs the bill against the relay. `run` is renamed: inside an XCTestCase that
# name resolves to XCTestCase.run(), so the test would start itself and fail
# with "a test run that has already been started".
python3 - "$root/tools/ffi/swift/Consumer.swift" \
         "$src/Tests/SimConsumerTests/Flow.swift" \
         "$RELAY_ORIGIN" "$RELAY_DOWN_ORIGIN" <<'PY'
import io, sys
s = io.open(sys.argv[1], encoding='utf-8').read()
def once(old, new):
    global s
    if s.count(old) != 1:
        raise SystemExit(f"Consumer.swift no longer has exactly one {old!r}")
    s = s.replace(old, new)
once("import SplitzFFI", "import SplitzFFI\nimport XCTest")
once("func run(origin: String, downOrigin: String) async throws {",
     "func runBill(origin: String, downOrigin: String) async throws {")
i = s.index("let arguments = CommandLine.arguments")
s = s[:i] + '''final class SimConsumerTests: XCTestCase {
    func testDrivesAWholeBill() async throws {
        try await runBill(origin: "%s", downOrigin: "%s")
        XCTAssertEqual(failures, 0, "\\(failures) check(s) failed")
    }
}
''' % (sys.argv[3], sys.argv[4])
io.open(sys.argv[2], 'w', encoding='utf-8').write(s)
PY

cat > "$src/Package.swift" <<PKG
// swift-tools-version:5.9
import PackageDescription
let package = Package(
    name: "SimConsumer",
    platforms: [.iOS(.v13)],
    products: [.library(name: "SimConsumer", targets: ["SimConsumer"])],
    dependencies: [.package(path: "$pkg")],
    targets: [
        .target(name: "SimConsumer",
                dependencies: [.product(name: "SplitzFFI", package: "SplitzFFI")],
                path: "Sources/SimConsumer"),
        .testTarget(name: "SimConsumerTests",
                    dependencies: ["SimConsumer",
                                   .product(name: "SplitzFFI", package: "SplitzFFI")],
                    path: "Tests/SimConsumerTests")
    ]
)
PKG

echo "running on simulator $udid"
(cd "$src" && xcodebuild test -scheme SimConsumer \
   -destination "platform=iOS Simulator,id=$udid" 2>&1) |
  grep -E "PASS  |FAIL  |Test Case .*(passed|failed)|\*\* TEST"
