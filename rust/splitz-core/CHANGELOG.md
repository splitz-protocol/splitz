# Changelog

## Unreleased

Breaking, in the invite surface:

- `Invite::expiry` is `Option<i64>`, was `Option<u64>`. An expiry is a Unix
  timestamp in seconds and §11.1 bounds it to a signed 64-bit integer, which
  is the width the other implementations carry. Code holding
  `let t: u64 = invite.expiry.unwrap();` no longer compiles.
- `INVITE_VERSION` is `i64`, was `u32`, for the same reason. Before this, a
  `v` between 2^32 and 2^63 was `invite_missing_version` here and
  `invite_future_version` elsewhere.
- `render_invite` returns `Result<String>`, was `String`. It now refuses an
  expiry its own parser would refuse rather than emitting a URI no reader
  accepts.

Newly exported from the crate root, to match what the Dart package exposes —
previously `pub` inside their modules and reachable only by spelling the
module out: the bill model (`Bill`, `Participant`, `Expense`, `PaymentRecord`,
`Payout`, `Settlement`, `SettlementPlan`, `DirectDebt`, `Position`),
`FoldResult`, `SetAside`, `ReplacedAddress`, `ScannedPayload`, `SealedFrame`,
`derive_entry_id`, `order_entries`, `confirmation_rule`, `canonical_instant`,
`decode_rate`, `decode_participant`, `decode_expense`, `decode_payment`,
`qchar`, `bounded_label`, `strip_scan_padding`, the UTF-8 ordering helpers and
every `MAX_*` / `*_VERSION` / domain constant.

Also: a payload body nests at most 64 deep (§11.2) and its `v` is bounded as
§11.1 bounds the invite's; only a `splitz1:` payload carries an `invite`, and
only an object is one; an entry whose payload the decoder refuses is set aside
rather than making the whole bill undecodable; and `tests/` is no longer part
of the published package, because the conformance corpus is not and a suite
with nothing to run reports `ok`.

## 1.0.0 — 2026-09-19

First release, and wire format version 1.

Implements `SPEC.md` in full: the five split methods, exact integer money,
minimal settlement with coverage attribution, the append-only log, and ZIP 321
output. Every entry id is the digest of its entry (§9.5), so no re-pushed copy
can displace a genuine one.
