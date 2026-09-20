#!/usr/bin/env python3
"""Checks that a sample a document shows is the file a lane runs.

A code block in a document is a claim about the library, and it is the one
kind of claim nothing else here reaches: the corpus checks values, the
differential lane checks that three implementations answer alike, the parity
lane checks the two public surfaces, and `tools/spec/claims.py` checks
SPEC.md. None of them compiles a sample. A sample that no longer compiles is
worse than no sample, because it is the first thing an integrator writes.

So a block may quote a file instead of standing on its own. The fence names
it:

    ```kotlin file=tools/ffi/kotlin/Doc.kt
    ...
    ```

and the file marks where the quotation starts:

    // docs:begin

Everything after that line, to the end of the file, must be what the block
holds, byte for byte.

What is checked:

  block-matches   every `file=` block is its source, exactly
  source-quoted   every file carrying a `docs:begin` marker is quoted by some
                  document — a marked file nothing shows is a sample with no
                  reader
  source-run      every quoted file is named by a script under `tools/`, so
                  the block is code that ran rather than code that parsed

Usage: python3 tools/docs/blocks.py
Exit status is 1 when a block has drifted, so it can gate a commit.
"""
import difflib
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
DOCUMENTS = ["INTEGRATING.md", "README.md", "CONFORMANCE.md", "SPEC.md"]
MARKER = "docs:begin"
# A whole comment line and nothing else, so that a document or a script
# writing the word does not become a sample by mentioning one.
MARKER_LINE = re.compile(r"^\s*(?://|#)\s*" + MARKER + r"\s*$")

FENCE = re.compile(
    r"^```[^\n]*?\bfile=(?P<path>\S+)[^\n]*\n(?P<body>.*?)^```$",
    re.MULTILINE | re.DOTALL,
)


def quoted_region(path: pathlib.Path) -> str:
    """The part of a source file a document is allowed to show."""
    text = path.read_text(encoding="utf-8")
    lines = text.splitlines(keepends=True)
    for i, line in enumerate(lines):
        if MARKER_LINE.match(line):
            return "".join(lines[i + 1 :])
    raise SystemExit(f"{path}: no `{MARKER}` marker, so nothing may quote it")


def marked_sources() -> set[str]:
    """Every file in the tree that offers itself to a document."""
    out = set()
    me = pathlib.Path(__file__).resolve()
    for path in ROOT.rglob("*"):
        if not path.is_file() or ".git" in path.parts or "target" in path.parts:
            continue
        if path.resolve() == me:
            continue
        if path.suffix not in {".kt", ".dart", ".mjs", ".js", ".rs", ".swift", ".py"}:
            continue
        try:
            if any(MARKER_LINE.match(line) for line in path.read_text(encoding="utf-8").splitlines()):
                out.add(path.relative_to(ROOT).as_posix())
        except UnicodeDecodeError:
            continue
    return out


def scripts_text() -> str:
    return "\n".join(
        p.read_text(encoding="utf-8")
        for p in (ROOT / "tools").rglob("*")
        if p.is_file() and p.suffix in {".sh", ".py", ".yml"}
    )


def main() -> int:
    failures: list[str] = []
    quoted: set[str] = set()
    blocks = 0

    for name in DOCUMENTS:
        document = ROOT / name
        if not document.exists():
            continue
        text = document.read_text(encoding="utf-8")
        for match in FENCE.finditer(text):
            blocks += 1
            rel = match.group("path")
            quoted.add(rel)
            source = ROOT / rel
            if not source.exists():
                failures.append(f"block-matches  {name} quotes {rel}, which does not exist")
                continue
            want = quoted_region(source)
            saw = match.group("body")
            if saw == want:
                print(f"  ok    block-matches  {name} ← {rel} ({want.count(chr(10))} lines)")
                continue
            diff = "".join(
                difflib.unified_diff(
                    want.splitlines(keepends=True),
                    saw.splitlines(keepends=True),
                    fromfile=rel,
                    tofile=f"{name} (the block)",
                )
            )
            failures.append(f"block-matches  {name} has drifted from {rel}:\n{diff}")

    for rel in sorted(marked_sources() - quoted):
        failures.append(f"source-quoted  {rel} is marked for a document, and no document quotes it")

    scripts = scripts_text()
    for rel in sorted(quoted):
        if rel not in scripts:
            failures.append(f"source-run     {rel} is quoted, and no script under tools/ runs it")
        else:
            print(f"  ok    source-run     {rel}")

    print()
    if failures:
        for failure in failures:
            print(f"  FAIL  {failure}")
        print(f"\n{len(failures)} failure(s) over {blocks} quoted block(s)")
        return 1
    print(f"{blocks} quoted block(s), every one its source and every source run")
    return 0


if __name__ == "__main__":
    sys.exit(main())
