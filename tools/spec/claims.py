#!/usr/bin/env python3
"""Checks the claims SPEC.md makes against the tree that has to keep them.

The corpus checks values, the differential lane checks that three
implementations answer alike, and the parity lane checks the two public
surfaces. None of them reads the specification, so a sentence in SPEC.md is
the one kind of claim in this repository that nothing verifies — and a
specification that says something the tree does not do is a defect that
outranks a defect in the code, because every implementation written from it
inherits the mistake.

What is checkable is checked here:

  codes-declared   every §12 code is declared by all three implementations
  codes-listed     every code an implementation declares appears in §12
  codes-thrown     every §12 code has a throw site in each implementation
  codes-covered    every §12 code has a corpus vector, but for the two
                   exceptions §12 names and justifies in its own text
  vectors-named    every `vectors/…` file and every corpus case SPEC.md names
                   by name exists
  figures-pinned   every number SPEC.md quotes beside a named case matches
                   what that case measures
  sections-resolve every §N cross-reference points at a section that exists
  seam-declared    every interface §15 names is declared in the host package,
  seam-declared-rust  the same seven in the Rust host crate, its operation
                   names compared in snake case
                   with every operation §15 names on it, and no interface the
                   package declares is missing from §15

What is not checkable is not pretended: a MUST that is prose about a host, a
rationale, or a rule whose subject is an implementation this repository does
not contain. Those are listed at the end as the surface this lane does not
reach, so the number is visible rather than implied.

Usage: python3 tools/spec/claims.py
Exit status is 1 when a claim fails, so it can gate a commit.
"""
import json
import pathlib
import re
import sys

ROOT = pathlib.Path(__file__).resolve().parents[2]
SPEC = ROOT / "SPEC.md"


def spec_text() -> str:
    return SPEC.read_text(encoding="utf-8")


def section_12(text: str) -> str:
    start = text.index("## 12. Error codes")
    end = text.find("\n## ", start + 1)
    return text[start:end if end > 0 else len(text)]


def listed_codes(text: str) -> set[str]:
    """The codes §12 enumerates, from its leading inventory paragraphs."""
    body = section_12(text)
    stop = body.find("**The code is part of the protocol")
    return set(re.findall(r"`([a-z][a-z0-9_]{3,})`", body[:stop]))


def exceptions(text: str) -> set[str]:
    """The codes §12 itself exempts from needing a vector, by name."""
    body = section_12(text)
    tail = body[body.find("**The code is part of the protocol"):]
    # The form §12 names one in: "`code` is that exception". A code merely
    # near the word is not exempted by it.
    return set(re.findall(
        r"`([a-z][a-z0-9_]{3,})` is (?:that|the|an|one) exception", tail))


def _reference_codes() -> set[str]:
    """Codes the reference can produce.

    Two shapes: `Refused("…")` raises, and the fold's `aside(entry, "…", …)`
    reports without raising. A check that reads only the first calls the fold's
    own refusals missing.
    """
    text = (ROOT / "tools/corpus/_spec.py").read_text(encoding="utf-8")
    return (set(re.findall(r'Refused\("([a-z][a-z0-9_]+)"\)', text))
            | set(re.findall(r'aside\([^,]+,\s*"([a-z][a-z0-9_]+)"', text))
            | set(re.findall(r'"code":\s*"([a-z][a-z0-9_]+)"', text)))


def declared() -> dict[str, set[str]]:
    """The code strings each implementation declares."""
    dart = set(re.findall(
        r"static const \w+ =\s*'([a-z][a-z0-9_]+)';",
        (ROOT / "dart/lib/src/errors.dart").read_text(encoding="utf-8")))
    rust = set(re.findall(
        r'pub const [A-Z0-9_]+: &str = "([a-z][a-z0-9_]+)";',
        (ROOT / "rust/splitz-core/src/error.rs").read_text(encoding="utf-8")))
    ref = _reference_codes()
    return {"dart": dart, "rust": rust, "reference": ref}


def thrown() -> dict[str, set[str]]:
    """The codes each implementation can actually raise.

    Dart and Rust name a constant at the throw site, so the constant's own
    name is resolved back to its string; the reference names the string.
    """
    dart_src = "\n".join(p.read_text(encoding="utf-8")
                         for p in (ROOT / "dart/lib/src").glob("*.dart"))
    dart_names = dict(re.findall(
        r"static const (\w+) =\s*'([a-z][a-z0-9_]+)';",
        (ROOT / "dart/lib/src/errors.dart").read_text(encoding="utf-8")))
    dart = {dart_names[n] for n in re.findall(r"SplitCode\.(\w+)", dart_src)
            if n in dart_names}

    rust_src = "\n".join(p.read_text(encoding="utf-8")
                         for p in (ROOT / "rust/splitz-core/src").glob("*.rs"))
    rust_names = dict(re.findall(
        r'pub const ([A-Z0-9_]+): &str = "([a-z][a-z0-9_]+)";',
        (ROOT / "rust/splitz-core/src/error.rs").read_text(encoding="utf-8")))
    rust = {rust_names[n] for n in re.findall(r"code::([A-Z0-9_]+)", rust_src)
            if n in rust_names}

    return {"dart": dart, "rust": rust, "reference": _reference_codes()}


def corpus_codes() -> set[str]:
    """Every code string any vector produces, anywhere in its expectation."""
    found: set[str] = set()

    def walk(node):
        if isinstance(node, dict):
            for key, value in node.items():
                if key in ("error", "code") and isinstance(value, str):
                    found.add(value)
                walk(value)
        elif isinstance(node, list):
            for item in node:
                walk(item)

    for path in sorted((ROOT / "vectors").glob("*.json")):
        walk(json.loads(path.read_text(encoding="utf-8")))
    return found


def corpus_cases() -> dict[str, dict]:
    """Every corpus case, keyed `file:name`.

    A case name is unique within its file and not across the corpus: seven
    names are carried by more than one file, because `a_version_from_the_future`
    is a sensible name for a bill, an invite, a payload and a sealed frame. A
    map keyed by name alone silently keeps one of each and drops the rest,
    which both undercounts the corpus and lets a figure quoted beside one case
    be checked against another.
    """
    cases: dict[str, dict] = {}
    for path in sorted((ROOT / "vectors").glob("*.json")):
        doc = json.loads(path.read_text(encoding="utf-8"))
        for case in doc.get("cases", []):
            if isinstance(case, dict) and "name" in case:
                cases[f"{path.name}:{case['name']}"] = case
    return cases


def seam_claims(text: str) -> dict[str, list[str]]:
    """The interfaces §15 names, each with the operations it names on it.

    One heading per interface, `### 15.N `Name``, followed by a line that
    begins `**Operations.**` and lists them in backticks. A heading with no
    such line is a claim with nothing to check and is reported as a failure
    rather than skipped.
    """
    start = text.index("## 15. The wallet seam")
    body = text[start:]
    end = body.find("\n## ", 1)
    body = body[:end] if end > 0 else body
    claims: dict[str, list[str]] = {}
    blocks = re.split(r"^### 15\.\d+ `([A-Za-z]+)`", body, flags=re.M)
    for name, chunk in zip(blocks[1::2], blocks[2::2]):
        line = re.search(r"^\*\*Operations\.\*\*(.+)$", chunk, re.M)
        claims[name] = re.findall(r"`([A-Za-z][A-Za-z0-9]*)`", line.group(1)) \
            if line else []
    return claims


def seam_source() -> dict[str, str]:
    """Each `abstract interface class` in the host package, with its body.

    The body runs to the first line that closes it at column zero, which is
    how the package formats every one of them.
    """
    bodies: dict[str, str] = {}
    for path in sorted((ROOT / "splitz_host" / "lib").rglob("*.dart")):
        src = path.read_text(encoding="utf-8")
        for match in re.finditer(
                r"^abstract interface class (\w+)[^{]*\{", src, re.M):
            tail = src[match.end():]
            close = re.search(r"^\}", tail, re.M)
            bodies[match.group(1)] = tail[:close.start()] if close else tail
    return bodies


def case_names(cases: dict[str, dict]) -> set[str]:
    """The bare names, for a document that cites a case without its file."""
    return {key.split(":", 1)[1] for key in cases}


def seam_source_rust() -> dict[str, str]:
    """Each `pub trait` in the Rust host crate's seam module, with its body.

    The body runs to the first line that closes it at column zero, which is
    how `cargo fmt` writes every one of them.
    """
    bodies: dict[str, str] = {}
    # One module, and only that one: `wallet.rs` is the seam, and a trait
    # somewhere else in the crate is a shape a caller passes rather than an
    # interface a wallet implements. A check over the whole crate would ask
    # §15 to specify every closure the plumbing takes.
    for path in [ROOT / "rust" / "splitz-host" / "src" / "wallet.rs"]:
        src = path.read_text(encoding="utf-8")
        for match in re.finditer(r"^pub trait (\w+)[^{]*\{", src, re.M):
            tail = src[match.end():]
            close = re.search(r"^\}", tail, re.M)
            bodies[match.group(1)] = tail[:close.start()] if close else tail
    return bodies


def snake(name: str) -> str:
    """`minorUnitsPerZec` -> `minor_units_per_zec`."""
    return re.sub(r"(?<!^)(?=[A-Z])", "_", name).lower()


def main() -> int:
    text = spec_text()
    failures: list[str] = []
    counts: dict[str, int] = {}

    def check(name: str, subjects: int, bad: list[str]) -> None:
        counts[name] = subjects
        for line in bad:
            failures.append(f"{name}: {line}")

    listed = listed_codes(text)
    decl, thrw = declared(), thrown()
    covered = corpus_codes()
    cases = corpus_cases()
    exempt = exceptions(text)

    # Every §12 code is declared, and everything declared is listed.
    check("codes-declared", len(listed) * len(decl),
          [f"§12 lists `{c}`, {impl} does not declare it"
           for impl, names in sorted(decl.items())
           for c in sorted(listed - names)])
    check("codes-listed", sum(len(v) for v in decl.values()),
          [f"{impl} declares `{c}`, §12 does not list it"
           for impl, names in sorted(decl.items())
           for c in sorted(names - listed)])

    # Every §12 code has a throw site. A declared constant nothing raises is a
    # refusal that cannot happen.
    check("codes-thrown", (len(listed) - len(exempt)) * len(thrw),
          [f"§12 lists `{c}`, nothing in {impl} raises it"
           for impl, names in sorted(thrw.items())
           for c in sorted(listed - names - exempt)])

    # Every §12 code has a vector, but for the ones §12 exempts by name.
    check("codes-covered", len(listed),
          [f"§12 lists `{c}` and no vector produces it"
           for c in sorted(listed - covered - exempt)])
    check("codes-exempt-are-real", len(exempt),
          [f"§12 exempts `{c}`, which is not a code it lists"
           for c in sorted(exempt - listed)])

    # Every vector file and every case SPEC.md names by name exists.
    named_files = set(re.findall(r"`vectors/([a-z0-9-]+\.json)`", text))
    check("vectors-named", len(named_files),
          [f"SPEC.md names `vectors/{f}`, which does not exist"
           for f in sorted(named_files)
           if not (ROOT / "vectors" / f).exists()])

    named_cases = set(re.findall(r"`(the_[a-z0-9_]+|[a-z]+_payable_[a-z0-9_]+)`", text))
    check("cases-named", len(named_cases),
          [f"SPEC.md names the case `{c}`, which no vector file carries"
           for c in sorted(named_cases) if c not in case_names(cases)])

    # A figure quoted in the same table row as a case name must be what that
    # case measures. This is what stops a number drifting from its evidence.
    pinned = 0
    bad_figures: list[str] = []
    for row in re.findall(r"^\|.*`([a-z0-9_…]+)`.*\|\s*\**(\d[\d,]*|refused)\**\s*\|$",
                          text, re.M):
        name, figure = row
        bare = name.lstrip("…")
        match = [c for c in cases if c.split(":", 1)[1].endswith(bare)]
        if not match:
            continue
        pinned += 1
        if len(match) > 1:
            # Resolving to whichever file sorts first would check the figure
            # against a case SPEC.md was not talking about.
            bad_figures.append(
                f"`{name}` names a case in {len(match)} files "
                f"({', '.join(sorted(m.split(':', 1)[0] for m in match))}); "
                f"quote it with its file")
            continue
        case = cases[match[0]]
        if figure == "refused":
            if "error" not in case:
                bad_figures.append(f"`{match[0]}` is quoted as refused and is accepted")
        elif "expect" not in case:
            bad_figures.append(f"`{match[0]}` is quoted as {figure} and is refused")
        else:
            measured = len(case["expect"]) - len("splitz1:")
            if measured != int(figure.replace(",", "")):
                bad_figures.append(
                    f"`{match[0]}` is quoted as {figure} and measures {measured}")
    check("figures-pinned", pinned, bad_figures)

    # Every §N cross-reference points at a section that exists.
    headings = set(re.findall(r"^#{2,3} (\d+(?:\.\d+)?)[.  ]", text, re.M))

    def resolves(ref: str) -> bool:
        if ref in headings:
            return True
        # A section that numbers its rules as steps rather than subsections is
        # referred to the same way: §3.6 is step 6 of §3, not a heading.
        top, _, part = ref.partition(".")
        if not part or top not in headings:
            return False
        start = text.index(f"\n## {top}.")
        end = text.find("\n## ", start + 1)
        return bool(re.search(rf"^{int(part)}\. ", text[start:end], re.M))

    # Every document that cites the specification, not only the
    # specification itself: a renumbered section breaks a guide silently, and
    # the guide is what somebody implementing this in a fourth language reads.
    cited = {"SPEC.md": text}
    for name in ("CONFORMANCE.md", "README.md", "vectors/README.md"):
        path = ROOT / name
        if path.exists():
            cited[name] = path.read_text(encoding="utf-8")

    refs = {(name, r)
            for name, body in cited.items()
            for r in re.findall(r"§(\d+(?:\.\d+)?)", body)}
    check("sections-resolve", len(refs),
          [f"{name} refers to §{r}, which is neither a heading nor a "
           f"numbered step of its section"
           for name, r in sorted(refs)
           if not resolves(r)])

    # Every count of the corpus quoted in a document is the corpus's own.
    # A figure typed once and left behind tells somebody implementing this
    # that they have run everything when they have not.
    total = len(cases)
    files = len(list((ROOT / "vectors").glob("*.json")))
    counted = 0
    bad_counts: list[str] = []
    for name, body in cited.items():
        # Only phrasings that mean the whole corpus. "11 cases" in a
        # sentence about the oracle is a count of something else.
        for m in re.finditer(
                r"(\d[\d,]*)\s+(?:language-neutral\s+conformance\s+cases"
                r"|cases\s+(?:across|in)\s+(\d+)\s+files)", body):
            counted += 1
            got = int(m.group(1).replace(",", ""))
            if got != total:
                bad_counts.append(
                    f"{name} says {got} cases, the corpus holds {total}")
            if m.group(2) and int(m.group(2)) != files:
                bad_counts.append(
                    f"{name} says {m.group(2)} files, `vectors/` holds {files}")
    check("corpus-size", counted, bad_counts)

    # Every count of §12's codes quoted in a document is §12's own.
    code_total = len(listed_codes(text))
    code_counts = 0
    bad_code_counts: list[str] = []
    for name, body in cited.items():
        for m in re.finditer(r"§12 lists (\d+) reasons", body):
            code_counts += 1
            if int(m.group(1)) != code_total:
                bad_code_counts.append(
                    f"{name} says §12 lists {m.group(1)}, it lists {code_total}")
    check("code-count", code_counts, bad_code_counts)

    # Every interface §15 names is declared in the host package, with every
    # operation §15 names on it. A seam written down is a contract, and a
    # contract nothing checks is a sentence.
    claimed = seam_claims(text)
    declared_seam = seam_source()
    bad_seam: list[str] = []
    subjects = 0
    for name, ops in sorted(claimed.items()):
        subjects += 1
        if not ops:
            bad_seam.append(f"§15 names `{name}` with no **Operations.** line")
            continue
        body = declared_seam.get(name)
        if body is None:
            bad_seam.append(
                f"§15 names `{name}`, which `splitz_host/lib/` does not "
                f"declare as an abstract interface class")
            continue
        for op in ops:
            subjects += 1
            if not re.search(rf"\b(get\s+{op}\b|{op}\s*\()", body):
                bad_seam.append(
                    f"§15 names `{name}.{op}`, which its declaration does "
                    f"not carry")
    for name in sorted(set(declared_seam) - set(claimed)):
        bad_seam.append(
            f"`splitz_host/lib/` declares the interface `{name}`, which §15 "
            f"does not specify")
    check("seam-declared", subjects, bad_seam)

    # The same seven, in the Rust host crate. §15 says a binding names them in
    # its own idiom, so the operation names are compared in snake case.
    declared_rust = seam_source_rust()
    bad_rust: list[str] = []
    subjects_rust = 0
    for name, ops in sorted(claimed.items()):
        subjects_rust += 1
        body = declared_rust.get(name)
        if body is None:
            bad_rust.append(
                f"§15 names `{name}`, which `rust/splitz-host/src/wallet.rs` "
                f"does not declare as a public trait")
            continue
        for op in ops:
            subjects_rust += 1
            if not re.search(rf"\bfn {snake(op)}\s*[(<]", body):
                bad_rust.append(
                    f"§15 names `{name}.{op}`, which the Rust trait does not "
                    f"carry as `{snake(op)}`")
    for name in sorted(set(declared_rust) - set(claimed)):
        bad_rust.append(
            f"`rust/splitz-host/src/wallet.rs` declares the public trait "
            f"`{name}`, which §15 does not specify")
    check("seam-declared-rust", subjects_rust, bad_rust)

    # A check with no subjects is broken rather than passing.
    for name, n in sorted(counts.items()):
        if n == 0:
            failures.append(f"{name}: no subjects — the check reads nothing")

    print(f"{len(listed)} codes in §12, {len(cases)} corpus cases, "
          f"{len(counts)} checks")
    for name in sorted(counts):
        print(f"  {name:22} {counts[name]:5} subjects")

    if failures:
        print(f"\n{len(failures)} CLAIM(S) THE TREE DOES NOT KEEP")
        for line in failures:
            print(f"  {line}")
        return 1
    print("\nevery checkable claim in SPEC.md is kept by the tree")
    print("NOT reached by this lane: a MUST addressed to a host, a rationale, "
          "and any rule whose\nsubject is an implementation this repository "
          "does not contain.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
