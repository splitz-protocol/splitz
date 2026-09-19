# splitz

A shared-bill protocol for Zcash wallets. Expenses go in; out come the fewest
payments that settle them, and the ZIP 321 payment request URI that carries one
payer's whole obligation in a single transaction.

No wallet dependency, no network, no storage, no framework. Two
implementations, one specification, one set of vectors that both run.

```
├── SPEC.md         the protocol
├── INTEGRATING.md  what a wallet supplies, and what it does not get
├── vectors/        408 language-neutral conformance cases
├── dart/           reference implementation  (0 dependencies)
└── rust/           second implementation     (serde_json, for the wire format)
```

```
cd dart && dart test                 # 427 tests, 408 of them the corpus
cd rust && cargo test                # the same 408 cases, plus the oracle
cd rust && cargo test --release      # and again with overflow checks off
tools/differential/run.sh 1 1200     # three implementations, one operation list,
                                     #   diffed against each other and against
                                     #   §10.2's own properties
python3 tools/spec/claims.py         # SPEC.md's claims, against the tree
python3 tools/oracle/instants.py     # §9.3's instants, against a reader
                                     #   nobody here wrote
python3 tools/parity/surface.py      # the two public surfaces, diffed
tools/web-target/run.sh              # asserts it still does not compile to JS
cd dart && dart run example/dinner.dart
```

## Why a protocol and not a library

A splitting library is easy; two wallets agreeing on a split is not. Six people
at one table hold six copies of the same bill on six devices, and any
disagreement about a rounded cent leaves a bill that never closes. The hard
part is not the arithmetic. It is that everyone's arithmetic must be identical,
and "identical" has to be demonstrable rather than assumed.

So the deliverable is the specification and the vectors. The two
implementations exist to show the specification is implementable and
unambiguous, which is a claim a single implementation cannot make about itself.

## What it does

- **Five split methods**: equal, exact amounts, percentage in basis points,
  shares, and itemised with tax and tip apportioned by what each person
  actually ate.
- **Exact money arithmetic.** Integer minor units throughout; no floating point
  touches an amount at any point. Every split conserves the total to the last
  unit by largest-remainder allocation, deterministically, so every device
  computes an identical split from identical inputs.
- **Provably minimal settlement.** Balances are netted, then partitioned into
  as many zero-sum groups as possible; a group of `k` needs exactly `k−1`
  payments, so maximising groups minimises payments. Exact up to 14
  participants, greedy above it, and the plan says which ran rather than
  claiming minimality it has not established.
- **Routing provenance.** Netting reroutes payments, so people get asked to pay
  someone they never ate with. Each settlement carries the original debts it
  discharges, and says whether any part of it is owed elsewhere, so a UI can
  explain the payment instead of asking for trust.
- **ZIP 321 output.** One payer, one transaction, one output per recipient, in
  a single canonical rendering so two wallets can compare byte for byte.
- **An append-only log** that merges by set union: idempotent, commutative,
  associative, including when two entries arrive under one id, which is
  resolved by content rather than by whichever arrived last. Two devices that
  saw different subsets of the history materialise the same bill.
- **Entries that speak only for their author.** Anyone may add a participant;
  only that participant may change their own payout address, and any address
  that does change is reported to the caller rather than silently used. Ids are
  unauthenticated, so this narrows the attack rather than closing it. See
  `SPEC.md` §10.4.

## A worked example

`dart run example/dinner.dart` — Ana covers a 4800.00 MXN dinner split three
ways; Ben covers a 300.00 taxi he shared with Cai.

```
Net positions
  ana  $3200.00
  ben  -$1450.00
  cai  -$1750.00

Settlement: 2 payments
  ben pays ana $1450.00
  cai pays ana $1750.00
        $1600.00 of what cai owes ana
         $150.00 of what cai owes ben

zcash:u1ana...?amount=0.15263158&fiat=MXN:145000&label=Ana
```

## Conformance

An implementation is conformant when it reproduces every case in `vectors/`.
See `vectors/README.md` for the format, and for how the expectations were
produced: a reference written from the specification text, not from either
shipped implementation, because a corpus generated from an implementation
cannot contain a case that implementation is self-consistently wrong about.

All but one of the 77 refusal codes in `SPEC.md` §12 have a case.
The exception is `bill_not_scalar_values`: its input is a document carrying a
lone surrogate, which a conformant JSON reader refuses, so a vector carrying
one would make the corpus unreadable rather than test the code. It is covered
by a test in each implementation whose string type can hold the input.

## The five lanes, and what each catches that the others cannot

| Lane | What it sees |
|---|---|
| `vectors/` | Both implementations against one fixed corpus. |
| `tools/differential` | Inputs nobody wrote an expectation for, answered by every implementation and diffed against each other. Catches what a corpus generated from one reference structurally cannot. |
| `tools/parity` | The two public surfaces. An API one side has and the other does not never reaches the wire, so nothing watching the wire can see it. |
| `rust/tests/oracle.rs` | Our payment request URIs against `librustzcash`'s `zip321` crate, byte for byte and round-tripped, over real mainnet addresses. Every other lane compares implementations written from one specification by one author; they can all be wrong together. **No transaction has been broadcast from a URI this library produced** — that is a wallet's milestone, not a library's, and the oracle is the closest thing to it here. |
| `tools/web-target` | That the package still refuses to compile to JavaScript. A JS number is exact only to 2^53−1; amounts here are 64-bit, so compiling would round them silently rather than fail. |

## Integrating it into a wallet

`INTEGRATING.md` has the whole surface. The short version is four steps in
three calls, and the URI is where the library stops: the wallet decodes the addresses, builds
the transaction and signs it.

## Licence

MIT or Apache-2.0, at your option. See `LICENSE-MIT` and `LICENSE-APACHE`.
