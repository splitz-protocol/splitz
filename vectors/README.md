# Conformance vectors

476 cases across 18 files. An implementation is conformant when it reproduces
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

`withholdings.json` is the only file whose subject is the wallet rather than
the wire. SPEC.md §14 is addressed to a host, so an implementation can keep
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
| `bill-json.json` | `json` | the decoded bill |
| `log.json` | `log` and `billId`, or `entry`, or `left` and `right` | the fold, the entry's admission, or the merge |
| `invite.json` | `uri` to decode, or `invite` to encode | the fields, or the URI |
| `payload.json` | `payload` to decode, or `encode` | the contents, or the string |
| `sealed.json` | `frame` | the version, nonce and body length |
| `seal.json` | `entry`, or `billId` | the plaintext and nonce it seals under, or the channel |
| `withholdings.json` | `plan`, `bill`, `payer`, `contested`, `payAnyway` | what a request carries, and what is held back |
| `delta.json` | `log`, `theyHave` | nothing missing, one square, or past the cap |

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
every settlement plan is checked to leave each participant at zero.

## An external oracle

`rust/tests/oracle.rs` checks this implementation against `librustzcash`'s
`zip321` crate, stock from crates.io, two ways. It renders the same payment
with both and compares byte for byte — 11 cases; the other 3 emit `fiat` or
carry ten thousand payments, neither of which librustzcash can express. And it
parses every URI the corpus states, checking the recipients and amounts come
back unchanged: 14 from `zip321.json`, 9 from `obligations.json`.

The rendering half uses this crate's own renderer rather than the corpus
string. Comparing the corpus would leave a defect in the renderer invisible
here, caught only transitively. It also settles §8.4's claim that a parser predating the
proposed `fiat` parameter ignores it — the stock crate is exactly such a
parser, and all three fiat-bearing URIs parse to the same zatoshi.

That crate decodes each address into a `ZcashAddress`, which is why every
address here is a real mainnet Unified Address rather than filler.

## One guard with no distinguishing case

`allocation.json`'s `product_overflows` refuses with `allocation_overflow`, as
§3 requires. It does not pin step 4 specifically: on a 64-bit runtime the
wrapped product always leaves a leftover outside `0 ≤ leftover < n`, so step 6
refuses the same input with the same code. Fifty-four overflowing inputs were
probed and every one was caught by either guard. Removing step 4 turns nothing
red here. It is kept because it names the fault at the point it happens, and
because a runtime that wraps differently may not be so obliging.

