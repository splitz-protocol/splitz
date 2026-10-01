# What a package in dist/ was built from, sourced by the package scripts and
# by publish-preflight.sh. Two stamps, both compared by the preflight: the
# code the package compiles, and the scripts that shape it.
#
# The stamp is the git tree hash of rust/ at HEAD — every crate the binding
# compiles and the lock file — with "+dirty" when rust/ carries uncommitted
# changes, since those are compiled but named by no hash.

# The directory cargo builds rust/ into, as cargo itself resolves it —
# CARGO_TARGET_DIR, build.target-dir in a config file, or rust/target. A
# package copied from any other directory ships whatever an earlier build left
# there under a stamp naming HEAD.
cargo_target_dir() {
  local root="$1"
  (cd "$root/rust" && "${CARGO:-cargo}" metadata --format-version 1 --no-deps) |
    python3 -c 'import json, sys; print(json.load(sys.stdin)["target_directory"])'
}

rust_source_stamp() {
  local root="$1" tree
  tree="$(git -C "$root" rev-parse HEAD:rust)"
  if [ -n "$(git -C "$root" status --porcelain -- rust)" ]; then
    echo "$tree+dirty"
  else
    echo "$tree"
  fi
}

# The git hashes, at HEAD, of the script that builds <pkg>, of this file and
# of tools/package/relay/ (the relay clients every package carries), joined by
# "+", with "+dirty" when any carries uncommitted changes. A package script
# decides what lands in the package — slices, ABIs, the manifest, the sources
# copied in — so a change to one leaves a package stale though rust/ is not.
script_source_stamp() {
  local root="$1" pkg="$2" a b c
  a="$(git -C "$root" rev-parse "HEAD:tools/package/$pkg.sh")"
  b="$(git -C "$root" rev-parse HEAD:tools/package/source.sh)"
  c="$(git -C "$root" rev-parse HEAD:tools/package/relay)"
  if [ -n "$(git -C "$root" status --porcelain -- "tools/package/$pkg.sh" tools/package/source.sh tools/package/relay)" ]; then
    echo "$a+$b+$c+dirty"
  else
    echo "$a+$b+$c"
  fi
}

# One SHA-256 over what compiles <pkg> besides its sources: the Rust
# toolchain, every environment variable cargo reads flags, profiles or linkers
# from, each cargo config file that applies to rust/, and the platform
# toolchain the package script builds with. Two builds of one tree under a
# different compiler or `RUSTFLAGS` are different binaries, so neither stamp
# above can tell them apart. The files are hashed and never printed: a cargo
# config may carry a registry token.
build_env_stamp() {
  local root="$1" pkg="$2" dir ndk sdk
  {
    (cd "$root/rust" && "${CARGO:-cargo}" -V && rustc -vV)
    env | LC_ALL=C sort | grep -E '^(RUSTFLAGS|RUSTDOCFLAGS|RUSTC|RUSTC_WRAPPER|CARGO_ENCODED_RUSTFLAGS|CARGO_BUILD_[A-Z_]+|CARGO_PROFILE_[A-Z_]+|CARGO_TARGET_[A-Z0-9_]+|ANDROID_API_LEVEL|ANDROID_NDK_HOME|ANDROID_HOME|IPHONEOS_DEPLOYMENT_TARGET|MACOSX_DEPLOYMENT_TARGET)=' || true
    dir="$(cd "$root/rust" && pwd -P)"
    while :; do
      for f in "$dir/.cargo/config.toml" "$dir/.cargo/config"; do
        [ -f "$f" ] && { echo "== $f"; cat "$f"; }
      done
      [ "$dir" = / ] && break
      dir="$(dirname "$dir")"
    done
    for f in "${CARGO_HOME:-$HOME/.cargo}/config.toml" "${CARGO_HOME:-$HOME/.cargo}/config"; do
      [ -f "$f" ] && { echo "== $f"; cat "$f"; }
    done
    case "$pkg" in
      ios) xcodebuild -version 2>/dev/null || true ;;
      android)
        ndk="${ANDROID_NDK_HOME:-}"
        if [ -z "$ndk" ]; then
          sdk="${ANDROID_HOME:-$HOME/Library/Android/sdk}"
          ndk="$(ls -d "$sdk"/ndk/* 2>/dev/null | sort -V | tail -1 || true)"
        fi
        echo "ndk $ndk"
        [ -f "$ndk/source.properties" ] && cat "$ndk/source.properties"
        ;;
      npm) node --version 2>/dev/null || true ;;
    esac
  } | shasum -a 256 | cut -d' ' -f1
}

# One SHA-256 over what <dir> holds: each file's path and its own SHA-256, in
# byte order of path. SOURCE itself is left out, and so is what a later tool
# run adds beside the package — build/, .gradle/, node_modules/ — so building
# the AAR from dist/android does not make the package read as changed.
dist_digest() {
  local dir="$1"
  (cd "$dir" && find . -type f ! -name SOURCE ! -path '*/build/*' \
      ! -path '*/.gradle/*' ! -path '*/node_modules/*' -print0 |
    LC_ALL=C sort -z | xargs -0 shasum -a 256) | shasum -a 256 | cut -d' ' -f1
}

# Records the three stamps, taken before the build, and the digest of what the
# build left, into <dir>/SOURCE. Called last, so a build that fails leaves no
# stamp claiming it finished.
write_source_stamp() {
  local dir="$1" stamp="$2" root="$3" scripts="$4" build="$5"
  printf 'rust-tree %s\nscripts %s\nbuild %s\ncommit %s\nfiles %s\n' "$stamp" "$scripts" \
    "$build" "$(git -C "$root" rev-parse HEAD)" "$(dist_digest "$dir")" >"$dir/SOURCE"
}
