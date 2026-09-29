#!/usr/bin/env bash
# The crate as a Node wallet reaches it: one npm package carrying the
# generated JavaScript and a native library it resolves on import.
#
# ONE RUN OF THIS SCRIPT PRODUCES ONE PLATFORM — the machine it runs on.
# Node has no cross-compilation of its own: the package ships a compiled Rust
# cdylib per platform under `prebuilds/<platform>-<arch>/`, and the loader
# picks the directory matching the host at import time. So the package built
# here carries exactly one such directory, and its `os`/`cpu` fields say which,
# so npm refuses the install elsewhere instead of resolving it and then failing
# at the first call. A release on the registry needs this same build repeated
# on each platform a wallet runs on — linux-x64-gnu, linux-arm64-gnu,
# darwin-x64, darwin-arm64, win32-x64 — and their prebuild directories merged
# into one package. That matrix is not built here and is not implied anywhere
# in the output.
#
#     tools/package/npm.sh        # -> dist/npm, and dist/npm/splitz-ffi-<v>.tgz
#
# Needs node, npm and cargo, and a network the npm registry and crates.io are
# reachable from: the generator is third-party and pinned (as in
# tools/ffi/node.sh), and the package's one dependency, koffi, is fetched when
# the tarball is installed.
#
# Nothing here is published. `private: true` in the generated package.json is
# the npm counterpart of `publish = false` on the crates: `npm publish` refuses
# a manifest carrying it. `npm publish --dry-run` does not — the check runs in
# the publish itself — so a clean dry run says nothing about that guard.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
out="$root/dist/npm"
# Taken before the build: the source this package is compiled from.
. "$root/tools/package/source.sh"
stamp="$(rust_source_stamp "$root")"
scripts="$(script_source_stamp "$root" npm)"
work="${NPM_WORK:-$(mktemp -d)}"
# Pinned with tools/ffi/node.sh: every generator outside the uniffi tree
# targets uniffi 0.31, which is the version this crate pins.
generator_version="0.0.16"

for tool in node npm; do
  if ! command -v "$tool" >/dev/null; then
    echo "no $tool: an npm package cannot be built or checked without it" >&2
    exit 2
  fi
done
if ! command -v "${CARGO:-cargo}" >/dev/null; then
  echo "no cargo: the package carries a compiled library, not Rust source" >&2
  exit 2
fi

# The platform directory the generated loader will look in, and the library
# name it expects there. Both come from the host, because that is the only
# platform this run can build.
target="$(node -e 'const p = process.platform, a = process.arch;
if (p !== "linux") { console.log(`${p}-${a}`); } else {
  const g = process.report?.getReport?.().header?.glibcVersionRuntime;
  console.log(`${p}-${a}-${g == null ? "musl" : "gnu"}`);
}')"

# --release, because a wallet ships release.
echo "building splitz-ffi for $target"
# musl targets link the C runtime statically by default, and a static runtime
# cannot be a shared library: cargo drops the cdylib the loader needs.
case "$target" in
  *-musl) export RUSTFLAGS="${RUSTFLAGS:-} -C target-feature=-crt-static" ;;
esac
(cd "$root/rust" && "${CARGO:-cargo}" build --quiet --release -p splitz-ffi)
lib="$(cargo_target_dir "$root")/release"
for name in libsplitz_ffi.dylib libsplitz_ffi.so splitz_ffi.dll; do
  [ -f "$lib/$name" ] && library="$lib/$name" && break
done
if [ -z "${library:-}" ]; then
  echo "no splitz-ffi library under $lib" >&2
  exit 1
fi

generator="${UNIFFI_BINDGEN_NODE_JS:-$work/tools/bin/uniffi-bindgen-node-js}"
if [ ! -x "$generator" ]; then
  echo "installing uniffi-bindgen-node-js $generator_version"
  "${CARGO:-cargo}" install uniffi-bindgen-node-js \
    --version "$generator_version" --root "$work/tools" --quiet
fi

rm -rf "$out"
# --bundled-prebuilds is what makes this a package rather than a directory: the
# loader resolves `prebuilds/<platform>-<arch>/` by itself, so a consumer
# imports a name and never names a `.dylib`. Run from the workspace: the
# generator reads `cargo metadata` from its own working directory.
(cd "$root/rust" && "$generator" generate --out-dir "$out" \
   --package-name splitz-ffi --bundled-prebuilds "$library")

# The §15.5 relay client, re-exported from the entry point so
# `import { SplitzRelay } from "splitz-ffi"` reaches it beside the binding.
cp "$root/tools/package/relay/splitz_relay.js" "$root/tools/package/relay/splitz_relay.d.ts" "$out/"
for entry in index.js index.d.ts; do
  if [ ! -f "$out/$entry" ]; then
    echo "the generator wrote no $entry to re-export the relay client from" >&2
    exit 1
  fi
  printf '\nexport * from "./splitz_relay.js";\n' >>"$out/$entry"
done

mkdir -p "$out/prebuilds/$target"
cp "$library" "$out/prebuilds/$target/$(basename "$library")"

# The package's identity is the crate's: one version number, one description,
# one licence, owned by rust/splitz-ffi/Cargo.toml and read from it here.
manifest="$root/rust/splitz-ffi/Cargo.toml"
field() { grep -m1 "^$1 = " "$manifest" | cut -d'"' -f2; }
version="$(field version)"
description="$(field description)"
license="$(field license)"
repository="$(field repository)"
for name in version description license repository; do
  if [ -z "${!name}" ]; then
    echo "no $name in $manifest" >&2
    exit 1
  fi
done

# The generator owns the dependency, the entry point and the engine range;
# everything below is what a package needs and a binding generator has no
# opinion about. `files` is read from what was actually generated, so a file
# the generator starts or stops emitting cannot fall out of the tarball
# silently.
PKG_FILE="$out/package.json" PKG_VERSION="$version" PKG_DESCRIPTION="$description" \
PKG_LICENSE="$license" PKG_REPOSITORY="$repository" \
node -e '
const fs = require("node:fs");
const path = require("node:path");
const file = process.env.PKG_FILE;
const dir = path.dirname(file);
const pkg = JSON.parse(fs.readFileSync(file, "utf8"));
pkg.version = process.env.PKG_VERSION;
pkg.description = process.env.PKG_DESCRIPTION;
pkg.license = process.env.PKG_LICENSE;
// npm rewrites a bare string into this shape on publish and warns about it;
// written out, the manifest in the tarball is the manifest npm would send.
const repo = process.env.PKG_REPOSITORY.replace(/\.git$/, "");
pkg.repository = { type: "git", url: `git+${repo}.git` };
// Held back from the registry for the same reason the crates are.
pkg.private = true;
// One platform per build. Without these npm installs the package anywhere and
// the failure surfaces at the first call instead of at the install.
pkg.os = [process.platform];
pkg.cpu = [process.arch];
pkg.files = fs.readdirSync(dir)
  .filter((n) => n !== "package.json" && n !== "node_modules" && !n.endsWith(".tgz"))
  .sort();
fs.writeFileSync(file, JSON.stringify(pkg, null, 2) + "\n");
'

tarball="$(cd "$out" && npm pack --silent)"
[ -f "$out/$tarball" ] || { echo "npm pack wrote no tarball" >&2; exit 1; }

# A package nobody has installed is a claim. This installs the tarball into an
# empty directory outside it — the only way to find out whether `files`,
# `exports` and the staged library are right — and drives one whole bill
# through it.
consumer="$work/consumer"
rm -rf "$consumer"
mkdir -p "$consumer"
cat > "$consumer/package.json" <<'JSON'
{ "name": "splitz-npm-consumer", "private": true, "type": "module" }
JSON
cp "$root/tools/ffi/node/package-consumer.mjs" "$consumer/"
(cd "$consumer" && npm install --silent --no-fund --no-audit "$out/$tarball")
(cd "$consumer" && node package-consumer.mjs)

echo
echo "npm package: $out"
echo "  tarball:   $out/$tarball"
echo "  platform:  $target only — no other prebuild is in this package"
echo "  a consumer depends on it with:"
echo "    npm install $out/$tarball"

write_source_stamp "$out" "$stamp" "$root" "$scripts"
echo "  source:    rust tree $stamp, scripts $scripts (dist/npm/SOURCE)"
