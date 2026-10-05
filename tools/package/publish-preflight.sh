#!/usr/bin/env bash
# Everything that must be true before anything is published. Reads only:
# it never flips a flag, never pushes, never publishes.
#
#     tools/package/publish-preflight.sh
#
# crates.io cannot be undone. A version number is taken forever, a yanked
# crate still occupies its name, and the contents stay downloadable. So this
# reports and stops, and the publish itself stays a decision somebody makes
# with the output in front of them.
set -uo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
cd "$root"
fail=0
note() { printf '  %-8s %s\n' "$1" "$2"; }
ok()   { note "ok" "$2"; }
bad()  { note "BLOCKED" "$2"; fail=1; }

echo "== attribution =="
# A single hit here is a stop: it is in the history permanently once pushed.
hits="$(git log --pretty='%an <%ae>|%cn <%ce>|%B' |
        grep -i -c -e claude -e anthropic -e co-authored || true)"
if [ "$hits" = "0" ]; then
  ok "" "no attribution strings in any commit"
else
  bad "" "$hits commit line(s) match claude/anthropic/co-authored"
fi

author_bad="$(git log --pretty='%an <%ae>|%cn <%ce>' | sort -u |
              grep -v 'keerthi-chandan <kamalsutra.in.pt@gmail.com>|keerthi-chandan <kamalsutra.in.pt@gmail.com>' || true)"
if [ -z "$author_bad" ]; then
  ok "" "every commit is authored AND committed by keerthi-chandan"
else
  bad "" "identities other than keerthi-chandan: $(echo "$author_bad" | head -3 | tr '\n' ' ')"
fi

echo
echo "== secrets =="
# A seed phrase or a named mainnet wallet must never reach a registry.
if git grep -l -i -E 'seed phrase|mnemonic|birthday' -- . >/dev/null 2>&1; then
  note "look" "$(git grep -l -i -E 'seed phrase|mnemonic|birthday' -- . | tr '\n' ' ')"
else
  ok "" "no seed/mnemonic/birthday vocabulary in tracked files"
fi

echo
echo "== publish guards, as they stand =="
for f in rust/splitz-core/Cargo.toml rust/splitz-host/Cargo.toml \
         rust/splitz-ffi/Cargo.toml; do
  if grep -q '^publish = false' "$f"; then
    note "held" "$f"
  else
    note "OPEN" "$f  <- would publish"
  fi
done
for f in dart/pubspec.yaml splitz_host/pubspec.yaml; do
  if grep -q '^publish_to: none' "$f"; then
    note "held" "$f"
  else
    note "OPEN" "$f  <- would publish"
  fi
done

echo
echo "== names, live =="
# Checked against the registries rather than remembered: a name free last week
# is not a name free today.
# Read the HTTP code, not curl's exit: `curl -f` fails on 404, which would
# make a free name and an unreachable registry the same answer.
crates_code() {
  curl -sS -o /dev/null -w '%{http_code}' -A 'splitz-preflight' \
    "https://crates.io/api/v1/crates/$1" 2>/dev/null
}
# A probe where every case returns one answer is broken, not conclusive. serde
# must come back 200 and a nonsense name 404, or this section says nothing.
control_taken="$(crates_code serde)"
control_free="$(crates_code zzz-no-such-crate-9f3a)"
if [ "$control_taken" != "200" ] || [ "$control_free" != "404" ]; then
  bad "" "crates.io probe is not working (serde=$control_taken, absent=$control_free) — treat the names below as unknown"
fi
for crate in splitz splitz-core splitz-host splitz-ffi; do
  code="$(crates_code "$crate")"
  case "$code" in
    404) note "free"  "crates.io/$crate" ;;
    200) note "TAKEN" "crates.io/$crate" ;;
    *)   note "?"     "crates.io/$crate — HTTP $code" ;;
  esac
done
for pkg in splitz splitz_core splitz_host; do
  code="$(curl -fsS -o /dev/null -w '%{http_code}' "https://pub.dev/api/packages/$pkg" 2>/dev/null)"
  if [ "$code" = "404" ]; then note "free" "pub.dev/$pkg"
  elif [ "$code" = "200" ]; then note "TAKEN" "pub.dev/$pkg"
  else note "?" "pub.dev/$pkg — HTTP $code"; fi
done

echo
echo "== packaging =="
# A package is compiled from rust/; one built from any other tree ships code
# the repository no longer holds. Each script stamps dist/<pkg>/SOURCE.
. "$root/tools/package/source.sh"
current="$(rust_source_stamp "$root")"
case "$current" in
  *+dirty) bad "" "rust/ has uncommitted changes — commit, then rebuild the packages" ;;
esac
for pkg in ios android npm; do
  [ -d "dist/$pkg" ] || continue
  if [ ! -f "dist/$pkg/SOURCE" ]; then
    bad "" "dist/$pkg has no SOURCE stamp — rebuild with tools/package/$pkg.sh"
    continue
  fi
  built="$(sed -n 's/^rust-tree //p' "dist/$pkg/SOURCE")"
  if [ "$built" = "$current" ]; then
    ok "" "dist/$pkg built from the current rust/ ($current)"
  else
    bad "" "dist/$pkg is stale: built from rust tree $built, rust/ is now $current — rerun tools/package/$pkg.sh"
  fi
  scripts="$(script_source_stamp "$root" "$pkg")"
  built="$(sed -n 's/^scripts //p' "dist/$pkg/SOURCE")"
  case "$scripts" in
    *+dirty) bad "" "tools/package/$pkg.sh or source.sh has uncommitted changes — commit, then rebuild dist/$pkg" ;;
    *) if [ "$built" = "$scripts" ]; then
         ok "" "dist/$pkg built by the current tools/package/$pkg.sh"
       else
         bad "" "dist/$pkg is stale: built by scripts ${built:-(unrecorded)}, now $scripts — rerun tools/package/$pkg.sh"
       fi ;;
  esac
  build="$(build_env_stamp "$root" "$pkg")"
  built="$(sed -n 's/^build //p' "dist/$pkg/SOURCE")"
  if [ "$built" = "$build" ]; then
    ok "" "dist/$pkg built with this toolchain, flags and cargo config"
  else
    bad "" "dist/$pkg was built with ${built:+another }${built:-an unrecorded} toolchain, flags or cargo config — rerun tools/package/$pkg.sh"
  fi
  # The stamps name what the package was built from; this checks that what
  # is in dist/ is still what was built. A file edited or replaced after the
  # build leaves every stamp above matching.
  built="$(sed -n 's/^files //p' "dist/$pkg/SOURCE")"
  if [ -z "$built" ]; then
    bad "" "dist/$pkg records no digest of its files — rerun tools/package/$pkg.sh"
  elif [ "$built" = "$(dist_digest "dist/$pkg")" ]; then
    ok "" "dist/$pkg holds exactly what was built"
  else
    bad "" "dist/$pkg has changed since it was built — rerun tools/package/$pkg.sh"
  fi
  # The digest leaves out build/, where Gradle and npm write their own
  # outputs. A prebuilt artefact there — the AAR above all — is what a
  # consumer would install, and nothing above says what it was built from.
  stray="$(cd "dist/$pkg" && find . -path '*/build/outputs/*' -type f \( -name '*.aar' -o -name '*.jar' \) -print -quit)"
  if [ -n "$stray" ]; then
    bad "" "dist/$pkg holds a build output no stamp covers (${stray#./}) — delete dist/$pkg/*/build and ship the module"
  fi
done
[ -d dist/ios/SplitzFFI/splitz_ffiFFI.xcframework ] &&
  ok "" "dist/ios/SplitzFFI built, with its xcframework" ||
  note "absent" "dist/ios/SplitzFFI — run tools/package/ios.sh"
[ -d dist/android/jniLibs ] &&
  ok "" "dist/android/jniLibs built ($(ls dist/android/jniLibs | tr '\n' ' '))" ||
  note "absent" "dist/android — run tools/package/android.sh"

# The npm package is checked for its guard as well as its existence.
# `npm publish --dry-run` exits 0 on a package marked private, so the dry run
# is not the thing that holds it back and cannot be relied on to.
if [ -f dist/npm/package.json ]; then
  if grep -q '"private"[[:space:]]*:[[:space:]]*true' dist/npm/package.json; then
    ok "" "dist/npm built and still marked private"
  else
    bad "" "dist/npm is NOT marked private — npm publish would go through"
  fi
else
  note "absent" "dist/npm — run tools/package/npm.sh"
fi

echo
echo "== Dart packages, as a registry would take them =="
# Checking the guards says nothing about whether a package would publish. Each
# is copied with the edits publishing makes — publish_to removed, and
# splitz_host depending on splitz_core by version rather than by path — and put
# through pub's own dry run. splitz_core resolves from the copy beside it
# through an override pub does not publish.
scratch="$(mktemp -d)"
for pkg in dart splitz_host; do
  rsync -a --exclude .dart_tool --exclude build "$pkg/" "$scratch/$pkg/"
  python3 - "$scratch/$pkg/pubspec.yaml" <<'PY'
import sys
path = sys.argv[1]
lines = open(path).read().splitlines()
out = [l for l in lines
       if l.strip() != "publish_to: none" and l.strip() != "path: ../dart"]
open(path, "w").write("\n".join(out) + "\n")
PY
  if [ "$pkg" = splitz_host ]; then
    printf 'dependency_overrides:\n  splitz_core:\n    path: ../dart\n' \
      >"$scratch/$pkg/pubspec_overrides.yaml"
  fi
  report="$(cd "$scratch/$pkg" && dart pub publish --dry-run 2>&1)"
  status=$?
  # Until splitz_core is on the registry, splitz_host resolves it only through
  # the override, and pub warns about the override. That one is expected;
  # anything else stops the publish.
  problems="$(echo "$report" | grep -E '^\* ' |
              grep -v 'overridden in pubspec_overrides.yaml' || true)"
  if [ "$status" = 0 ] || { [ "$pkg" = splitz_host ] && [ -z "$problems" ]; }; then
    ok "" "$pkg would publish$([ "$pkg" = splitz_host ] && echo ' once splitz_core is on pub.dev')"
  else
    bad "" "$pkg would not publish:"
    echo "$problems" | sed 's/^/             /' | head -12
  fi
done
rm -rf "$scratch"

echo
if [ "$fail" = "0" ]; then
  echo "preflight clean. Publishing is still a separate, deliberate act:"
  echo "  1. remove publish = false / publish_to: none"
  echo "  2. cargo publish --dry-run, then cargo publish, deepest crate first"
  echo "  3. dart pub publish --dry-run, then dart pub publish"
  echo "crates.io cannot be undone. Leave days, not hours."
else
  echo "preflight BLOCKED — fix the lines above before going further."
fi
exit "$fail"
