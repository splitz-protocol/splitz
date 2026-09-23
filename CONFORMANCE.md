# Implementing splitz

What an implementation must do to be conformant, and how to check that it is.

`SPEC.md` is normative. This file is a guide to running the corpus against a
new implementation; where the two disagree, `SPEC.md` is right.

## What conformance means

Three things, and the third is the one most implementations miss.

**1. Reproduce the corpus.** `vectors/` holds 486 cases across 18 files.
Every case carries a `name`, its inputs, and either `expect` (the value the
implementation must produce) or `error` (the §12 code it must refuse with).
`vectors/README.md` describes each file's shape.

**Refusing for the wrong reason is a failure.** "This bill is from a newer
version" and "this bill is damaged" send a person to different remedies.

**2. Keep the rules the corpus cannot reach.** A corpus is generated from an
implementation, so it cannot hold a case that implementation is
self-consistently wrong about. Three properties are checked separately here
and are worth checking anywhere:

- two implementations answering one operation list identically (`tools/differential/`);
- §10.2's own algebra — a merge is idempotent, commutative, and independent of
  arrival order — asserted over generated logs rather than fixed ones;
- §9.3's instants against a reader nobody involved wrote (`tools/oracle/`).

**3. Keep §14, which is addressed to the wallet.** `vectors/withholdings.json`
is the only corpus file whose subject is the host rather than the wire. An
implementation can reproduce §1–§12 exactly and still ask somebody to pay a
debt they already paid, or send the balance of a bill to whoever minted the
second claim on an id. A wallet is not conformant without it.

## What the host supplies

§14.1 in full. In any language, the embedding wallet provides:

| | why it cannot be the library's |
|---|---|
| the participant id it speaks as | §10.4 decides what that id authorises |
| the payout address, or none | a participant with none is reported (§8.4), never dropped |
| a clock giving §9.3 instants | §10.2 orders a log by instant; a fold must never read a clock |
| unpredictable randomness | §9.4 derives a bill id from a nonce |
| a way to send, answering three ways | §14.3 — built-and-not-broadcast is neither sent nor failed |
| optionally a signer and verifier | §10.6 fixes the message; the curve operation is the host's |

## Numbers

**Every amount MUST be representable in a signed 64-bit integer** (§2.2),
and so must every total, sum and intermediate product formed from amounts.

That is a fact about the language, not about the library. A signed 64-bit
maximum is 19 digits; an IEEE-754 double is exact only to 2^53−1, which is 16.
A language whose only integer is a double cannot hold the amounts this
protocol admits.

Concretely: **JavaScript and TypeScript need BigInt throughout**, not `number`.
`tools/web-target/run.sh` demonstrates the failure rather than asserting it —
it compiles the Dart implementation to JavaScript and the build refuses, naming
the integer literal a JS number cannot hold.

Languages with a native 64-bit integer — Rust, Go, Swift, Kotlin, Java, C#,
Python, Dart on a native target — have nothing to do here.

## Determinism

Two devices holding the same entries must produce the same bill, byte for
byte. That rules out, in any language:

- comparing identifiers with the language's own string comparison. §2.3
  orders by UTF-8 bytes; Dart compares UTF-16 code units and Rust compares
  `char` values, so both need an explicit comparator. Two implementations
  that disagree here fold the same entries into different bills;
- iterating a hash map whose order is unspecified — sort by §2.3;
- floating point anywhere near an amount;
- reading a clock, a locale, a random source or anything on disk during a
  fold;
- resolving §10.7 over the live entry set rather than the whole one.

## Running the corpus

Each case is data. A harness in any language loads the JSON, feeds the inputs
to the implementation, and compares. The two harnesses here are worth reading
before writing a third: `dart/test/conformance_test.dart` and
`rust/splitz-core/tests/conformance.rs`. Neither is long.

Objects keyed by participant id are compared by content, not key order.
Amounts are integer minor units throughout. Instants are canonical (§9.3).

## Lanes CI does not run

`.github/workflows/ci.yml` runs thirteen jobs. Nine lanes in the tree are not
among them, because each needs a toolchain, a device or a daemon no hosted
runner carries by default. They pass on a developer machine and nothing
re-checks them, so a change that breaks one is found by hand or not at all:

| lane | what it needs |
|---|---|
| `tools/ffi/swift.sh` | Xcode, and `tools/package/ios.sh` run first |
| `tools/ffi/swift-simulator.sh` | the same, plus a booted iOS simulator |
| `tools/ffi/aar-consumer.sh` | Gradle, the Android SDK, and the NDK |
| `tools/ffi/aar-device.sh` | the same, plus a booted emulator or a device |
| `tools/ffi/flutter.sh` | the Flutter SDK |
| `tools/regtest/run.sh` | Docker, and a chain it brings up |
| `tools/package/ios.sh` | Xcode and the four Apple targets |
| `tools/package/android.sh` | the Android NDK; Gradle and the SDK for the AAR |
| `tools/package/npm.sh` | node and npm; it builds one platform, its own |

`tools/package/publish-preflight.sh` is not a lane. It reads only, and answers
whether anything may be published yet.

An implementation is conformant on the corpus and the differential lanes
alone; these say whether it can be *shipped*, which is a separate claim.

## Telling whether a green suite means anything

Break what an assertion guards and watch it go red for the reason it names,
before trusting it. A probe where every case returns the same answer is broken
rather than conclusive — a corpus that passes against an implementation with
one rule removed was never testing that rule.
