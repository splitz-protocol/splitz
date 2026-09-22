#!/usr/bin/env bash
# The same bill, executed on an Android device or emulator.
#
# `tools/ffi/aar-consumer.sh` runs the bill on the JVM, which loads the HOST
# cdylib: it proves the packaged Kotlin surface and the protocol behind it,
# and says nothing about the four `jni/<abi>/libsplitz_ffi.so` the AAR
# carries. This lane runs the same flow as an instrumented test, so the
# Android ELF is extracted from the APK by the platform and executed.
#
#     tools/package/android.sh                 # writes the module
#     (cd dist/android/splitz && gradle assembleRelease)
#     tools/ffi/aar-device.sh
#
# Needs Gradle, the Android SDK, and a booted emulator or attached device.
# Not run by CI: a hosted runner has neither.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
sdk="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
aar="$root/dist/android/splitz/build/outputs/aar/splitz-release.aar"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

if [ ! -f "$aar" ]; then
  echo "no AAR at $aar — run tools/package/android.sh then gradle assembleRelease" >&2
  exit 2
fi
if ! "$sdk/platform-tools/adb" devices | grep -qE "device$"; then
  echo "no device: boot an emulator, e.g." >&2
  echo "  $sdk/emulator/emulator -avd \$($sdk/emulator/emulator -list-avds | head -1) &" >&2
  exit 2
fi

# Built outside the repository so no build/, .gradle/ or .kotlin/ lands in it.
cp -R "$root/tools/ffi/aar/." "$work/"
(cd "$work" && ANDROID_HOME="$sdk" gradle connectedDebugAndroidTest --no-daemon \
   -PsplitzAar="$aar" -PsplitzLibDir="$root/rust/target/release")
status=$?

# The device's own stdout, which Gradle does not print.
log="$(find "$work/build/outputs/androidTest-results" -name 'logcat-null*' 2>/dev/null | head -1)"
if [ -n "$log" ]; then
  grep -oE "native library loaded[^\"]{0,80}|PASS  [^\"]{0,80}|FAIL  [^\"]{0,80}|AAR DEVICE RESULT[^\"]{0,40}" \
    "$log" | sed 's/\\n.*//'
fi
exit "$status"
