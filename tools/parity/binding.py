#!/usr/bin/env python3
"""The binding's surface against the Rust host crate's.

    python3 tools/parity/binding.py

Every public function `rust/splitz-host/src/lib.rs` re-exports is either
exported by `rust/splitz-ffi/src/pure.rs` under the same name or listed in
`allow-binding.txt`, with the export that carries it or the reason a binding
wallet has no use for it. Exit 1 names each one that is neither, and each
listed export that does not exist.

Every export is also named in INTEGRATING.md or SPEC.md, by its own name or
the camelCase name the Kotlin, Swift and Dart bindings give it: an export a
wallet cannot learn of from the documents is one it will not call.
"""

from __future__ import annotations

import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
HOST = ROOT / "rust/splitz-host/src/lib.rs"
BINDING = ROOT / "rust/splitz-ffi/src/pure.rs"
ALLOW = pathlib.Path(__file__).resolve().parent / "allow-binding.txt"


def host_functions() -> set[str]:
    names: set[str] = set()
    for braces, single in re.findall(
        r"pub use [a-z_:]+::\{([^}]*)\}|pub use [a-z_:]+::([A-Za-z_]+);", HOST.read_text(), re.S
    ):
        for name in re.split(r"[,\s]+", braces or single):
            if re.fullmatch(r"[a-z][a-z0-9_]*", name):
                names.add(name)
    return names


def binding_exports() -> set[str]:
    return set(
        re.findall(
            # Any attribute, argument or doc line may sit between the export
            # and the function it marks.
            r"#\[uniffi::export(?:\([^)]*\))?\]\s*(?:(?:#\[[^\]]*\]|///[^\n]*)\s*)*pub fn ([a-z0-9_]+)",
            BINDING.read_text(),
        )
    )


def allowed() -> dict[str, str | None]:
    out: dict[str, str | None] = {}
    for line in ALLOW.read_text().splitlines():
        line = line.split("#", 1)[0].strip() if not re.match(r"^[a-z0-9_]+\s+#", line) else line
        if not line or line.startswith("#"):
            continue
        m = re.fullmatch(r"([a-z0-9_]+)\s*->\s*([a-z0-9_]+)", line)
        if m:
            out[m.group(1)] = m.group(2)
            continue
        m = re.fullmatch(r"([a-z0-9_]+)\s+#\s*\S.*", line)
        if m:
            out[m.group(1)] = None
            continue
        raise SystemExit(f"allow-binding.txt: cannot read {line!r}")
    return out


def main() -> int:
    host, exports, allow = host_functions(), binding_exports(), allowed()
    problems = []
    for name in sorted(host - exports):
        if name not in allow:
            problems.append(f"UNEXPLAINED host::{name}: not exported, and not in allow-binding.txt")
        elif allow[name] is not None and allow[name] not in exports:
            problems.append(f"STALE host::{name} -> {allow[name]}: no such export")
    for name in sorted(set(allow) - host):
        problems.append(f"STALE allow-binding.txt names host::{name}, which the crate does not export")
    docs = "\n".join(
        (ROOT / doc).read_text(encoding="utf-8") for doc in ("INTEGRATING.md", "SPEC.md")
    )
    for name in sorted(exports):
        camel = re.sub(r"_([a-z0-9])", lambda m: m.group(1).upper(), name)
        if not re.search(rf"\b({re.escape(name)}|{re.escape(camel)})\b", docs):
            problems.append(f"UNDOCUMENTED export {name}: named in neither INTEGRATING.md nor SPEC.md")
    carried = sum(1 for n in host if n in exports or allow.get(n))
    print(
        f"binding: {len(host)} host functions, {len(exports)} exports, "
        f"{carried} carried, {len(host) - carried} excused, {len(problems)} unexplained"
    )
    for p in problems:
        print("  " + p)
    return 1 if problems else 0


if __name__ == "__main__":
    sys.exit(main())
