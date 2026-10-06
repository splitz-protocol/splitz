# Splitz-Protocol

Split a bill with friends and settle it in Zcash.

Everyone adds what they spent to a shared bill, and whoever opened it closes
it once everything is on it. Splitz then figures out who owes whom, reduces it
to the fewest payments that clear the whole thing, converts the amounts using
a rate saved on the bill, and gives each payer a single Zcash payment request
covering everything they owe. If you owe four people, you sign one
transaction, and if one of them wants paying in another asset, their swap
deposit goes out in that same transaction. A debt counts as paid only once the
person paid says it arrived.

This is a library and a specification, not an app. The wallet keeps what a
wallet should keep: your keys, your storage, the clock, randomness, the network,
and actually sending the transaction. `SPEC.md` §13 lists all nine of those, so
nobody mistakes them for missing features.

## What's in the repo

| | | |
|---|---|---|
| **the protocol** | `dart/`, `rust/splitz-core` | bills, the shared history, the five ways to split, working out who pays whom, the payment request, invites |
| **the plumbing** | `splitz_host/`, `rust/splitz-host` | signing entries, sealing them, storing them, syncing through a relay, swaps, activity |
| **the binding** | `rust/splitz-ffi` | using the Rust crate from Kotlin, Swift, Dart or JavaScript |
| **the packages** | `tools/package` | an xcframework for Swift, an AAR for Android and an npm package for Node.js, each built from the binding |
| **the relay** | `tools/relay` | a relay to sync bills through, in Python and as a Cloudflare Worker; it holds only sealed entries |
| **the seam** | `SPEC.md` §15 | the seven things a wallet has to provide |
| **the test cases** | `vectors/` | 995 cases in 24 files any implementation can run, in no particular language |
| **the extra checks** | `tools/` | everything a fixed set of test cases can't catch |

Screens are the wallet's own. This repository ships none: a wallet draws its
bills over `splitz_host` or the binding, in its own design.

There are two implementations, one in Dart and one in Rust, written separately
from the same specification and run against the same test cases. The test cases
themselves come from a third implementation, written only from the text of the
spec — so no case was produced by the same code it's meant to be checking.

## What a bill goes through

1. **Invite.** A link, a QR code or a pasted code, carrying the key the bill
   is sealed under (§11).
2. **Join.** Each person joins under their own key. A name the creator added
   can be merged into the person once they join (`planMerge`, §14.11). The
   creator, or the person themselves, can take them off, their expenses
   re-split (`planRemoval`, §10.8).
3. **Add expenses.** Split equally, by exact amounts, by percentage, by shares
   or item by item (§4). The writer corrects an expense; the writer or the
   creator withdraws it (§10.4, §10.8).
4. **Say how you get paid.** ZEC, another asset by swap, or cash, in order of
   preference (§9.1); a payer may fall back to a later one (§14.8).
5. **Close for settling.** Only the creator closes the bill. While closed, no
   expense is written; a change to the expenses reopens it (`closeFor`,
   `reopenFor`, §10.9, §14.9).
6. **Settle.** The payer sees who they pay, the amounts, the addresses and the
   rate before signing (§14.2). One transaction carries every ZEC debt as a
   ZIP 321 request, with a swap deposit as one more output (`combinedSend`,
   §14.10). Cash is recorded by hand. A send whose fate is unknown is held
   until the wallet says (§14.3).
7. **Confirm.** A debt is paid only when the person paid says so (§10.5).
   Until then nothing is asked for twice, and a request plus everything
   pending never exceeds what is owed (`withholdings`, §14.4). A payee's
   wallet finds payments that already arrived (`arrivalsFor`, §14.7).
8. **Square.** No payment left to make.

## How a wallet uses it

A wallet provides seven things: sending a transaction, a secret store,
storage, a relay, exchange rates, swaps, and its account details. They are the
seven interfaces of `SPEC.md` §15, 21 calls in all; everything else is the
library's job.

Rust wallets use the crates directly; Dart and Flutter wallets use
`splitz_core` and `splitz_host`, held to the same answers as Rust by the
differential and parity lanes. Kotlin, Swift and JavaScript wallets use the
generated binding: pass in the facts a call needs and get an answer back, with
no callbacks across the language boundary.

`INTEGRATING.md` walks through it, with working samples in Kotlin, Dart and
JavaScript. The library stops at the payment request; your wallet builds and
signs the transaction.

## Use it in your wallet

Nothing is on a package registry yet: every crate and Dart package is marked
unpublishable. Depend on this repository by git, pinned to a commit.

| your wallet | depend on | what you get |
|---|---|---|
| Dart or Flutter | `splitz_core` and `splitz_host` | the protocol and the whole wallet layer: signing, sealing, the log, relay sync, swaps, activity, address checks, the guard against paying twice, a check of what is about to be signed, payments that arrived, totals across bills, ZEC prices, and checks for your own seams |
| Rust | `splitz-core` and `splitz-host` | the same, in Rust |
| Kotlin, Swift, JavaScript | `splitz-ffi`, built by `tools/package/{android,ios,npm}.sh` | the protocol through a generated binding — address checks, a check of what is about to be signed, payments that arrived, totals, prices and plain-language refusals included — and a relay client (`SplitzRelay`) in that language; storage and the guard against paying twice are yours to write |

Dart — both packages, **pinned to the same commit, by its full 40-character
sha**. `splitz_host` reaches `splitz_core` by a path inside the repository,
which pub resolves to the full sha of that commit, and pub compares refs as
written: a branch name, or a short sha, names the same commit and is still
refused as a different ref.

```yaml
dependencies:
  splitz_core:
    git:
      url: https://github.com/KamaIOps/Splitz-Protocol.git
      path: dart
      ref: <full commit sha>
  splitz_host:
    git:
      url: https://github.com/KamaIOps/Splitz-Protocol.git
      path: splitz_host
      ref: <full commit sha>
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

Every amount is a whole number of the smallest unit; no floating point touches
money. When a split doesn't divide evenly, the leftover units go out in a
fixed order, so the parts add back to the total on every device.

Debts are cancelled against each other, then people are split into as many
groups as possible whose debts balance to zero. A group of `k` people needs
`k−1` payments, so more groups means fewer payments. Up to 14 people it finds
the true minimum; above that it approximates, and says which it did.

The history is append-only and merging copies is a union, in any order and any
number of times, so two people who saw different parts of it see the same
bill.

## Verify it yourself

Needs Dart 3.11.4 or later and Rust 1.88 or later, the minimum the crates
declare and CI builds them with from the lockfile. Run each command from the repository root; the comment beside
it says what a pass looks like.

```
cd dart && dart test                 # All tests passed! — every shared
                                     #   case in vectors/ among them
cd splitz_host && dart test          # All tests passed!
cd rust && cargo test                # every "test result: ok"
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

Fixed test cases only cover what someone wrote down, so there are checks
around them:

- **Same input, different code** (`tools/differential`) — unwritten inputs run
  through every implementation, answers compared.
- **Same surface** (`tools/parity`) — a function one side has and the other
  doesn't.
- **The spec against the code** (`tools/spec`) — every error code, figure and
  cross-reference in `SPEC.md` still true of the tree.
- **Somebody else's reader** (`tools/oracle`, `rust/splitz-core/tests/oracle.rs`)
  — timestamps against Python's date library, payment requests against
  `librustzcash`, since everything else shares one author.
- **A real wallet in four languages** (`tools/ffi`) — Kotlin, Swift, Dart and
  JavaScript each drive a whole bill; Swift through the packaged xcframework.
- **Properties over generated bills**
  (`dart/test/withholding_property_test.dart`,
  `rust/splitz-core/tests/withholding_property.rs`,
  `splitz_host/test/honest_lifecycle_property_test.dart`) — an honest payer is
  never refused and never asked for more than they owe.
- **The packaged libraries, run** (`tools/ffi/swift-simulator.sh`,
  `tools/ffi/aar-device.sh`, `tools/package/npm.sh`) — the same bill on an
  iOS simulator, an Android emulator and from a packed npm package. Run by
  hand; not in CI.
- **Four devices and an intruder** (`splitz_host/test/rehearsal_test.dart`) —
  four devices sync through a relay in different orders and end with one bill;
  then someone with the key redirects a payout, and settlement stops.
- **Does the money arrive** (`tools/regtest`) — on a local chain, a payment
  request is built, sent and mined, and the recipient's balance checked
  against splitz's figure. Needs Docker.
- **Samples and documents** (`tools/docs`, `tools/examples`,
  `tools/web-target`) — every sample in the docs is a file that runs, and the
  package still refuses to build for JavaScript, where 64-bit amounts lose
  precision.

## Conformance

An implementation conforms when it reproduces every case in `vectors/` and
follows §14, which covers what the wallet must do beyond the wire format.

`SPEC.md` §12 lists 96 reasons the library refuses something, each with a
test case except `bill_not_scalar_values`, which the spec excuses: its input
is a document any correct JSON reader rejects, so each implementation carries
it in its own suite instead.

`CONFORMANCE.md` explains what passing proves; `vectors/README.md` describes
the file format.

## Docs

- `SPEC.md` — the protocol itself.
- `INTEGRATING.md` — what a wallet has to provide, and what it gets back.
- `CONFORMANCE.md` — what conformance means.
- `vectors/README.md` — the test case format.

## Licence

MIT or Apache-2.0, whichever you prefer. See `LICENSE-MIT` and
`LICENSE-APACHE`.
