# Conformance vectors

1016 cases across 24 files. An implementation is conformant when it reproduces
all of them.

## Shape

```json
{ "description": "…", "count": 18, "cases": [ … ] }
```

Every case has a `name`, its inputs, and exactly one of:

- `"expect": <value>` — the implementation MUST produce this value;
- `"error": "<code>"` — the implementation MUST refuse the input with this
  code from `SPEC.md` §12.

A case that folds a log carries neither: the fold reports what it set aside
rather than refusing the whole log (§10.3), so the codes appear inside
`expect.setAside`.

**Refusing for the wrong reason is a failure.** The distinction between "this
bill is from a newer version" and "this bill is damaged" is the difference
between telling a user to update and telling them to give up.

`withholdings.json` and `writers.json` are the files whose subject is the
wallet rather than the wire. SPEC.md §14 is addressed to a host, so an implementation can keep
every rule in §1–§12 and still ask somebody to pay a debt they have already
paid, or send the balance of a bill to whoever minted the second claim on an
id. A wallet is not conformant without it.

Amounts are integer minor units throughout. Instants are canonical (§9.3).
Objects keyed by participant id are compared by content, not by key order.

## Files

| File | Input | Expected |
|---|---|---|
| `allocation.json` | `total`, `weights` | the parts |
| `split-methods.json` | `total`, `split` | owed minor units per id |
| `balances.json` | `bill` | net positions, creditors, debtors, direct debts |
| `settlement.json` | `balances`, `exactLimit` | the plan |
| `coverage.json` | a `bill` or bare `balances`, `exactLimit` | the plan, each payment carrying the debts it discharges |
| `rate.json` | `rate`, `minorUnits` or `zatoshi`, `rounding` | the conversion |
| `zip321.json` | `payments`, `includeFiat` | the URI |
| `request.json` | `uri`, and `outputs` for a proposal | the payments read back, or what the proposal is missing and adds |
| `address.json` | `address` | its `network`, `kind`, `receivers` and `canReceiveMemo` |
| `bill-json.json` | `json` | the decoded bill |
| `log.json` | `log` and `billId`, or `entry`, or `left` and `right` | the fold, the entry's admission, or the merge |
| `invite.json` | `uri` to decode, or `invite` to encode, with `base` for a link | the fields, or the URI |
| `payload.json` | `payload` to decode, or `encode` | the contents, or the string |
| `sealed.json` | `frame` | the version, nonce and body length |
| `seal.json` | `entry`, or `billId` | the plaintext and nonce it seals under, or the channel |
| `withholdings.json` | `plan`, `bill`, `payer`, `contested`, `payAnyway` | what a request carries, and what is held back |
| `writers.json` | `payout` or `payment` | accepted, or the code a host refuses to write it with |
| `closing.json` | `log`, `actor`, `op` (`settle`, `expense`, `close`, `reopen`) | accepted, whether a reopening is written, or the code a host refuses with (§14.9) |
| `delta.json` | `log`, `theyHave` | nothing missing, one square, or past the cap |
| `scan.json` | `text` | a bill code read, or refused when its key is not the bill's (§9.4) |
| `messages.json` | `code` | the sentence a host may show for it |

Two cases in `zip321.json` carry `repeatPayment` and `paymentCount` instead of
a literal list, because ten thousand payments would make the file unreadable. A
suite builds the list from them.

## How the expectations were produced

`tools/corpus/` implements the computational sections of `SPEC.md` **from the
specification text**, and every expectation in this corpus comes from it.

No shipped implementation produced any of them. A corpus generated from an
implementation cannot contain a case that implementation is self-consistently
wrong about, and the point of a second implementation is to find exactly those.

Each generator carries its own check: allocation and the split methods assert
that shares sum to their total, rate conversions are recomputed from the
formula in §7.1, every rendered URI amount is parsed back to the zatoshi it
came from, every encoded payload is decoded back to the log it carried, and
every settlement plan is checked to leave each participant at zero. Every
address in `address.json` is checked against the network, kind and receivers
its source names.

## An external oracle

`rust/tests/oracle.rs` checks this implementation against `librustzcash`'s
`zip321` crate, stock from crates.io, two ways. It renders the same payment
with both and compares byte for byte — 14 cases; the other 3 emit `fiat` or
carry ten thousand payments, neither of which librustzcash can express. And it
parses every URI the corpus states, checking the recipients and amounts come
back unchanged: 17 from `zip321.json`, 20 from `obligations.json`.

The rendering half uses this crate's own renderer rather than the corpus
string. Comparing the corpus would leave a defect in the renderer invisible
here, caught only transitively. It also settles §8.4's claim that a parser predating the
proposed `fiat` parameter ignores it — the stock crate is exactly such a
parser, and all three fiat-bearing URIs parse to the same zatoshi.

That crate decodes each address into a `ZcashAddress`, which is why every
address here is a real one rather than filler.

The same file puts every case in `address.json` through `zcash_address`, the
decoder those wallets use, and compares network, kind, receivers and whether a
memo can be delivered. §8.6 is stricter than that crate in five ways — nothing
is trimmed, Bech32 padding bits are zero, a revision 0 Unified Address carries
Sapling or Orchard and no MUST-understand typecode, and Sprout is refused — so
the eleven cases those rules refuse are named in the test, which asserts the
crate accepts each. Every other case agrees. Nothing §8.6 accepts is refused
there. And each `zip321.json` case refused with `zip321_memo_undeliverable` is
one whose payment `zip321::Payment::new` refuses with `TransparentMemo`, while
no other case is.

## One guard with no distinguishing case

`allocation.json`'s `product_overflows` refuses with `allocation_overflow`, as
§3 requires. It does not pin step 4 specifically: on a 64-bit runtime the
wrapped product always leaves a leftover outside `0 ≤ leftover < n`, so step 6
refuses the same input with the same code. Fifty-four overflowing inputs were
probed and every one was caught by either guard. Removing step 4 turns nothing
red here. It is kept because it names the fault at the point it happens, and
because a runtime that wraps differently may not be so obliging.

