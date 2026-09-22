# What a package in dist/ was built from, sourced by the package scripts and
# by publish-preflight.sh.
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

# Records the stamp taken before the build into <dir>/SOURCE. Called last, so
# a build that fails leaves no stamp claiming it finished.
write_source_stamp() {
  local dir="$1" stamp="$2" root="$3"
  printf 'rust-tree %s\ncommit %s\n' "$stamp" \
    "$(git -C "$root" rev-parse HEAD)" >"$dir/SOURCE"
}
