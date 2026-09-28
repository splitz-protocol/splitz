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

# The git blob hashes, at HEAD, of the script that builds <pkg> and of this
# file, joined by "+", with "+dirty" when either carries uncommitted changes.
# A package script decides what lands in the package — slices, ABIs, the
# manifest — so a change to one leaves a package stale though rust/ is not.
script_source_stamp() {
  local root="$1" pkg="$2" a b
  a="$(git -C "$root" rev-parse "HEAD:tools/package/$pkg.sh")"
  b="$(git -C "$root" rev-parse HEAD:tools/package/source.sh)"
  if [ -n "$(git -C "$root" status --porcelain -- "tools/package/$pkg.sh" tools/package/source.sh)" ]; then
    echo "$a+$b+dirty"
  else
    echo "$a+$b"
  fi
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

# Records both stamps, taken before the build, and the digest of what the
# build left, into <dir>/SOURCE. Called last, so a build that fails leaves no
# stamp claiming it finished.
write_source_stamp() {
  local dir="$1" stamp="$2" root="$3" scripts="$4"
  printf 'rust-tree %s\nscripts %s\ncommit %s\nfiles %s\n' "$stamp" "$scripts" \
    "$(git -C "$root" rev-parse HEAD)" "$(dist_digest "$dir")" >"$dir/SOURCE"
}
