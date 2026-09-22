#!/usr/bin/env bash
# The crate as an Android wallet reaches it: one .so per ABI, in the jniLibs
# layout Gradle expects, with the generated Kotlin beside it.
#
#     tools/package/android.sh        # -> dist/android/jniLibs/<abi>/
#
# Needs the NDK. Set ANDROID_NDK_HOME, or the newest one under
# $ANDROID_HOME/ndk is taken.
#
# Cargo does not know where the NDK's linkers are, so each target is given its
# linker explicitly through CARGO_TARGET_<TRIPLE>_LINKER. The NDK's clang
# wrappers carry the API level in their name: the number is the minimum
# Android version the library will load on, and raising it later silently
# drops devices.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
out="$root/dist/android"
# Taken before the build: the source this package is compiled from.
. "$root/tools/package/source.sh"
stamp="$(rust_source_stamp "$root")"
scripts="$(script_source_stamp "$root" android)"
API="${ANDROID_API_LEVEL:-21}"

ndk="${ANDROID_NDK_HOME:-}"
if [ -z "$ndk" ]; then
  sdk="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
  ndk="$(ls -d "$sdk"/ndk/* 2>/dev/null | sort -V | tail -1 || true)"
fi
if [ -z "$ndk" ] || [ ! -d "$ndk" ]; then
  echo "no NDK: set ANDROID_NDK_HOME" >&2
  exit 2
fi

host_tag="$(ls "$ndk/toolchains/llvm/prebuilt" | head -1)"
bin="$ndk/toolchains/llvm/prebuilt/$host_tag/bin"
if [ ! -d "$bin" ]; then
  echo "no toolchain under $ndk/toolchains/llvm/prebuilt" >&2
  exit 2
fi

# triple : jniLibs ABI directory : the NDK clang wrapper's prefix
#
# armv7 is the one that does not follow the pattern: Rust calls the target
# `armv7-linux-androideabi`, the NDK calls its compiler `armv7a-…eabi`, and
# Android calls the directory `armeabi-v7a`. Three spellings, one architecture.
TARGETS=(
  "aarch64-linux-android:arm64-v8a:aarch64-linux-android"
  "armv7-linux-androideabi:armeabi-v7a:armv7a-linux-androideabi"
  "x86_64-linux-android:x86_64:x86_64-linux-android"
  "i686-linux-android:x86:i686-linux-android"
)

rm -rf "$out"
mkdir -p "$out/jniLibs"

for row in "${TARGETS[@]}"; do
  IFS=: read -r triple abi prefix <<<"$row"
  if ! rustup target list --installed | grep -qx "$triple"; then
    echo "missing rust target $triple: rustup target add $triple" >&2
    exit 2
  fi
  linker="$bin/${prefix}${API}-clang"
  if [ ! -x "$linker" ]; then
    echo "no linker $linker (API $API not in this NDK?)" >&2
    exit 2
  fi
  # The variable name is the triple upper-cased with dashes as underscores.
  var="CARGO_TARGET_$(echo "$triple" | tr 'a-z-' 'A-Z_')_LINKER"
  echo "building $triple -> $abi"
  (cd "$root/rust" && env "$var=$linker" "AR_${triple//-/_}=$bin/llvm-ar" \
     "${CARGO:-cargo}" build --quiet --release -p splitz-ffi --target "$triple")
  mkdir -p "$out/jniLibs/$abi"
  cp "$root/rust/target/$triple/release/libsplitz_ffi.so" "$out/jniLibs/$abi/"
done

# uniffi reads the metadata by loading the library, so the Kotlin is generated
# from a HOST build. The metadata comes from the source, not the target.
(cd "$root/rust" && "${CARGO:-cargo}" build --quiet --release -p splitz-ffi)
lib="$root/rust/target/release/libsplitz_ffi.dylib"
[ -f "$lib" ] || lib="$root/rust/target/release/libsplitz_ffi.so"
(cd "$root/rust" && "${CARGO:-cargo}" run --quiet --bin uniffi-bindgen \
   -p splitz-ffi -- generate --library "$lib" \
   --language kotlin --out-dir "$out/kotlin")

# The library as an Android wallet declares it: a Gradle module whose AAR
# carries the four ABIs and the generated Kotlin, so a consumer writes one
# `implementation` line and never sees a `.so`.
pkg="$out/splitz"
mkdir -p "$pkg/src/main/kotlin" "$pkg/src/main/jniLibs"
cp -R "$out/jniLibs/." "$pkg/src/main/jniLibs/"
cp -R "$out/kotlin/." "$pkg/src/main/kotlin/"

cat > "$pkg/build.gradle.kts" <<'GRADLE'
// The splitz protocol and its wallet plumbing, as an Android wallet reaches it.
//
// The native library is not built here: `tools/package/android.sh` cross
// compiles it per ABI and drops it into `src/main/jniLibs`, which is where the
// Android plugin expects to find one. JNA is an `api` dependency rather than
// an `implementation` one because the generated binding's own types reach the
// consumer through it.
// AGP 9 carries Kotlin support itself; the separate
// `org.jetbrains.kotlin.android` plugin is refused alongside it.
plugins {
    id("com.android.library")
    // Without this the build emits a bare .aar and no POM, so the JNA
    // dependency below never reaches a consumer: a wallet that drops the
    // file in gets `Unresolved reference 'jna'` at compile time. With it,
    // `gradle publishToMavenLocal` writes the POM that carries it.
    id("maven-publish")
}

android {
    namespace = "cash.splitz.ffi"
    compileSdk = 36
    defaultConfig { minSdk = 21 }
    compileOptions {
        sourceCompatibility = JavaVersion.VERSION_17
        targetCompatibility = JavaVersion.VERSION_17
    }
    // The `release` software component does not exist until a variant is
    // declared publishable; without this, `publishToMavenLocal` fails with
    // "SoftwareComponent with name 'release' not found".
    publishing { singleVariant("release") }
}

// No `sourceSets` block: the generated Kotlin and the per-ABI `.so` files are
// written to `src/main/kotlin` and `src/main/jniLibs`, which is where AGP
// looks by convention. Declaring them again is refused by AGP 9.


dependencies {
    // `api`, not `implementation`: the generated binding's own types reach the
    // consumer through JNA.
    api("net.java.dev.jna:jna:5.17.0@aar")
}

afterEvaluate {
    publishing {
        publications {
            create<MavenPublication>("release") {
                from(components["release"])
                groupId = "cash.splitz"
                artifactId = "splitz"
                version = "0.1.0"
            }
        }
    }
}
GRADLE

cat > "$pkg/settings.gradle.kts" <<'GRADLE'
pluginManagement {
    repositories { google(); mavenCentral(); gradlePluginPortal() }
    plugins {
        // Pinned, and read from Google's own maven metadata rather than
        // guessed: an AGP version that does not exist fails at plugin
        // resolution with a list of repositories rather than a version.
        id("com.android.library") version "9.4.1"
    }
}
dependencyResolutionManagement {
    repositories { google(); mavenCentral() }
}
rootProject.name = "splitz"
GRADLE

echo
echo "jniLibs: $out/jniLibs"
for row in "${TARGETS[@]}"; do
  IFS=: read -r _ abi _ <<<"$row"
  printf '  %-12s ' "$abi"
  file -b "$out/jniLibs/$abi/libsplitz_ffi.so" | cut -d, -f1-2
done
echo
echo "gradle module: $pkg"
echo "  a consumer depends on it with:"
echo "    implementation(project(\":splitz\"))"
echo "  build the AAR with: (cd $pkg && gradle assembleRelease)"
echo "  or publish it with:  (cd $pkg && gradle publishToMavenLocal)"
echo
echo "  two costs a consumer pays, and should be told about:"
echo "    - the AAR declares minCompileSdk=36, so a wallet compiling against"
echo "      an older SDK cannot depend on it"
echo "    - a wallet that drops the bare .aar in, rather than resolving it"
echo "      from a repository, gets no POM and must declare JNA itself"

write_source_stamp "$out" "$stamp" "$root" "$scripts"
echo "  source:    rust tree $stamp, scripts $scripts (dist/android/SOURCE)"
