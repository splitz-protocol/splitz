# Splitz-Protocol

Split a bill with friends and settle it in Zcash.

Everyone adds what they spent to a shared bill. Splitz figures out who owes
whom, reduces it to the fewest payments that clear the whole thing, converts
the amounts using a rate saved on the bill, and gives each payer a single Zcash
payment request covering everything they owe. If you owe four people, you sign
one transaction.

This is a library and a specification, not an app. The wallet keeps what a
wallet should keep: your keys, your storage, the clock, randomness, the network,
and actually sending the transaction. `SPEC.md` §13 lists all ten of those, so
nobody mistakes them for missing features.

## What's in the repo

| | | |
|---|---|---|
| **the protocol** | `dart/`, `rust/splitz-core` | bills, the shared history, the five ways to split, working out who pays whom, the payment request, invites |
| **the plumbing** | `splitz_host/`, `rust/splitz-host` | signing entries, sealing them, storing them, syncing through a relay, swaps, activity |
| **the binding** | `rust/splitz-ffi` | using the Rust crate from Kotlin, Swift, Dart or JavaScript |
| **the seam** | `SPEC.md` §15 | the seven things a wallet has to provide |
| **the test cases** | `vectors/` | 460 cases in 18 files any implementation can run, in no particular language |
| **the extra checks** | `tools/` | everything a fixed set of test cases can't catch |

A Flutter wallet can also take the screens ready-made: `splitz_flutter` is a
separate package holding the bill, expense, settle, activity and share
screens over this library. It names no wallet and takes what it needs as
interfaces, so it drops into any Flutter Zcash wallet.

There are two implementations, one in Dart and one in Rust, written separately
from the same specification and run against the same test cases. The test cases
themselves come from a third implementation, written only from the text of the
spec — so no case was produced by the same code it's meant to be checking.

## How a wallet uses it

A wallet has to provide seven things: a way to send a transaction, somewhere to
keep secrets, storage, a relay to sync through, exchange rates, swaps, and its
own account details. Those are the seven interfaces in `SPEC.md` §15, 28 calls
in all. Everything else is the library's job.

Rust wallets use the crates directly, and Dart and Flutter wallets use the Dart
packages (`splitz_core`, `splitz_host`) — the Dart implementation, held to the
same answers as the Rust one by the differential and parity lanes. Kotlin,
Swift and JavaScript wallets use the generated binding, and it's deliberately
simple: you pass in the facts it needs and it passes back an answer, instead of
you having to implement seven sets of callbacks across a language boundary.

`INTEGRATING.md` walks through the whole thing, with a working sample in
Kotlin, Dart and JavaScript. The library stops at the payment request: your
wallet reads the addresses out of it, builds the transaction, and signs it.

## Use it in your wallet

Nothing is on a package registry yet: every crate and Dart package is marked
unpublishable. Depend on this repository by git, pinned to a commit.

| your wallet | depend on | what you get |
|---|---|---|
| Dart or Flutter | `splitz_core` and `splitz_host` | the protocol and the whole wallet layer: signing, sealing, the log, relay sync, swaps, activity |
| Rust | `splitz-core` and `splitz-host` | the same, in Rust |
| Kotlin, Swift, JavaScript | `splitz-ffi`, built by `tools/package/{android,ios,npm}.sh` | the protocol through a generated binding; the wallet layer is yours to write |

Dart — both packages, **pinned to the same commit**. Pub refuses a branch name
here: `splitz_host` reaches `splitz_core` by a path inside the repository, which
resolves to a commit, and a direct dependency on `main` is not that commit.

```yaml
dependencies:
  splitz_core:
    git:
      url: https://github.com/KamaIOps/Splitz-Protocol.git
      path: dart
      ref: <commit sha>
  splitz_host:
    git:
      url: https://github.com/KamaIOps/Splitz-Protocol.git
      path: splitz_host
      ref: <commit sha>
```

Rust — cargo finds each crate in the workspace by name:

```toml
[dependencies]
splitz-core = { git = "https://github.com/KamaIOps/Splitz-Protocol", rev = "<commit sha>" }
splitz-host = { git = "https://github.com/KamaIOps/Splitz-Protocol", rev = "<commit sha>" }
```

The binding cannot call back into a wallet, so a Kotlin, Swift or JavaScript
wallet passes in the facts each call needs and does its own sending, storage
and sync. `INTEGRATING.md` covers all three routes.

## How the money is handled

Every amount is a whole number of the smallest unit. No floating point ever
touches money. When a split doesn't divide evenly, the leftover units are handed
out in a fixed order, so the parts always add back up to the total and every
device gets the same answer from the same inputs.

Payments are worked out by cancelling debts against each other first, then
splitting people into as many groups as possible where the group's debts
balance to zero. A group of `k` people needs exactly `k−1` payments, so more
groups means fewer payments. Up to 14 people it finds the true minimum; above
that it uses a fast approximation, and the result tells you which of the two it
used rather than claiming a minimum it hasn't proven.

The bill's history is append-only, and merging two copies is just a union: do
it twice, or in a different order, and you get the same thing. So two people
who have seen different parts of the history still see the same bill.

## Verify it yourself

Needs Dart 3.11.4 or later and a Rust toolchain; the crates declare 1.82 as
their minimum. Run each command from the repository root; the comment beside
it says what a pass looks like.

```
cd dart && dart test                 # +554: All tests passed!
                                     #   every one of the 460 shared cases
                                     #   among them
cd splitz_host && dart test          # +132: All tests passed!
cd rust && cargo test                # every "test result: ok", 183 in all
cd dart && dart run example/dinner.dart
                                     # one bill, three people, the fewest
                                     #   payments, and one payer's single
                                     #   payment request
```

Whether the money arrives, on a local chain. Needs Docker with ports 9067
and 18232 free, and downloads the Sapling parameters on its first run:

```
tools/regtest/run.sh up              # a regtest node, mined past maturity
tools/regtest/run.sh prove           # ends: the money that arrived is the
                                     #   money splitz said.
tools/regtest/run.sh down
```

`prove` funds a wallet, builds one person's debts into a payment request,
sends it, mines it, and compares the recipient's balance with the amount
splitz computed.

## How it's tested

A fixed set of test cases can only cover what someone thought to write down,
so there are checks around it:

- **Same input, different code** (`tools/differential`) — inputs nobody wrote
  an answer for are run through every implementation and the answers compared.
- **Same surface** (`tools/parity`) — catches a function one side has and the
  other doesn't, which no amount of testing the output would show.
- **The spec against the code** (`tools/spec`) — every error code, quoted
  figure and cross-reference in `SPEC.md` has to still be true of the tree.
- **Somebody else's reader** (`tools/oracle`, `rust/splitz-core/tests/oracle.rs`)
  — timestamps checked against Python's date library, and payment requests
  against `librustzcash`. Everything else here was written by one author from
  one spec, so it could all be wrong in the same way.
- **A real wallet in four languages** (`tools/ffi`) — Kotlin, Swift, Dart
  and JavaScript each drive a whole bill through the crate. The Swift one
  builds against the packaged xcframework, the way a wallet reaches it.
- **Four devices and an intruder** (`splitz_host/test/rehearsal_test.dart`) —
  four devices with their own storage and keys sync through a relay, receive
  the updates in different orders, and end up with an identical bill. Then
  somebody with the bill's key tries to point a payout at their own address,
  and the settlement stops instead.
- **Does the money actually arrive** (`tools/regtest`) — on a local test chain
  it funds a wallet, turns one person's debts into a payment request, builds and
  sends the transaction from that request, mines it, and checks the recipient's
  balance against the figure splitz quoted. Needs Docker.
- **Samples and documents** (`tools/docs`, `tools/examples`,
  `tools/web-target`) — every code sample in the docs is a real file that gets
  run, not just compiled, and the package still refuses to build for
  JavaScript, where a 64-bit amount would quietly lose precision.

## Conformance

An implementation conforms when it reproduces every case in `vectors/` and
follows §14, which is about what the wallet has to do rather than what goes
over the wire.

`SPEC.md` §12 lists 78 reasons the library can refuse something. Every one has
a test case except the two the spec excuses by name: their input is a document
that any correct JSON reader would reject, so a test case containing one would
break the whole file rather than test anything.

`CONFORMANCE.md` explains what passing does and doesn't prove.
`vectors/README.md` describes the file format.

## Docs

- `SPEC.md` — the protocol itself.
- `INTEGRATING.md` — what a wallet has to provide, and what it gets back.
- `CONFORMANCE.md` — what conformance means.
- `vectors/README.md` — the test case format.

## Licence

MIT or Apache-2.0, whichever you prefer. See `LICENSE-MIT` and
`LICENSE-APACHE`.
