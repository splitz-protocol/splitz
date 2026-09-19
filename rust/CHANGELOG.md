# Changelog

## 1.0.0 — 2026-09-19

First release, and wire format version 1.

Implements `SPEC.md` in full: the five split methods, exact integer money,
minimal settlement with coverage attribution, the append-only log, and ZIP 321
output. Every entry id is the digest of its entry (§9.5), so no re-pushed copy
can displace a genuine one.
