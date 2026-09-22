# What a package in dist/ was built from, sourced by the package scripts and
# by publish-preflight.sh. Two stamps, both compared by the preflight: the
# code the package compiles, and the scripts that shape it.
#
# The stamp is the git tree hash of rust/ at HEAD — every crate the binding
# compiles and the lock file — with "+dirty" when rust/ carries uncommitted
# changes, since those are compiled but named by no hash.

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

# Records both stamps, taken before the build, into <dir>/SOURCE. Called last,
# so a build that fails leaves no stamp claiming it finished.
write_source_stamp() {
  local dir="$1" stamp="$2" root="$3" scripts="$4"
  printf 'rust-tree %s\nscripts %s\ncommit %s\n' "$stamp" "$scripts" \
    "$(git -C "$root" rev-parse HEAD)" >"$dir/SOURCE"
}
