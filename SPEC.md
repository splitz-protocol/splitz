# The Splitz Bill Protocol

A shared bill is a set of expenses among a set of people, settled in the fewest
Zcash payments that clear it. This document specifies every computation and
every encoding the parties must agree on, so that six devices holding six
copies of one bill arrive at the same answer about who owes what.

The wire format is version 1. A reader refuses a version above the one it
implements rather than guessing at it.

Key words **MUST**, **MUST NOT**, **SHOULD**, **SHOULD NOT** and **MAY** are to
be read as in RFC 2119.

## 1. Scope and conformance

An implementation is conformant when it reproduces every case in `vectors/`.
Each vector file pins one part of this document:

| File | Section |
|---|---|
| `allocation.json` | 3 |
| `split-methods.json` | 4 |
| `balances.json` | 5 |
| `settlement.json` | 6 |
| `rate.json` | 7 |
| `zip321.json` | 8 |
| `obligations.json` | 8.4 |
| `bill-json.json` | 2, 9 |
| `log.json` | 9.2, 10, 10.5 |
| `signing.json` | 10.6 |
| `authority.json` | 10.7 |
| `invite.json` | 11.1 |
| `payload.json` | 11.2 |
| `sealed.json` | 11.3 |

Every case carries either an `expect` value or an `error` code. An
implementation MUST produce the stated value, or refuse the input with the
stated code from §12.

**Refusing for the wrong reason is a conformance failure, not a detail.** The
code is what a wallet turns into a sentence for its user, and the remedy
differs: `payload_future_version` means update the app, `payload_missing_body`
means the data is damaged. A reader that returns either for both sends half its
users to the wrong fix.

This document specifies computation and encoding. Transport, encryption,
storage, key management, address validation, transaction construction and
signing are the wallet's; §13 lists them so they are not mistaken for
omissions. §14 states what this protocol nonetheless requires of a host, and
§15 specifies the seam through which a wallet supplies the rest. Conformance
with §15 is a separate claim from conformance with the corpus: a wallet may
keep one and not the other.

## 2. Money and amounts

An amount is a signed integer count of the smallest indivisible unit of one
currency. **An amount MUST NOT be a floating point value at any point in its
life, including in transit.**

### 2.1 Currency

A currency is an ISO 4217 alpha-3 code in upper case: exactly three characters,
each `A` to `Z`. A reader MUST refuse anything else with `bill_bad_currency`,
wherever a currency arrives — the bill document, an expense or payment payload,
an entry's own field, a rate.

Lower case is refused, not folded. `usd` and `USD` are one currency to a person
and two to the byte order of §2.3, so admitting both makes `currency_mismatch`
fire between amounts a user considers the same.

**The exponent is not carried.** The number of minor units in one major unit is
the currency's ISO 4217 exponent, and this protocol never transmits it: `1234`
is €12.34 in `EUR` (exponent 2), ¥1234 in `JPY` (exponent 0), and 1.234 KWD in
`KWD` (exponent 3).

Because it is not carried, **shape is the whole of what this protocol can
check**. `XAU` is a well-formed code that ISO 4217 assigns no minor unit; an
amount denominated in one has no scale at which a figure typed in major units
means anything. A reader that renders or accepts an amount in major units MUST
therefore also refuse a code for which its own ISO 4217 register gives no
exponent. That register is the reader's, not this protocol's. The requirement
is stated here so that it is not independently forgotten by every reader.

Arithmetic between two currencies MUST be refused with `currency_mismatch`
rather than converted.

### 2.2 Bounds

Every amount MUST be representable in a signed 64-bit integer, and so MUST
every total, sum and intermediate product formed from amounts. An operation
whose result would exceed that range MUST be refused with `amount_overflow`, or
with the narrower code §3 and §7 name, and MUST NOT be allowed to wrap.

The bound is stated as a number rather than left to whichever integer type an
implementation happens to have, so that every implementation accepts and
refuses the same inputs. A wrapped total satisfies every check downstream of
it, which is what makes it dangerous rather than merely wrong.

Implementers should note where the boundaries fall, because they differ by
runtime: a signed 64-bit maximum is 19 digits, an unsigned one 20, and an
IEEE-754 double is exact only to 2^53−1, which is 16. A bound chosen in one and
consumed in another is a silent interoperability bug.

### 2.3 Ordering identifiers

Wherever this document says identifiers are in **ascending** order —
participant ids, entry ids, currency codes, instants compared as text — the
order is the **lexicographic order of the UTF-8 encoding, byte by byte**, with
a shorter string ordering before a longer one it prefixes.

**An implementation MUST NOT use its language's native string comparison unless
that comparison is UTF-8 byte order.** Dart compares UTF-16 code units and Rust
compares UTF-8 bytes. The two disagree for every character outside the Basic
Multilingual Plane: `"p\u{1F600}"` sorts before `"p\u{FFFD}"` in UTF-16 and
after it in UTF-8. Leftover minor units, settlement ties and log order are all
resolved by ascending id, so a disagreement there is two devices paying
different amounts to different people.

Every string this order is defined over MUST be a sequence of Unicode scalar
values. A document or payload carrying one that is not MUST be refused.

A lone surrogate has no UTF-8 encoding, so the order above says nothing about
it, and an implementation that encodes anyway substitutes U+FFFD for every such
code point: two identifiers that are not equal then compare equal, and
"ascending id" stops being a total order on exactly the inputs a peer chooses.
A document carrying one is refused with `bill_not_scalar_values`.

Rust's JSON parser refuses such a document before this check is reached. Dart's
accepts it, and Python's accepts it and then raises on encoding, so both MUST
apply the check explicitly at document and payload ingress.

### 2.4 One bill, one currency

A bill has exactly one currency. Every expense and every payment on it is
denominated in that currency.

## 3. Allocation

`allocate(total, w₀…wₙ₋₁) → p₀…pₙ₋₁` distributes an integer `total` across
non-negative integer weights so that `Σpᵢ = total` exactly.

Let `W = Σwᵢ`, `s = sign(total)` and `m = |total|`.

1. **`W` MUST be greater than zero.** An empty weight list is refused with
   `empty_weights`, a negative weight with `negative_weight`, and weights
   summing to zero with `zero_weight_sum`.

2. **`W` MUST be accumulated without overflow**, refused with
   `weight_sum_overflow` otherwise. `W` is formed before any product, so it is
   the first value that can wrap. A wrapped `W` is negative, every part then
   divides to a plausible wrong number, and step 5's guarantee fails silently.

3. **`total` MUST NOT be the most negative 64-bit integer**, refused with
   `allocation_overflow`. That value has no positive counterpart: taking its
   magnitude is the identity, so every part comes back with the wrong sign.

4. **Each product `m · wᵢ` MUST fit a signed 64-bit integer**, refused with
   `allocation_overflow` otherwise. A wrapped product allocates a plausible
   wrong number, which passes every check downstream of it.

5. `pᵢ = ⌊m · wᵢ / W⌋` and `rᵢ = (m · wᵢ) mod W`.

6. `leftover = m − Σpᵢ`. Order the indices by **descending `rᵢ`, ties broken by
   ascending index**, and add one unit to each of the first `leftover`.

   Each part discards a fraction below one, so `0 ≤ leftover < n`. An
   implementation MUST check that rather than assume it, refusing with
   `allocation_overflow` if it does not hold. The distribution hands out one
   unit per index and would double-credit otherwise, which is exactly what a
   wrapped `W` produces.

7. If `s` is negative, negate every `pᵢ`.

The result is a function of the inputs alone. Two devices holding the same
expense compute the same split without exchanging anything, which is what lets
a bill close without reconciliation.

`allocateEvenly(total, count)` is `allocate(total, [1] × count)`. Earlier
indices absorb the extra units.

## 4. Split methods

A split method resolves an expense total into owed minor units per participant.
**Every method MUST produce shares summing exactly to the expense total.**

Participants are ordered by ascending id (§2.3) before allocation in every
method, so the leftover units of §3.6 land on the same people everywhere.

**No share may carry the sign opposite to the expense total, in any method.**
A share of the same sign as the total, or of zero, is admissible; one of the
opposite sign is refused with `negative_share`.

`equal`, `percentage` and `shares` inherit this from §3.1, which refuses a
negative weight, and from §3 step 7, which gives every share the sign of the
total. `exact` and `itemized` MUST check it themselves, because their values
never reach the allocator.

The rule is about sign agreement rather than about negativity because §10.4
makes a refund an expense with a negative total, and all five methods MUST
divide one. A rule refusing every negative share would leave `exact` and
`itemized` unable to express a refund the other three divide.

What the rule forbids is a share pulling against its own expense: without it
the stated amounts sum to the expense total at any magnitude, turning a
five-unit bill into an obligation bounded only by §8.1.

### 4.1 `equal`

`{"type": "equal", "among": [id, …]}`

Duplicate ids collapse. The surviving set is sorted and the total allocated
evenly across it. An empty `among` is refused with `empty_split`. An id
appearing more than once counts **once**: `among` names a set, so a list
holding a participant twice splits the expense the same way as one holding
them once. An implementation that weighted a repeat would charge that person
twice for the same meal and disagree with every other reader of the log.

### 4.2 `exact`

`{"type": "exact", "amounts": {id: minorUnits, …}}`

A `type` this specification does not define is refused with
`bill_unknown_split_type`. The five below are the whole set; a sixth is not a
split a second implementation could agree with.

The amounts are the shares. No amount may carry the sign opposite to the
expense total (`negative_share`), their sum MUST be formed without overflow
(`amount_overflow`), and it MUST equal the expense total, else
`exact_total_mismatch`. No allocation runs and nothing is
rounded.

### 4.3 `percentage`

`{"type": "percentage", "basisPoints": {id: bp, …}}`

Basis points MUST be non-negative, MUST sum without overflow, and MUST sum to
exactly `10000`, else `percentage_not_full_scale`.

**The overflow check comes first.** Four values of `2^62` sum to zero in a
64-bit integer, and a fifth of `10000` then satisfies the full-scale check.

Percentages are carried in basis points so a share is an integer at every step.
The total is allocated with basis points as weights.

### 4.4 `shares`

`{"type": "shares", "shareCounts": {id: count, …}}`

The total is allocated with share counts as weights. A participant with zero
shares owes nothing; all counts zero is refused with `zero_weight_sum`.

### 4.5 `itemized`

```json
{"type": "itemized",
 "extraMinorUnits": 5000,
 "items": [{"description": "tacos", "minorUnits": 5200, "sharedBy": ["ben", "eli"]}]}
```

1. `items` MUST be non-empty (`itemized_no_items`), and every item MUST be
   assigned to at least one participant (`itemized_unassigned_item`).

2. No item cost and no `extraMinorUnits` may carry the sign opposite to the
   expense total (`negative_share`). `Σitem.minorUnits + extraMinorUnits` MUST
   be formed without overflow and MUST equal the expense total, else
   `itemized_total_mismatch`.

   **These checks are listed in the order an implementation MUST apply them**,
   so an input failing two of them is refused with the same code everywhere.

3. Each item's cost is allocated evenly across its deduplicated, sorted
   `sharedBy`. Summing per participant gives their subtotal.

4. `extraMinorUnits` — tax, tip, service — is allocated across participants
   with the **magnitudes** of their subtotals as weights, so people pay tax in
   proportion to what they ate. Magnitudes rather than the subtotals themselves
   because §3.1 refuses a negative weight, and a refund's subtotals are all
   negative; a magnitude preserves the same proportion whichever sign the
   expense carries. When every subtotal is zero the extra is allocated evenly
   instead, since weights that are all zero have no proportion to preserve.

## 5. Balances and direct debts

### 5.1 Net balances

Every participant on the bill appears, including those who net to zero. Starting
from zero for each:

- an expense adds its full amount to the payer, and subtracts each
  participant's owed share from that participant;
- a **confirmed** payment (§10.5) adds its amount to the payer and subtracts it
  from the payee.

A payment discharges debt without changing what the bill cost, which is the
same movement an expense the payer covered would make. Paying on someone else's
behalf therefore needs no special case.

**A payment that is recorded and not yet confirmed takes no part in this.** It
is a claim, and the person who owes the money is the one making it. A bill that
counted it would clear a debt on the word of the only party with a reason to
misstate it. What discharges a debt is the person paid saying the money
arrived.

Positive means the bill owes the participant. **Balances MUST sum to zero**; a
non-zero residual means the bill is inconsistent.

**The residual is a property of the set, and MUST be decided without forming a
value the set does not already contain.** A running total over participants
exceeds a signed 64-bit integer at some orderings of a set whose total is zero
and whose every member is representable, and the order a map yields is an
implementation's, not the document's. So does the sum of the positives. A
reader doing either refuses bills its neighbour accepts, and the bill never
closes.

Decide it by cancellation: take the largest amount owed and the largest amount
owing, subtract the smaller magnitude from the larger, and return any remainder
to its side. Every value formed is no larger than one already in the set. The
residual is zero exactly when both sides empty together.

**A bill the fold returns always has balances §2.2 can hold.** Balances are
formed in the order this section states them — expenses in the order the fold
applies them, each crediting its payer before debiting each share, then
confirmed payments in the order the bill lists them — and the fold (§10.3)
applies each entry against the balances formed so far. An expense whose effect
would carry a balance out of range is set aside with `amount_overflow`, and so
is every confirmation of a payment that would; the payment stays unconfirmed.
A payment is set aside the same way when it would carry the total one
participant has recorded paying another, confirmed or not, out of range —
§14.4 sums the unconfirmed part of that total. Without these, one expense of
the largest amount §2.2 admits, written by anyone holding the invite, leaves
§5 refusing the whole bill on every device and nobody able to settle.

`creditors` is the positive balances, most owed first, ties by ascending id.
`debtors` is the negative balances, largest debt first, ties by ascending id.

### 5.2 Direct debts

The debts as they arose, before any netting: for each expense, every
participant other than the payer owes the payer their share.

Debts are aggregated per (debtor, creditor) pair and ordered by debtor then
creditor, both ascending. Zero-valued pairs are dropped.

**Recorded payments are not subtracted here.** These are the debts the bill
created, and §6.3 reads them to explain a rerouted payment.

## 6. Settlement

### 6.1 The minimisation

Take the participants with a non-zero net balance, ordered by ascending id.
Partition them into as many groups as possible where each group's balances sum
to zero. **A group of size `k` is cleared in exactly `k−1` payments, so
maximising the number of groups minimises the total.**

Within a group, repeatedly pay the largest debtor's balance to the largest
creditor, transferring `min(|debt|, credit)`. Each step zeroes at least one
participant. Where two participants tie on magnitude, the one earlier in
ascending id order is chosen, which keeps the plan a function of the inputs.

The resulting settlements are sorted by payer then payee, both ascending.

### 6.2 The exact limit

**A subset sum that cannot be formed in a signed 64-bit integer is not zero,
and the subset is not a group.** The search MUST track which of its sums are
exact and MUST NOT read an inexact one as zero. It MUST NOT wrap, and MUST NOT
refuse the bill: a subset too large to add is simply not a group, while the
bill around it may settle perfectly.

This has to be tracked per subset rather than bounded once over the whole set.
A running total over participants in one order overflows and in another does
not, and the order is chosen by whoever joined. A wrapped subset sum that lands
on zero is read as a group that settles, and the plan then reports itself
optimal while clearing nothing.

Balances reaching the search MUST also satisfy §5.1. A set that does not — taken
directly by §6 rather than derived from a bill, where §4 makes a non-zero
residual impossible — is refused with `balances_nonzero_residual`.

The partition search enumerates submasks: it allocates `2ⁿ` entries and costs
`3ⁿ`.

An implementation MUST solve exactly when the number of non-zero participants
is at most `exactLimit`, whose default is **14**, and MUST report
`isOptimal: false` when it falls back to treating the whole set as one group.

**`exactLimit` MUST NOT exceed 20**, refused with `exact_limit_too_large`.

The ceiling is normative because the cost is superlinear and the failure past
it is silent. At 26 the search does not finish. At 64 the shift `1 << n`
overflows, and the search returns an empty plan that still reports itself
optimal.

A plan that overshoots by one payment is acceptable. A plan that claims
minimality it has not established is not.

### 6.3 Coverage

Netting reroutes payments, so a participant is often asked to pay someone they
never transacted with. Each settlement therefore carries the original debts it
discharges.

A payer's direct debts (§5.2, in their existing order) are consumed against
that payer's settlements in order, each settlement taking as much of each
remaining debt as it needs.

A settlement is **rerouted** when any part of it discharges a debt owed to
someone other than its payee — that is, when the covered amounts naming a
creditor other than `to` sum above zero.

**Membership is not the test.** A payment covering a little of what the payee
lent and a great deal of what two other people lent is still a payment the
payer cannot account for by looking at the payee. Testing only whether the
payee appears somewhere in the coverage reports such a payment as direct.

Only a positive debt row is consumed. A pair aggregating to a negative amount
is a credit on that pair rather than a debt, and discharges nothing.

A payer who is also owed money settles less than they directly owe, so some
debts are left uncovered. Those are offset by what the payer is owed, not
unpaid.

**A settlement may also carry more than its coverage explains, and the excess
MUST be reported.** §10.4 lets anybody holding the invite write an expense, and
§4 admits a negative total, so a peer can attribute a refund to somebody who
never agreed to it. The victim's settlement then exceeds every debt the bill
records for them, and coverage — the one line a payer has to answer "why do I
owe this" — goes quiet precisely when it is needed. A settlement whose covered
amounts sum below its own amount states the difference as `unexplained`.

For a plan computed from balances, where §6.3 requires coverage to be empty,
every settlement is wholly unexplained. That is the true answer and not a
defect: no debts were supplied, so nothing about the plan can be explained to
the payer.

**Coverage requires the bill.** Net balances alone do not carry the debts that
produced them, so a plan computed from balances carries no coverage and every
settlement in it reports as direct. An implementation planning from balances
MUST leave coverage empty; one planning from a bill MUST populate it.

## 7. Rates

A bill is denominated in fiat; settlement happens in ZEC. **The rate is part of
the bill's shared state, snapshotted by a `setRate` entry (§10.1), not looked
up per device.** Six people
applying six live rates to one dinner compute six different amounts and the bill
never closes.

A rate is `{currency, minorUnitsPerZec, at, source?}`, where `minorUnitsPerZec`
is the price of one ZEC in the currency's minor units: a ZEC worth US$512.34 is
`51234`.

### 7.1 Fiat to zatoshi

One ZEC is `100000000` zatoshi.

```
numerator = minorUnits × 100000000
zatoshi   = numerator / minorUnitsPerZec        (integer division)
remainder = numerator mod minorUnitsPerZec
```

**The multiplication precedes the division** so a small amount at a high ZEC
price does not collapse to zero.

When `remainder` is non-zero the last zatoshi is decided by the rounding mode:

| Mode | Result |
|---|---|
| `up` | `quotient + 1` |
| `down` | `quotient` |
| `nearest` | `quotient + 1` when `remainder ≥ minorUnitsPerZec − remainder`, else `quotient` |

The half-way test is written as a comparison rather than as `remainder × 2`,
which wraps whenever the remainder reaches 2^62 and then rounds the wrong way.
Both sides of the comparison are bounded by `minorUnitsPerZec`, so neither can
exceed what the rate already contains.

**Settlement amounts MUST default to `up`.** A debt rounded down leaves a dust
balance behind and the bill never quite closes.

`minorUnitsPerZec` MUST be positive (`rate_not_positive`), in both directions
of conversion. The amount MUST be non-negative (`negative_amount`) and in the
rate's currency (`rate_currency_mismatch`).

`numerator` MUST fit a signed 64-bit integer, which bounds the amount at
**92233720368** minor units. A larger amount is refused with
`rate_amount_too_large` rather than wrapped.

### 7.2 Zatoshi to fiat

`⌊zatoshi × minorUnitsPerZec / 100000000⌉`, rounding halves up. `zatoshi` MUST
be non-negative, the price positive, and the product MUST NOT overflow.

**This is for display only** — a label under a number, not a number anyone
settles against. An implementation MUST NOT let its result reach a settlement
amount.

### 7.3 Payer obligations

A settlement plan is priced and grouped by payer, payers in ascending id order,
each payer's settlements in plan order. **One payer's obligation is one
transaction with one output per recipient.**

## 8. Payment requests

An obligation becomes a ZIP 321 payment request URI. This section constrains
ZIP 321 to a single canonical rendering, because two wallets that render the
same obligation differently cannot check each other.

### 8.1 Amount

`amount` is decimal ZEC. Let `coins = ⌊zatoshi / 100000000⌋` and
`zats = zatoshi mod 100000000`.

When `zats` is zero the value is `coins` with no decimal point. Otherwise it is
`coins`, a period, and `zats` padded to eight digits with **trailing zeros
removed**: `50000000` renders as `0.5`, never `0.50000000`. One value therefore
has exactly one representation.

The amount MUST be positive (`zip321_amount_not_positive`) and at most
`2100000000000000` zatoshi, being 21000000 ZEC (`zip321_amount_too_large`).

### 8.2 Indexing and parameter order

A request with one payment is written `zcash:<address>?<params>` with no
`address` parameter. A request with two or more is written `zcash:?<params>`
where every payment carries an explicit `address`.

The first payment carries the empty parameter index; payment `i` carries the
suffix `.i`. Indices run to `9999`; a request carrying more outputs than that
is refused with `zip321_too_many_payments`, and one carrying none with
`zip321_no_payments`. A request with no output asks for nothing and is not a
request.

Parameters within a payment MUST be emitted in this order:

```
address, amount, fiat, memo, label, message
```

Payments MUST be emitted in obligation order. Ordering is not semantically
significant to a ZIP 321 parser, but fixing it is what makes two
implementations' output comparable byte for byte.

### 8.3 Value encoding

- `label` and `message` are `qchar`. The unreserved set (`A–Z a–z 0–9 - . _ ~`)
  and `! $ ' ( ) * + , ; : @` are written literally. **Every** other byte of
  the UTF-8 encoding, including every non-ASCII byte, is written as an
  upper-case percent escape. `Zcon7 dîner ✨` becomes
  `Zcon7%20d%C3%AEner%20%E2%9C%A8`.

- `memo` is unpadded base64url. The decoded memo MUST be at most **512** bytes
  (`zip321_memo_too_large`). ZIP 321 requires the whole URI to be refused when
  a memo shares a parameter index with a transparent address; this protocol
  does not parse addresses, so an implementation MUST NOT attach a memo to a
  recipient it has not confirmed can receive one.

- An address is written verbatim. It MUST be present and non-empty
  (`zip321_no_address`) and **ASCII alphanumeric** — `A`–`Z`, `a`–`z`, `0`–`9`
  and nothing else — which is all the ZIP 321 grammar admits
  (`zip321_bad_address`): that grammar reads
  `zcashaddress = 1*( ALPHA / DIGIT )`, and RFC 3986's `ALPHA` and `DIGIT` are
  ASCII. A Unicode-aware test is a different rule: it admits U+00E9 and
  U+FF12, which are letters and digits and are not in the grammar. **This is a
  syntactic check, not validation:** a wallet MUST put every address through
  its own decoder — **ZIP 316** for a Unified Address, and the network's own
  rules for the receivers inside it. This protocol never decodes one, so it
  cannot tell a mainnet address from a testnet address, nor a Unified Address
  from a string that merely looks like one.

  **The address is checked before any other parameter of the same payment**, so
  a payment invalid in two ways is refused with the same code everywhere.

- A `label` MUST be at most **96** bytes of UTF-8 once decoded, truncated if
  longer at the last Unicode scalar value that fits whole — never inside one
  scalar's encoding, and without regard to grapheme clusters, which would make
  the cut depend on each implementation's Unicode tables. Display names are chosen by whoever they belong
  to, and nothing else in the pipeline bounds them.

### 8.4 Fiat price

`fiat` records what the `amount` at the same parameter index was priced as, as
`<CUR>:<minorUnits>`.

**It is the value of that payment, not the price of one ZEC.**
`amount=0.06&fiat=EUR:3000` is six hundredths of a ZEC priced at thirty euro. A
per-ZEC rate written here parses cleanly and is wrong by the ratio between the
rate and the amount.

The code MUST be exactly three upper-case letters
(`zip321_bad_currency_code`). The count MUST be greater than zero
(`zip321_fiat_not_positive`) and at most **18** digits
(`zip321_fiat_too_many_digits`), which keeps every admissible value inside a
signed 64-bit integer.

In a request with several payments, each carries its own `fiat` for its own
amount. One figure repeated across every index would be the total, and that is
not what any one recipient is owed.

**A `fiat` value is advisory. It MUST NOT be used to compute or adjust any
output value.**

`fiat` is a **proposed** ZIP 321 parameter, not one in a released version of
ZIP 321. Emitting it is therefore opt-in and off by default. It is safe to
emit: a parser that predates it treats it as an unrecognised parameter, ignores
it, and constructs exactly the payment `amount` specifies.

### 8.5 From obligation to URI

Each settlement in the obligation becomes one payment, in order. The address is
the recipient's `payTo`, and the label is the recipient's display name — a
payer asked to send money to someone they never ate with (§6.3) needs to
recognise the name on their wallet's confirmation screen.

A recipient the plan names who is not on the bill at all is refused with
`unknown_participant`. That is a merge or storage fault and needs a different
remedy from a missing address.

**A recipient who is on the bill but has published no `payTo` MUST NOT be
silently dropped**, because a dropped output settles less than the plan says it
does. An implementation MUST either refuse the whole request with
`zip321_no_address`, or render the payable outputs and **report the unpayable
recipients alongside the URI**. It MUST NOT do neither.

Refusing the whole request over one missing address is the safe default, but it
withholds every other output too, so the reporting form is what a wallet should
offer.

**A recipient whose published address is one §8.3 does not admit is excluded
the same way**, reported with the reason `bad_address`, and refused with
`zip321_bad_address` by an implementation that refuses rather than reports.
Anyone may publish any string as their own address, so without this one
participant's malformed `payTo` refuses every other output of every payer's
request.

The same holds for a recipient whose preferred payout is not a Zcash address at
all. A `swap` or `cash` payout (§9) cannot become an output of this URI, so
such a recipient is excluded for a reason that has nothing to do with a missing
address, and **MUST be reported alongside the URI exactly as an unpayable one
is.**

A URI that silently covers three of a payer's four debts is indistinguishable,
to the payer who sends it, from one that settles all four. They are told the
bill is settled and one person is still owed. Reporting these recipients is not
advisory copy; it is the only thing standing between the payer and that belief.

An implementation therefore reports, for one payer's obligation, three groups:
the outputs the URI carries, the recipients it cannot carry and why, and the
total each group accounts for. **A figure that prices the whole obligation MUST
NOT be presented as what the URI sends.**

## 9. The bill wire format

```json
{
  "v": 2,
  "id": "weekend",
  "name": "Zcon7 weekend",
  "currency": "MXN",
  "splitMode": "equal",
  "participants": [{"id": "ana", "name": "Ana", "payTo": "u1…"}],
  "expenses": [{
    "id": "dinner", "description": "dinner", "paidBy": "ana",
    "amount": 480000, "currency": "MXN",
    "at": "2026-10-28T19:30:00.000Z",
    "split": {"type": "equal", "among": ["ana", "ben"]}
  }],
  "payments": [{
    "id": "p1", "from": "ben", "to": "ana",
    "amount": 350, "currency": "MXN",
    "method": "shieldedZec", "at": "2026-10-28T19:30:00.000Z",
    "zatoshi": 36843,
    "paidAtRate": {"currency": "MXN", "minorUnitsPerZec": 950000, "at": "…"}
  }],
  "rate": {"currency": "MXN", "minorUnitsPerZec": 950000, "at": "…"}
}
```

### 9.1 Fields

**`v` is 1.** A `createBill` entry carries the fields §9.4 binds. `v` MUST be
present and MUST be an integer (`bill_missing_version`), and MUST be at least 1
(`bill_type_error`). A reader MUST refuse a version above the one it implements
(`bill_future_version`) rather than guess at it. A log document carries `v`
under the same rules.

**A version written as a string is not a version.** A reader that skips the
check when the type is wrong lets a future format present itself as this one.

**A field of the wrong type is refused with `bill_type_error`.** This applies to
optional fields exactly as to required ones: a reader MUST NOT treat a
wrong-typed `payTo`, `targetId`, `currency`, `splitMode` or `extraMinorUnits`
as absent. `payTo` is the address money is sent to, and an `extraMinorUnits`
silently defaulted to zero makes the §4.5.2 total check pass on a bill whose tax
has vanished.

**`currency`** MUST be non-empty (`bill_missing_currency`) and MUST be an
ISO 4217 alpha-3 code in upper case (`bill_bad_currency`, §2.1). This applies to
the bill's `currency`, to an expense's or payment's own, to an entry's own
field, and to a rate's.

**Participant ids** MUST be unique within a document (`duplicate_participant`).

**`identityKey`** on a participant is the Ed25519 public key that alone may
write as them (§10.7). Optional; absent means the identity is unclaimed.

**`payouts`** on a participant is how they want to be paid, most preferred
first: a list of `{"type": "zec", "address": …}`, `{"type": "swap", "asset": …,
"chain": …, "address": …}` or `{"type": "cash"}`. Optional; empty means none is
declared and `payTo` stands in.

A reader MUST refuse a `type` it does not define
(`bill_unknown_payout_method`) rather than skip it. **Skipping settles to the
next preference down, which is a different address.** Order is the preference
order and MUST be preserved.

A preference takes no part in any arithmetic; §6 decides who owes what.
Honouring one — building the transaction, performing a swap, telling a person
to hand over cash — is the wallet's.

**A participant id MUST be a non-empty string** (`bill_bad_participant_id`).
An empty id is not a name anyone can be settled to: §8.5 would render its
`payTo` into a payment request like any other, and two readers disagreeing
about whether to admit it fold different bills from one log. §8.3 already
refuses an empty address for the same reason.

**`confirmedPayments`** is the ids of the payments §10.5 has confirmed. It is
optional on read, and **a document that omits it has confirmed nothing** — not
everything. A reader that treats the absent key as "every payment stands"
settles a debt on the debtor's own unconfirmed claim, which §5.1 forbids in
bold. The fold (§10.3) is what computes it; a decoder carries it through
unchanged and never substitutes a default of its own.

**Optional fields:** `payTo`, `rate`, `note`, `reference`, `source`,
`confirmedPayments`. `payments`
is optional on read; a bill without it has no settlement history, not a
malformed one.

**`splitMode`** is `equal` or `percentage`, the default a UI reaches for. It
constrains nothing. An absent value reads as `equal`; an unrecognised one is
refused with `bill_unknown_split_mode`.

**Every payload carrying an amount states its own `currency`.** An expense or
payment therefore decodes on its own, which is what lets §10.3 transmit a
subset of a log that does not include the entry that opened the bill. A reader
MUST fall back to the enclosing bill's currency when the field is **absent**,
and a present value that is not a currency is **not** absent: it is refused
(`bill_bad_currency`), and §10.3 sets the entry carrying it aside. Reading the
two the same way lets one wrong-typed member redenominate an amount silently.

**An optional member that is a list reads `null` as absent**, because both
denote none and the fallback is the empty list rather than a value standing in
for something. **An optional member that is a scalar does not**: there `null`
is refused like any other wrong type, because the fallback would stand in for
a value nobody stated.

**An optional member that is an object is refused when it is not one**, with
one exception stated where it lives: §11.2's `invite`, which a reader ignores
rather than refuses, because it is carried for the caller and takes no part in
what the payload means.

### 9.2 Payments

**`method`** is `shieldedZec`, `swap` or `cash`, and anything else is refused
with `bill_unknown_settlement_method`. It is a label, not a branch:
the ledger arithmetic is identical whichever happened.

`cash` is unverifiable by construction — anyone on the bill can claim it — and
a UI SHOULD NOT present it with the confidence of an on-chain payment.

**`from` and `to` MUST name different participants** (`self_payment`). A
payment to oneself moves no balance and is either a mistake or an attempt to
pad a settlement history; either way it is not a payment this protocol
records.

A payment MAY carry **`zatoshi`** and **`paidAtRate`**, recording what it sent
and the rate it was converted at. **Both are advisory:** the fiat `amount` is
what settles the debt, and neither takes any part in §5 or §6.

`zatoshi` MUST be an integer (`bill_type_error`) and greater than zero
(`negative_amount`). `paidAtRate` MUST be an object (`bill_type_error`) and MUST
price the same currency the payment is in (`rate_currency_mismatch`): a rate in
another currency restates the debt at an unrelated number rather than pricing
it, and both halves can be individually valid while the product is wrong by a
hundredfold.

**A rate is checked against the currency the payment states, never one it
inherited.** A rate states its own currency, so it is never inherited; a reader
that checked it against the fallback would let a `createBill` entry the
authority goes on to refuse make an honest payment undecodable. Where the
amount inherited its currency, the fold checks the rate against the bill's
instead, and sets the entry aside with `rate_currency_mismatch` rather than
denominating the amount around a rate that prices something else.

**A `swap` payment is verifiable only in half.** What leaves the payer's wallet
is ZEC and is recorded in `zatoshi`; what the recipient was owed arrives as
another asset on another chain, which this bill cannot see.

`reference` identifies the swap — the provider's intent id, or the transaction
on the destination chain — and the chain it names is the `chain` of the `swap`
payout being settled, which the payment does not repeat. A reader that shows a
reference as a Zcash transaction because it looks like a txid is wrong for
every swap.

A UI **MUST NOT** present a `swap` payment as confirmed on the strength of the
ZEC leg alone: that the deposit was sent is not that the recipient was paid,
and only the recipient can say the latter (§10.5).

Where a participant's payout may change after a payment is recorded, the chain
of a historical `reference` stops being derivable from the current payout. An
implementation that allows this SHOULD record the chain in `note`.

### 9.3 Canonical form

**Instants.** Every instant is written in canonical form: RFC 3339, UTC,
exactly three fractional digits, `Z` suffix — `2026-10-28T19:30:00.000Z`.
Canonical instants are fixed width, so their lexicographic order is their
chronological order, and an implementation can sort a log without a calendar.

The grammar a reader accepts is exactly:

```
YYYY "-" MM "-" DD ("T" / "t") HH ":" MM ":" SS [ "." 1*DIGIT ] ("Z" / "z")
```

with `YYYY` from `0001` to `9999`, `MM` from `01` to `12`, `DD` no greater than
that month's length in the Gregorian calendar, `HH` at most `23`, and `MM` and
`SS` at most `59`. Fractional digits beyond the third are **truncated**, not
rounded.

Everything else is refused with `bill_type_error`: a numeric offset, a space
separator, a leap second, a day the month does not have, a year outside the
range, and a bare date.

`DIGIT` is `%x30-39` (RFC 5234), and nothing else. A regular expression whose
`\d` matches every Unicode decimal digit accepts `2026-١٠-28T19:30:00.000Z`,
and the "canonical" instant it then emits is not ASCII: it sorts after every
ASCII instant on the bill, so one entry can be forced last in the §10.2 order.
The property §9.3 rests on — that lexicographic order is chronological order —
holds only over this alphabet.

**A reader MUST accept exactly the grammar above and no more.** A looser rule
lets two readers normalise the same instant differently and give one entry two
different sort keys.

**JSON.** A document whose bytes are compared, hashed or signed MUST be encoded
canonically: object keys in ascending order (§2.3), no insignificant
whitespace. The `splitz1:` and `splitzd1:` payloads of §11.2 are encoded this
way. Elsewhere key order carries no meaning and a reader MUST NOT depend on it.

**A string is written as RFC 8785 §3.2.2.2 writes it.** `"` and `\` are escaped
as `\"` and `\\`; U+0008, U+0009, U+000A, U+000C and U+000D as `\b`, `\t`,
`\n`, `\f` and `\r`; every other code point below U+0020 as `\u` and four
lower-case hexadecimal digits. Nothing else is escaped: `/`, `&`, `<`, `>`,
U+007F, U+2028, U+2029 and every non-ASCII character are written as
themselves, in UTF-8. Every id is a digest of this encoding (§9.4, §9.5), so an
encoder that escapes one character more — `/` as `\/`, or `&` as `\u0026` —
derives a different id for the same entry, and every conforming reader
refuses what it writes.

**A float is not an amount.** Canonical encoding MUST refuse a floating point
number anywhere in a document with `canonical_json_float`, rather than
truncating it or aborting: §2 puts every amount in minor units as an integer,
and a document carrying a float was not built by a conforming writer.

### 9.4 The bill id

A bill's id is the id of the `createBill` entry that opens it, and it is printed
in every invite (§11.1).

One id names one entry (§10.2), so an id that anyone who has seen the invite
may claim is an id anyone may take. A second `createBill` carrying it occupies
the slot, the genuine entry is discarded by the merge, and the bill then names
itself whatever the second entry says, in whatever currency it says. Every
expense denominated in the original currency stops applying.

**So the id is not chosen. It is the digest of the entry that opens the bill:**

```
id = base64url( SHA-256( "splitz-bill-id-v1" || canonical(E) )[0..16] )
```

where `E` is the `createBill` entry's JSON object (§9) **with `id`, `sig` and
`v` removed**, encoded canonically (§9.3), and `canonical(E)` is its UTF-8
bytes. `base64url` is unpadded, as everywhere in this protocol.

**Every base64url value this protocol decodes MUST be the canonical encoding of
its bytes**: the bits its last character carries beyond the last whole byte
are zero, so encoding the decoded bytes again reproduces it exactly. One that
is not is refused as that value is refused when it does not decode. RFC 4648
§3.5 leaves this to the decoder, and two decoders choosing differently open a
bill from one scanned code on one device and refuse it on the next.

`id` is excluded because it is the output. `sig` because a signature covers the
id in turn. `v` because an entry does not carry its own format version through
an implementation's object model, so re-encoding an older entry would restate
the version the reader writes and break a binding that was correct when made.

A `createBill` entry MUST carry:

- **`creatorKey`** — the creator's Ed25519 public key, 32 bytes, unpadded
  base64url.
- **`nonce`** — 16 bytes, unpadded base64url, drawn from a cryptographic random
  source.

Without the nonce one creator could not open two bills agreeing in name,
currency, instant and split mode: they would derive one id and merge into one
bill.

Either field absent, not unpadded base64url, or the wrong length is
`create_unbound`. An entry carrying both but whose `id` is not the derivation
above is `create_id_not_derived`.

### 9.5 Every other entry's id

**An entry's id is the digest of the entry, for every kind.**

```
id = base64url( SHA-256( "splitz-entry-id-v1" || canonical(E) )[0..16] )
```

`E` is the entry with `id`, `sig` and `v` removed, exactly as in §9.4, and for
the same reasons. The domain separator differs so that a `createBill` id and
any other entry's id are drawn from different spaces and neither can be
presented as the other. An entry whose `id` is not this derivation is refused
with `entry_id_not_derived`.

§9.4 gives the reason for `createBill`: an id anyone may choose is an id anyone
may take. That reason is not special to `createBill`. §10.2 resolves two
entries sharing an id by keeping the one whose canonical encoding sorts higher,
and `author` is a free string sorting between `at` and the payload, so a peer
could append one high byte to it and beat any entry at will. Re-pushing a copy
of somebody's expense with the amount changed then displaced the genuine entry
on every device, with nothing refused and nothing set aside — the bill simply
said something else.

Deriving the id closes that by construction rather than by a rule. Two entries
share an id only if they agree in every member the digest covers, so §10.2
rule 2 no longer decides what an entry says; it decides only between encodings
of one entry. Changing any member yields a different id, which is a **new**
entry the fold applies on its own terms and §10.4 and §10.8 can withdraw,
rather than a silent substitution for an existing one.

A host that verifies signatures gains from this too: the genuine signed copy
can no longer be destroyed by the merge before the signature is examined.

**A reader MUST refuse such an entry before it enters the log** — before the
merge of §10.2, not during the fold of §10.3. The merge keys entries by id and
keeps one per id, so an entry admitted to the map has already displaced
whatever else claimed that id; refusing at fold time is refusing something that
has already won. A refused entry is reported to the caller and does not
otherwise affect the log.

**What this gives, and what it does not.** Producing a second entry with a
given id requires a second preimage of a 128-bit digest, so no holder of the
invite can claim a bill's id, whatever key they name. This holds with no
signature anywhere.

Naming *someone else's* key in an entry of one's own is possible and harmless:
it derives a different id, so it is a different bill.

Proving the creator holds the key the id names requires verifying `sig` against
`creatorKey`, which §10.1 leaves to the host.

The `id` of the flat bill document of §9 is not bound. That document is a
snapshot, not a merge target, and nothing in it occupies a slot another entry
could take.

## 10. The log

A bill is materialised from an append-only log. **Entries are never modified.**
Correcting an expense appends an `amendEntry` naming the one it replaces;
removing it appends a `voidEntry`. There is no state to conflict, only facts to
union.

### 10.1 Entries

`createBill`, `joinBill`, `addExpense`, `amendEntry`, `voidEntry`,
`recordPayment`, `confirmPayment`, `setRate`.

Each carries `id`, `author`, `kind`, `at`, and only the payload its kind uses.
An unrecognised kind is refused with `bill_unknown_entry_kind`.

**An entry MUST carry the payload its kind uses and no other.** One carrying
more than one of `expense`, `payment` and `confirmation` is refused
with `bill_ambiguous_entry`, because the currency fallback of §9.1 and the fold
of §10.3 would otherwise read different ones.

One carrying none is refused with `bill_missing_entry_payload`, and MUST be
refused before it reaches a log: `joinBill` needs `participant`, `addExpense`
`expense`, `recordPayment` `payment`, `confirmPayment` `confirmation`,
and `voidEntry` and `amendEntry` a `targetId`.

**Admitting one is not the harmless no-op it appears to be.** Removing a member
makes an entry's canonical encoding sort *higher* than the same entry with it:
at the removed key's position the shorter form holds the next key, or `}`, both
of which are greater. §10.2 therefore keeps the stripped copy. Anyone holding
the invite can re-push a copy of an entry with one member removed and take the
expense, payment or withdrawal it carried off the bill, on every device, with
nothing set aside to show for it.

**An entry's `v`, when present, MUST be an integer of at least 1**, refused
with `bill_type_error`. §9.5 leaves `v` out of the id, so a copy carrying any
value keeps the honest entry's id, and one whose `v` is not an integer would
reach the canonical encoding that §10.2's merge and order compare — which
refuses it — and take every batch it travels in down with it.

**The payload MUST be an object, and every id inside it MUST be a string**,
refused with `bill_type_error`. §10.1 is the only gate between a peer's JSON
and every pass that follows, and those passes index the payload without
re-checking it: a scalar `participant`, `payment` or `confirmation` admitted
here reaches the fold's authorisation pass, which reads the target's payload to
decide who may withdraw the entry. That pass fails before the withdrawal is
considered, so the entry cannot be taken off the bill by anybody — not its
author, not the creator — and the bill is unopenable on every device that
holds it.

This is why the check belongs at ingress rather than at the point of use. A
later pass that refuses cleanly still leaves §10.3's "set aside and report"
unsatisfiable, because the entry is already in the union on every device.

**An id list MUST hold only strings.** `among`, `sharedBy` and the keys of
`amounts`, `basisPoints` and `shareCounts` name participants, and a reader that
drops a member it cannot read reassigns that participant's share to the others:
three people splitting 9000 become two paying 4500 each, with nothing refused
and nothing set aside.

**Every order this specification defines MUST be total.** Where a comparison
leaves two rows equal, the tie is broken by their canonical encoding (§9.3),
which distinguishes any two rows that differ at all. A comparator returning
"equal" for rows that are not equal leaves the order to the host's sort, and a
sort that is stable in one language and not in another then produces different
output from one input — including, where the rows are entries, a different bill
document and so a different digest.

**`setRate`** carries a `rate` (§7). The rate is part of the bill's shared
state, so it has to reach the bill the way everything else does. Without an
entry that carries one, every device folding the same log must find a rate of
its own, which is the outcome §7 exists to prevent: two devices then price one
settlement differently and the bill does not close.

The **latest** live `setRate` decides, by §10.2's order — `at`, then `author`,
then `id`, then canonical encoding — so the answer is a function of the log and
not of which device last spoke. A `setRate` may be amended and withdrawn like
any other entry; when none survives, the bill has no rate and §7's conversions
are unavailable rather than guessed at.

**A `setRate` MUST be authored by a participant on the bill**, and one that is
not is set aside with `unknown_participant`. The rate decides how much ZEC
every request carries, so a rate written by somebody who owes nothing and is
owed nothing turns every payer's request into whatever figure they chose.
Participants may still set it, and a wallet MUST show a payer the rate a
request was priced at and who set it (§14.2).

**`sig`** carries the author's signature when the transport provides one.
When present it MUST be a string, and an entry whose `sig` is anything else is
refused with `bill_type_error`. Unsigned entries MUST be accepted: a bill among people at one table is consensus
by agreement, not by cryptography, and refusing them would claim a guarantee
this protocol does not make. Nothing in this version verifies a signature;
§10.4 says what that costs.

A host that does verify signatures MUST do so over the bytes §10.6 fixes, and
MUST verify a `createBill` entry's `sig` against the `creatorKey` in that same
entry, refusing the entry when it does not verify. That key is not taken on
trust from elsewhere: §9.4 binds it to the id, so it is the one key on a bill
that needs no prior acquaintance to check. Verifying any other entry needs a
key bound by §10.7, which this version does not provide.

A `createBill` entry additionally carries `creatorKey` and `nonce`, and its `id`
MUST be their derivation (§9.4).

**An entry naming a target the log does not hold is set aside with
`unknown_entry`.** This applies to `amendEntry` and `voidEntry` alike: a
correction or a withdrawal pointing at nothing corrects and withdraws nothing,
and a reader that ignores it silently differs from one that reports it.

### 10.2 Merge and order

Merging is **set union keyed by entry id**. Two entries with the same id are
meant to be the same entry, and nothing in an unauthenticated log guarantees
it, so union needs a rule for the case where they differ:

1. **A copy carrying a `sig` beats one that does not.**
2. **Two copies carrying different `sig` values are both kept.**
3. **Otherwise the entry whose canonical encoding (§9.3) sorts higher under
   §2.3 wins.**

The first part exists because a signed and an unsigned copy of one entry is the
pair a transport produces constantly: an author's own store holds the copy they
signed, and a delta may carry it stripped.

Canonical order alone resolves that pair the wrong way. The signed form is the
unsigned one with a `"sig"` member added, and the comma introducing it sorts
below the closing brace, so the unsigned copy always wins — and §10.7 then sets
it aside, because an entry from a bound author carrying no signature does not
verify. The merge would destroy the only usable copy of an entry both devices
hold.

**Why two signed copies are both kept.** §9.5's digest does not cover `sig`,
so a copy of a signed entry with its signature replaced keeps the entry's id.
Ed25519 is deterministic (RFC 8032 §5.1.6): an author signing one entry
produces one signature, so two different signatures under one id mean at least
one is not the author's. Nothing in the two entries says which. Resolving the
pair by canonical order hands the id to whichever signature sorts higher —
anyone holding the invite can then replace the bill's `createBill` with a copy
no verifier accepts, and every device that verifies stops opening the bill,
the genuine copy losing every later merge. Keeping both leaves the question
for the fold, which has the key (§10.3).

Copies differ only where §9.5's digest does not reach, so every kept copy of
an id carries the same payload. Keeping them costs storage and nothing else:
anyone holding the invite can already append entries without limit.

**The merge enforces §10.1 before the rule above is reached.** An entry that
does not carry the payload its kind uses is refused at ingress and never
enters the union.

This is not tidiness. Removing a payload member makes an entry's canonical
encoding sort **higher** than the same entry with it: at the removed key's
position the shorter form holds the next key or `}`, and both are greater than
`,`. Rule 2 therefore prefers a stripped copy over the genuine entry every
time. Anyone holding the invite could re-push a copy of an entry with its
payload removed and take the expense, payment or withdrawal it carried off the
bill, on every device, with nothing set aside to show for it. Signing does not
stop it: a stripped copy carrying any signature ties rule 1 and wins rule 2,
and setting the forgery aside afterwards does not restore what it displaced.

A reader MUST apply §10.1 to every entry before merging it, and MUST report
what it refused.

A `createBill` entry that fails §9.4 never reaches this rule: it is refused at
ingress and is not part of the set being merged. The rule decides between two
entries that both belong under the id, never between a genuine entry and one
claiming its slot.

**Both parts are functions of the two entries alone, which is what makes union
commutative.** Without that, an implementation resolves a collision by arrival
— keeping the last write, or the first — and the merged bill depends on the
order two devices happened to sync in. Two implementations picking opposite
conventions produce different bills from the same pair of logs, and neither is
detectably wrong.

Idempotence and associativity do not depend on the rule: union keyed by entry
id and signature, with any deterministic resolution among copies that share
both, has both. Only commutativity
distinguishes the rule above from resolving by arrival, and it is the only one
of the three a conformance case can falsify.

**The total order is `at`, then `author`, then `id`, all ascending.** It never
depends on arrival order or on local state.

### 10.3 Folding

1. Sort by §10.2.

2. The `createBill` entry whose id the caller holds the log under names the bill,
   its currency and its default split mode.

   A caller that names no id gets the only `createBill` in the log. **A log
   holding more than one is refused with `ambiguous_create`**: anyone holding
   the invite can push in a create entry of their own, which §9.4 admits
   because it is valid for a *different* bill, and guessing between them lets a
   backdated one rename this one.

   A log with no entries is refused with `log_empty`. A log with entries but no
   `createBill` is refused with `log_no_create`, which is also what a log whose
   only create entry was refused at ingress folds to.

3. Collect amendments per target — later ones win under the total order — and
   the set of voided targets.

4. Apply every non-voided `joinBill`, replaced by its amendment if any, subject
   to §10.4. **A participant who rejoins replaces their earlier record** rather
   than being refused as a duplicate, and a replaced destination — the `payTo`,
   or the `address` of the first payout when the record declares any — MUST be
   recorded and reported to the caller.

5. Apply every non-voided `addExpense` and `recordPayment`, replaced by their
   amendments if any.

6. Apply every non-voided `confirmPayment` (§10.5), in a pass of its own once
   every payment is on the bill.

**The fold reaches four answers beyond the bill itself, and an implementation
MUST report them rather than discard them.** A consumer that re-derives one has
a second place for it to come from:

- the **creator's id**, which the withdrawal rules of §10.8 name;
- the **identities bound and contested** under §10.7, which say why a payout is
  missing rather than leaving it looking undeclared;
- the ids of the **entries a void withdrew**, because a withdrawal is absent
  from the fold by design and is otherwise indistinguishable from an entry that
  was never written;
- the **entries set aside**, each with its code and the reason.

**The fold takes the host's verifier, and the fourth answer is empty without
one.** §13 makes the curve operation the host's, so a fold given no verifier
cannot decide a contest and reports none — which is the honest answer, not a
claim that none exists. A caller that hands one in gets `bound` and `contested`
computed over the same entry set the bill was materialised from, which is what
§10.7's rule depends on: a wallet MUST NOT settle to a contested participant's
address without putting it in front of the payer first, and it can only obey
that if the fold tells it.

**A verifier also decides which entries are applied at all.** §10.1 requires a
host that verifies to check a `createBill` entry's `sig` against the
`creatorKey` in that same entry. When a verifier is supplied, a `createBill`
whose signature does not verify is set aside with `unauthorized_entry` and
opens no bill. Unsigned entries are still accepted — §10.1 says so and says why
— and a fold with no verifier behaves exactly as one that is given a verifier
accepting everything.

**An amount that states no currency is denominated by the fold, not by the
reader.** A reader MUST record which of the two an amount did — stated its own
currency, or took the document's — because the currency a reader falls back to
is chosen before any signature is checked, and can therefore come from a
`createBill` entry the authority goes on to refuse.

The fold restates an inherited amount in the bill's currency. An amount that
**stated** a currency other than the bill's is set aside with
`currency_mismatch` and MUST NOT be restamped, which would keep the count and
change the unit.

Without the distinction, anyone holding the invite can push in a create entry
of their own naming another currency and take every amount that stated none off
the bill.

**One id names one entry, in the fold as in the merge.** A log holding two
copies of an id is reduced to one before anything is applied: a re-sent entry
would otherwise be applied twice, and one expense sent twice doubles what
everybody owes.

- A fold given a verifier resolves identities (§10.7) over every copy first.
  An entry whose author has a key — the creator, through `creatorKey`, or a
  participant bound under §10.7 — is applied from a copy whose `sig` verifies
  against that key. When no copy does, the entry is set aside with
  `unauthorized_entry`. Among copies that verify, and for an author with no
  key, the copy whose canonical encoding sorts higher is applied.
- A fold given no verifier applies the copy whose canonical encoding sorts
  higher.

§9.5 makes every copy agree in every member the digest covers, so which copy
applies changes nothing but whether the entry applies at all.

**An entry that cannot be applied MUST be set aside and reported, not raised as
a failure of the whole fold.** The log is append-only and merges by union, so a
single malformed entry propagates to every device. Aborting on it leaves the
bill permanently unopenable — including unopenable to append the void that would
remove it. A reader reports what it set aside and shows the rest.

**This binds every member the reader's own decoder requires, not only the ones
§10.1 types at ingress.** A document the fold returns and the decoder then
refuses is the same failure wearing a different code: the bill is unopenable
and nothing names the entry that did it. So a reader decides each participant,
expense, payment and rate by the same rules §9 decodes them under — and each
expense's split by §4, which is what turns it into what anyone owes — at the
point it applies the entry, and sets aside the one that fails. A reader that
leaves any of them to a later pass has moved the failure, not removed it: the
bill is built and then will not open, or opens and will not balance, and
`setAside` names nothing either way.

**The refusal report is per occurrence, and is not part of the convergent
state.** §10.2's union converges — the bill, the balances and what was
withdrawn are the same on every device that has seen the same entries. The
list of refusals is not: a device that received one malformed entry twice
reports it twice, and a device that merged before folding reports it from the
merge rather than from the fold. Both have reported what they refused, which
is all this section asks. A reader MUST NOT treat two devices' refusal lists
differing as evidence that their bills differ.

### 10.4 Authorisation

**An entry speaks for its author.**

- **Any author may create a participant record.** Someone who is not at the
  table still has to be split with.
- **Only the participant themselves may change a record that already exists.**
  An entry whose `author` differs from the `id` of the participant it modifies
  is set aside with `unauthorized_entry`.
- An `amendEntry` MUST be authored by the author of the entry it targets, and
  is otherwise set aside with `unauthorized_entry`. A `voidEntry` depends on
  what it targets: see §10.8.
- An `amendEntry` MUST carry a payload of the same kind as its target, and is
  otherwise set aside with `amend_kind_mismatch`. An amendment replaces its
  target wholesale, so one carrying no payload silently deletes what it claims
  to correct.
- An `amendEntry` MUST keep the id its target is about — a join's
  `participant.id`, an expense's `id`, a payment's `id`, a confirmation's
  `paymentId` — and is otherwise set aside with `amend_kind_mismatch`. A
  correction that renames its subject is a different entry: a join renamed
  out from under an expense takes its author off the bill and the debt with
  them, and the §10.8 checks that read the target never see it.

**A refund is an expense with a negative total**, and §3 step 7 negates every
share to divide one. It carries **no special authorship**: anybody holding the
invite may write a refund exactly as they may write an expense, and the second
already lets them put a cost on the bill nobody agreed to. The remedy is the
same — §10.8 lets a refund's author or the bill's creator take it off, and the
withdrawal is visible in the log either way.

A host that wants more than this needs signatures (§10.7) and a policy of its
own about who may spend on a bill. This protocol does not have one, because the
invite is the trust boundary and it is handed out at a table.

**This is not authentication on its own.** `author` is an unauthenticated
string, so a participant holding the invite can set it to anyone's id and these
rules pass. §9.4 is the one place the protocol binds a key: it fixes which
entry opens the bill, and so which name, currency and default split the bill
has. It says nothing about who wrote any other entry.

What these rules remove is **the silent case**. Without them, appending one
`joinBill` naming another participant's id and carrying your own address
redirects every later settlement to that person, while the payment request
still shows their name, and nothing in the bill records that anything changed.
With them, the substitution requires forging authorship — which is what `sig`
exists to make detectable — and any address that does change is reported to the
caller.

**A wallet MUST put a changed `payTo` in front of the payer before settling to
it.**

Participants are applied in a pass of their own, before anything that
references them. An expense and the join of the person who paid it can carry
the same instant, and the total order then falls back to author and id, which
is not an order the bill can be built in. Two passes make the fold independent
of that.

### 10.5 Confirming a payment

A `recordPayment` entry is **a claim** that a debt was discharged. Who made the
claim matters: the payer saying they paid and the recipient saying they were
paid are different facts, and cash has no other evidence. A `confirmPayment`
entry carries the second kind.

```json
{
  "v": 2, "id": "c1", "author": "ana", "kind": "confirmPayment",
  "at": "2026-10-28T19:34:00.000Z",
  "confirmation": {
    "paymentId": "p1", "method": "recipientConfirmed",
    "reference": "…", "note": "counted it"
  }
}
```

`paymentId` is the `id` of the **`PaymentRecord`**, not of the entry that
recorded it; `targetId` names entries and is not used here.

**A `PaymentRecord` id MUST be unique within a bill** (`duplicate_payment`). A
confirmation names one record, and a method that speaks for the payment's `to`
is checked against that record's `to`, so an id held by two records names a
payee ambiguously: one recipient's confirmation would settle a debt another
recipient never vouched for. One transaction paying several people is several
records, and they MUST NOT share an id — the transaction goes in `reference`,
which is what `onChain` reads and what ties the records to the chain. A second
record carrying an id the bill already holds is set aside; the first stands. `by` is the entry's
`author` and the instant is the entry's `at`; neither is restated in the
payload. `reference` and `note` are optional.

A confirmation carries `record`: the digest of the payment payload of the
record it confirms, `base64url( SHA-256( "splitz-payment-v1" || canonical(P) )[0..16] )`,
where `P` is the record's `payment` member as written, less its `id` — the
same shape as §9.5's entry id, under its own domain. **A confirmation applies only while the bill's record
under `paymentId` has that digest**, and is otherwise set aside with
`unknown_payment`. A record withdrawn and written again under the same id, or
amended since, is a payment nobody confirmed — without this, a payer records a
cent, has it confirmed, and then rewrites the record to the whole debt. A
reader reports each record's digest beside the bill it folds, so a wallet can
write the confirmation.

`method` is one of four, and each speaks for a particular participant:

| `method` | may be authored by | needs a `reference` | settles the debt |
|---|---|---|---|
| `recipientConfirmed` | the payment's `to` | no | yes |
| `walletReceived` | the payment's `to` | no | yes |
| `onChain` | the payment's `to` | **yes** | yes |
| `payerAttested` | the payment's `from` | no | **no** |

**Every method that settles anything is the recipient's.** The payer's own word
never does, however it is phrased — cash handed over, a transaction id, a
photograph. A payer is the one party to a payment with a reason to claim one
that did not happen, and theirs is the only claim on a bill that costs its
author nothing to get wrong.

`onChain` is the recipient saying they opened the transaction and saw it land.
**It is not "anyone may check".** A third party reading a transparent
transaction learns that one happened, not that it was this debt, and a
**shielded payment is invisible to everyone but the person paid** — which is
most of what this protocol settles.

A confirmation of a payment on a chain MUST carry the `reference` it was
checked against (`confirmation_missing_reference`): one saying a payment is on
a chain without saying where contains no chain.

An unrecognised method is refused with `bill_unknown_confirmation_method`.

**The author MUST be the participant the method speaks for**, and an entry
whose author is not is set aside with `unauthorized_confirmation`. A
confirmation's whole weight is in who gave it, so a method anyone may claim is
a method that says nothing: without this rule one holder of the invite marks
their own debt `recipientConfirmed`, or `onChain` with any string as its
reference, and every device shows it settled.

**`payerAttested` is never conclusive.** A payer saying they paid is the claim
of the `recordPayment` entry, not evidence for it; attaching a reference or a
photo corroborates it and does not settle it. It is recorded and reported all
the same, because a reader shows it.

A payment's **status** is `confirmed` when any confirmation that settles a debt
stands for it, and `pending` otherwise.

**A payment discharges a debt when it is confirmed, and not before** (§5.1).
Recording one is a claim, and a claim by the person who owes the money moves
nothing. Without this, tapping "I paid Ana" clears the debt whether or not a
cent moved, and the only recourse open to the person owed is to notice the
claim and withdraw it.

**A wallet MUST show a payer that a payment of theirs is awaiting confirmation
rather than asking them to pay it again.** Settlement reads balances (§6), so a
pending payment is not deducted and the same debt appears in the next plan. A
wallet that does not distinguish the two asks people to pay twice, which is
worse than the problem this rule solves.

A confirmation is set aside and reported when it names a payment the bill does
not hold (`unknown_payment`) or an author who is not on the bill
(`unknown_participant`).

**Confirmations are applied in a pass of their own, after every payment is on
the bill**, for the reason §10.4 gives about participants. A confirmation may be
written, and may arrive, before the payment it vouches for; the total order is
by instant, so a single pass would set aside one that is merely early.

A confirmation may be withdrawn by **its own author**, with a `voidEntry`
naming its entry id, under §10.4's rule. Anyone else's void of it is set aside
with `unauthorized_entry`. Someone who vouched in error must be able to take it
back, and nobody else may take it back for them.

### 10.6 What a signature covers

`sig` is optional (§10.1) and this version verifies nothing. But an
implementation that does verify, and a wallet that signs so another wallet can,
need the same answer to one question: **which bytes?**

Left unstated, each signs whatever its own encoder emitted and no two agree — a
divergence neither can detect alone, because each verifies its own signatures
perfectly.

The message is:

```
"splitz-entry-v2" || canonical({"bill": B, "entry": E})
```

where `B` is the id of the bill the entry is written for (§9.4) and `E` is the
entry's JSON object (§9) **with `sig` and `v` removed**, the object encoded
canonically (§9.3), and the whole is UTF-8. A `createBill` entry's bill is its
own id.

**The bill is part of the message because an entry does not name it.** A
participant's id and key are the same on every bill they are on, so a
signature over the entry alone verifies on any bill: a confirmation Ana gave
on one bill, carried into another, would settle a debt there that nobody paid.
A verifier checks against the bill it is folding, never one read from the
entry. The signature is Ed25519
(RFC 8032) over that message. Both the 32-byte public key and the 64-byte
signature are unpadded base64url.

**Canonical, not the encoder's own order.** This is the one place a document's
bytes are compared rather than its fields, so key order stops being
presentational and becomes part of the message. Dart's maps iterate in
insertion order and `serde_json`'s in sorted order; an implementation signing
its own encoder's output signs a different message from one that sorts, and the
two can never check each other.

**Why the two exclusions.** `sig` is the output, and a message containing it
could not be produced before it existed. `v` is excluded because an entry does
not carry its own format version through an implementation's object model: a
reader re-encodes with the version *it* writes, so a signature covering `v`
would stop verifying for every existing entry the day the version changed.

Everything else is covered. `id` is included, so a signed entry cannot be
restamped under another id. `creatorKey` and `nonce` are included, so the §9.4
binding cannot be edited under a signature made for different content.

Producing and checking the curve operation is the host's (§13). What this
section fixes is the message, which is the part two wallets must agree on.

**Vectors state the message as text rather than as a verdict.** A case
asserting that a signature verified would pass in two implementations that
disagree about the bytes, each checking its own.

### 10.7 Who a participant is

§10.4 restricts what an entry may do by the id in its `author` field, and says
plainly that the field is unauthenticated. This section says what a host that
verifies signatures (§10.6) does instead.

**Its answer MUST be a function of the entry set alone** — not of arrival
order, not of what a device has seen before, not of anything on its disk. A
device resolving this differently from its neighbour folds a different bill
from the same entries, which is the one outcome this protocol exists to
prevent.

A participant publishes their key in their own join:

```json
{"id": "ben", "name": "Ben", "payTo": "u1…", "identityKey": "<32 bytes, base64url>"}
```

**Binding.** A key is bound to a participant by a **self-claim**: a `joinBill`
whose `author` equals the `id` of the participant it carries, whose participant
states an `identityKey`, and whose `sig` verifies against that key. An entry
naming somebody else's id proves nothing about them, whoever signed it.

**The creator is bound by the invite, not by a join.** The bill's id is the
digest of the entry that opened it (§9.4), and that entry states `creatorKey`.
A reader MUST bind the creator's participant id to that key when the entry's
own `sig` verifies against it, MUST NOT treat a join claiming the creator's id
as a rival claim, and MUST refuse such a join under the rule below.

Without this, the one identity a bill can prove would be contestable by anyone
who photographed the invite. The signature requirement is not ornamental:
absent it, `creatorKey` is a number the bill's author typed, and anyone could
open a bill naming somebody else's public key and be taken for them on it.

**Contest.** Where two different keys each carry a validly signed self-claim
for one id, neither is bound and the id is **contested**.

Nothing internal to the log says which is the person: `at` is whatever its
author wrote, so resolving by time hands the identity to whoever backdates
furthest. A reader MUST NOT break the tie with anything outside the log. A host
MAY remember keys it has seen and use that memory to **warn** a person; it MUST
NOT use it to change which entries apply.

**A withdrawal does not undo a claim.** Identity is resolved over every entry
in the set, including ones §10.8 withdrew. A signed self-claim is evidence that
was made; taking the entry off the bill does not unmake it.

This is not a convenience. §10.8 lets a `joinBill` be withdrawn by "that
participant", and a rival claim names the same participant id as the genuine
one, so **either author qualifies to withdraw either entry**. Were a withdrawal
to clear a claim, an impostor who minted a rival claim could then withdraw the
genuine one, leave only their own standing, and bind their key to that
participant — silently replacing the person money is sent to. A contest
therefore stands until it is settled between the people involved, which is what
this section says it costs.

A reader MUST resolve identity over the whole entry set and MUST NOT resolve it
over the live set §10.3 folds.

**What follows from a binding.** For a participant whose key is bound:

- A `joinBill` carrying their id — creating their record or changing it —
  MUST be authored by them and MUST verify against their key, or it is set
  aside with `unauthorized_entry`. This is what stops a relay blob redirecting
  a payout, including to a participant who never joined by an entry of their
  own, such as a creator.
- Any entry authored as them MUST verify against their key, or it is set aside
  with `unauthorized_entry`. §10.3 applies this to every copy of the entry,
  so an unsigned copy, or one signed with any other key, never speaks for
  them.

**An unbound or contested identity is admitted unverified**, exactly as if this
section did not exist. Refusing its entries would let anyone make a bill
unopenable by minting a rival claim for its creator: every entry that creator
wrote would stop applying, the `createBill` included.

What a contest costs instead is **the ability to be paid**. A wallet MUST NOT
settle to a contested participant's address without putting it in front of the
payer first (§10.4).

**What this does not give.** A participant who joins after the bill was made
has no key in the invite, so nothing settles a rival claim against them: they
can be contested, and a contest is a denial of payment until the people
involved sort it out in person. Only the creator is beyond that.

### 10.8 Who may withdraw an entry

§10.4 gave one rule for both correction and withdrawal: an `amendEntry` or a
`voidEntry` must be authored by the author of the entry it targets. That is
right for correction and wrong for withdrawal, **because voiding is not one
action**. Removing a duplicate expense, retracting a claim that a debt was
paid, and taking somebody off the bill differ in who is harmed when the wrong
person does it, so they differ in who may.

**An `amendEntry` is its author's alone, whatever it targets.** An amendment
restates the amount and who paid, invisibly; anything looser lets a holder of
the invite rewrite any figure on the bill.

**An amendment that has been withdrawn does not apply.** Withdrawals are
resolved before amendments are applied, and an amendment whose own entry is
withdrawn is discarded with it, so the entry it corrected reads as it was
written. An implementation that collects amendments first and withdraws
afterwards leaves a retracted correction standing — the figure a person took
back is the figure the bill shows.

**A `voidEntry` depends on what it targets:**

| target | who may withdraw it |
|---|---|
| `addExpense` | its author, **or the bill's creator** |
| `recordPayment` | its author, **or either participant the payment names** |
| `joinBill` | the creator, **or that participant themselves** — and only under the rule below |
| `createBill`, `confirmPayment`, `amendEntry`, `voidEntry` | its author |

Anything else is set aside with `unauthorized_entry`.

**Why the creator, for an expense.** An expense is entered by hand and
duplicated by accident constantly, and the person who entered it may be asleep.
The creator is the one role every reader can verify without acquaintance: the
bill's id is the digest of the entry that states their key (§9.4, §10.7). A
withdrawal is visible in the log and the expense is re-addable, so the cost of
a wrong one is low — which is not true of the next two.

**Why not the creator, for a payment.** Withdrawing a payment reopens a debt
somebody believed was settled. The person who recorded it may take the claim
back, and the two it names may reject it — one of whom is owed the money and is
the only party harmed by a payment that never happened. Giving that to the
creator as well would let one participant un-settle everybody.

**Withdrawing a withdrawal.** A `voidEntry` may target another `voidEntry`,
and doing so takes the first withdrawal back: the entry it removed is on the
bill again. Somebody who withdraws an expense in error must be able to undo it,
and this is the only mechanism that does.

**A withdrawal is in force unless a withdrawal naming it is itself in force
and authorised.** `at` plays no part. A `voidEntry` names its target by id, and
§9.5 makes an id the digest of an entry's content, so a withdrawal can only
name one that already existed when it was written: what one withdrawal names
orders it after the other, whatever instants their authors wrote. Deciding by
`at` instead lets an author's clock, or a backdated entry, leave an undo
ignored while `withdrawn` reports it applied.

The same digest makes the chains acyclic: an entry cannot name one that names
it, since each id would have to be computed from the other. So the rule has
one answer for any set, resolved from the withdrawals that nothing names
inwards. A chain of three puts the first back in force.

Authorisation is decided first, for every withdrawal, and only an authorised
one counts when deciding what is in force. Resolving over every withdrawal
instead lets somebody who may not withdraw an entry cancel the withdrawal of
somebody who may: the fold reports their entry set aside with
`unauthorized_entry` and restores the entry anyway, refusing the action and
honouring it in the same breath.

An implementation MUST resolve withdrawals this way before applying any of
them. Applying them in one pass leaves a withdrawn withdrawal still in effect,
so the entry it removed never comes back and §10.8's own table is unenforceable.

**Taking somebody off the bill.** A `voidEntry` targeting a `joinBill` MUST be
refused with `participant_still_named` when any surviving entry names that
participant — as an expense's `paidBy` or in its split, as a payment's `from`
or `to`, or as a confirmation's author.

The fold cannot apply an entry naming somebody who is not on the bill, so
without this rule removing the person who spent the most silently drops every
expense they paid for and zeroes the bill. The check runs after every other
withdrawal is resolved, so withdrawing their expenses first and then removing
them is permitted — and is two visible acts rather than one silent one.

**Recording a payment** is subject to the same reasoning and belongs here: a
`recordPayment` entry MUST be authored by the payment's `from` or its `to`, and
is otherwise set aside with `unauthorized_payment`. A payment moves both
parties' balances, so without this rule any holder of the invite could write
"ben paid ana" and clear a debt neither of them had settled.

**Asking before writing.** An implementation SHOULD expose this rule as a
question a caller can ask before appending a `voidEntry`: given an author, the
entry they mean to withdraw and the bill's creator, either the refusal or
nothing.

A withdrawal the fold refuses is otherwise still written, still synced, and
looks to its author exactly like one that worked — the entry is in the log and
the thing it meant to remove is still on the bill.

**What all of this rests on.** These rules name participant **ids**, and an id
is unauthenticated until §10.7 binds a key to it. For a participant who has
published no key, an entry authored as them is admitted unverified, so the
rules above bind the honest and inconvenience nobody else. Only the creator is
bound with no prior acquaintance. A wallet that wants these rules to mean
something must have every participant publish a key on joining.

## 11. Invites

### 11.1 Invite URI

```
splitz://join?v=1&b=<billId>&k=<key>&n=<name>[&x=<expiry>]
```

`k` is the base64url-encoded symmetric key the bill's contents are encrypted
under. **This protocol carries the key; it does not encrypt.**

**The grammar is exact, and a reader MUST NOT delegate it to a general URI
library.** A general parser brings its own answers to questions this format has
to fix itself, and two libraries answer them differently:

- The scheme and host are matched **case-sensitively** against `splitz://join`.
  `SPLITZ://join` and `splitz://JOIN` are not invites.
- Nothing may follow `join` but an optional `?` and a query. A path, a port,
  userinfo or a fragment makes it not an invite; a `#` inside the query is an
  ordinary character of the value it appears in.
- Values are percent-encoded outside the unreserved set plus `!*'()`. On
  reading, **`+` is a literal plus, never a space**: that convention belongs to
  HTML form encoding, and a `+` inside a bill id or a key must survive the round
  trip. A malformed escape is left literal.
- Where a parameter appears more than once, **the first occurrence wins**. The
  alternative lets one code present one version to a reader that scans forwards
  and another to a reader that does not.
- `v` MUST be a bare decimal integer of at least 1 — no sign, no padding, no
  whitespace — and `k` MUST decode as unpadded base64url. ZIP 321 requires `+`,
  `/` and `=` to be refused, and a key that decodes under one alphabet and not
  the other is two wallets deriving two different keys from one code.

**Scan padding.** A code arrives from a camera, a clipboard or a text file and
may carry padding at either end. A reader MUST strip exactly U+0009, U+000A,
U+000D, U+0020 and U+FEFF from both ends before matching the scheme, and MUST
NOT strip anything else.

A general `trim` is not this set, and two standard libraries do not agree on
what it is: Dart's `String.trim()` strips every Unicode `White_Space` character
*and* U+FEFF; Rust's `str::trim()` strips that whitespace and leaves U+FEFF. So
one QR code with a leading byte order mark is an invite to one reader and
`invite_not_an_invite` to the other.

Padding is stripped at the ends only. The same characters anywhere inside the
URI are content, and `splitz://join ?v=1` is not an invite.

**`b`** MUST hold only base64url characters — `A`–`Z`, `a`–`z`, `0`–`9`, `-`
and `_` — and at most 128 of them, or it is refused with `invite_bad_bill_id`.

A bill's id is the digest §9.4 derives, so nothing outside that alphabet came
from a derivation; and a reader hands the id straight on as the name of a file,
a relay channel or a row, with nothing between the camera and that use but this
parser. **This refuses a `+`**, which the percent-decoding rule above otherwise
leaves intact: it is not in the alphabet, so a `b` carrying one is not an id
this protocol made. A derived id is 22 characters; the cap is generous rather
than tight.

`n` is optional and reads as empty when absent.

**`x`** is optional and holds the invite's expiry as a Unix timestamp in
seconds. **`v` and `x` are both bare decimal integers**: ASCII digits only, no
sign, no padding, no whitespace, and at most `9223372036854775807` — nineteen
digits. Anything else is `invite_missing_version` and `invite_bad_expiry`
respectively.

**An over-long numeral is refused, never raised.** A bound expressed only as
a comparison has to build the number first, and a reader whose integer type
refuses to convert an over-long numeral at all then fails on exactly the input
the bound exists to refuse. Checking the length first is one way; catching the
conversion is another. What a reader may not do is let the numeral escape as
something other than `invite_missing_version` or `invite_bad_expiry`.

Padding is refused for the same reason for both: `007` and `7` are one value
written two ways, and a length bound and a value bound disagree about which
numerals they are.

**An encoder refuses what this parser refuses.** A bound enforced only on
decode lets a caller build a URI no reader accepts, and the caller hears about
it from somebody else's scanner.

`invite_missing_version` covers a `v` that is absent **and** one that is
present and unreadable — non-decimal, padded, zero, or past the bound. A
wallet's user-facing string is derived from the code (§12), so it says the
invite states no version it can read, never that it states none.

Missing `v` is `invite_missing_version`; a version above the reader's is
`invite_future_version`; missing or empty `b` is `invite_missing_bill_id`;
missing or empty `k` is `invite_missing_key`; an unparseable `x` is
`invite_bad_expiry`. Anything that is not this scheme and host is
`invite_not_an_invite`.

Scanning the code is the whole of joining: no account, no server. That also
means anyone who photographs the screen can join, and holds the bill's key for
good: **an invite is a bearer credential this protocol cannot revoke.** `x` does
not change that. It is unauthenticated text, so removing it yields an invite
that opens the same bill, and the §11.2 payload carries no `x` at all. A key
that has been shown is shown; a host that wants to stop reading by people who
saw it has to move the bill to a new key, which this version does not specify.
`x` is a freshness hint a host MAY show a joiner, and nothing more.

**This protocol parses `x` and does not enforce it.** No refusal code here
means "expired", and no reader compares it to a clock: a clock is not in this
document's reach, the two devices' clocks disagree, and `at` is already
whatever its author wrote. Honouring an expiry is the host's (§13).

### 11.2 Scanned payloads

`splitz1:<base64url>` carries an invite together with the log, so a joiner who
scans it holds a bill rather than an id. `splitzd1:<base64url>` carries a
joiner's answer: the entries the inviter has not seen. Two scans, no network.

**A payload is capped at 2331 characters of encoded body in both
directions** — the base64url after the prefix, not the whole scanned string.
The prefix is fixed and known, so measuring it would make the two prefixes
carry different amounts. Refusing to encode
past it (`payload_too_large`) keeps a device from producing a code no camera
can read; refusing to *decode* past it keeps a stranger's code from handing a
device more work than a QR code could have carried. A cap enforced only on
encode bounds what an implementation emits rather than what it accepts, which
is the wrong direction for a trust boundary. A bill that has outgrown a scan
needs a relay.

The body is an object carrying `v` and `log`. **`v` is bounded exactly as
§11.1 bounds the invite's**: an integer of at least 1 and at most
`9223372036854775807`, and anything else is `payload_damaged`. Without the
bound each reader's integer type decides what one code means, and one QR
becomes `payload_damaged` to one wallet and `payload_future_version` to
another.

**Only `splitz1:` carries an `invite`**, and only an object is one. It is
*intended* to hold §11.1's `v`, `b` and `k`, without which a joiner has the
log and not the key it is encrypted under — but this section checks only that
it is an object. An empty one is accepted, and what is inside is the caller's
to put through §11.1. A reader **MUST** ignore an `invite` on a
`splitzd1:` payload and one that is not an object. A delta's reader already
holds a key, and a second one arriving from a peer names a bill and a key that
reader never chose. The member is carried verbatim for the caller to put
through §11.1; this section does not validate what is inside it.

**A body nests at most 64 levels deep**, and deeper is `payload_damaged`.
§10.1 applies the same bound to a single entry, with `bill_type_error`,
because an entry arriving over a relay (§11.3) never passes through this
section at all — and every pass that touches it walks it, deriving its id by
encoding it. A limit at one door only is a limit on the door nobody uses.
**The body itself is level 1, and every value occupies a level, scalars
included** — a string inside an array inside the body is level 3. Counting
only the containers gives a figure one smaller and a reader who does that
accepts a body these readers refuse, which is the divergence this paragraph
exists to prevent.

The limit is stated here rather than inherited from whichever JSON library a
reader links: one parser gives up at its own depth and another does not. The
cap is no defence, because a level of nesting costs two bytes. The deepest a
conforming document reaches is a participant id inside the `sharedBy` array
inside an itemised split, at **nine**: body, `log`, the entry, `expense`,
`split`, `items`, the item, `sharedBy`, the id.

2331 is what a version-40 QR code holds in byte mode at error-correction level
M. **The cap is that ceiling rather than a fraction of it, because the format
starts near it.** Every entry carries an 86-character signature; the
`createBill` a 43-character creator key and a 22-character nonce; a joiner's
participant a 43-character identity key (§10.7); a payout address about 106; and
base64url adds a third again.

Every figure below comes from a case in `vectors/payload.json`, which is
where the measurement lives; the numbers here follow it, and **the vector is
the normative statement**. Each row names its case, because the composition is
what sets the size and prose cannot carry all of it: rows three to five are
measured with ids `ana`/`ben`/`cai`, display names of three characters, a bill
name of six, 106-character payout addresses, and an expense whose description
is six characters.

| payload | case in `vectors/payload.json` | encoded |
|---|---|---|
| one `createBill`, two `joinBill`, no expenses, its invite, single-character ids, no display names, the joiner publishing an identity key | `the_smallest_signed_bill` | **1268** |
| the same with the ids and names a wallet would write | `…_a_wallet_would_write` | **1323** |
| two participants with payout addresses, one expense | `two_payable_participants_and_one_expense` | **2106** |
| three participants with payout addresses, no expenses | `three_payable_participants_and_no_expenses` | **2188** |
| three participants with payout addresses, one expense | `three_payable_participants_and_one_expense` | refused |

The headroom on the third row is 225 characters, so the free text on an
expense is part of the budget: the same bill with a 176-character description
is refused.

**The second half of that table is the number a wallet plans against.** A
signed bill with no payout address on it cannot be settled from, and each
participant that carries one costs between 531 and 553 characters, depending
on the length of their id and display name. So a scanned
`splitz1:` reaches **two people and one expense, or three people and none.** A
bill past that needs a relay — which is what "a bill that has outgrown a scan"
above means, and it is reached sooner than the floor suggests.

An implementation **MUST NOT** set the cap below the smallest signed bill, and
a conformance suite **SHOULD** assert that the smallest signed bill encodes.
Note the floor is not a licence: a cap between the floor and the ceiling
refuses no bill at all in the first row and every bill in the fourth.

A body that is not this prefix is `payload_not_a_payload`; one whose base64url
does not decode is `payload_damaged`; one that decodes to an object carrying no
log is `payload_missing_body`; a version above the reader's is
`payload_future_version`.

The encoded body is canonical JSON (§9.3), base64url without padding. Scan
padding (§11.1) is stripped from both ends before the prefix is matched and
before the size is measured, so padding does not count toward the cap.

### 11.3 Sealing an entry for a transport

§11.2's payloads carry a log between two phones in the room. A bill that
outgrows a scan needs a relay, and the moment an entry crosses a wire it is the
graph of who ate with whom and who owes whom — which nothing outside the bill
should hold. So an entry is sealed under the invite's key before any transport
sees it, and a relay stores a blob, its size, and when it changed.

Encrypting it is the host's (§13). What this section fixes is everything two
wallets must agree on **before** the cipher runs, because a wallet that differs
on any of it writes blobs another cannot open — or opens them and stores every
entry twice.

**The cipher** is XChaCha20-Poly1305: a 192-bit nonce, a 256-bit key, a 128-bit
tag. The nonce is long enough to be drawn per blob without the birthday bound a
96-bit nonce carries, so no device keeps a counter and no two devices
coordinate one.

**The key** is the 32 bytes the invite carries as `k` (§11.1).

**The plaintext** is the entry's canonical JSON (§9.3), UTF-8 — **canonical,
not whatever the encoder emitted.** The nonce is derived from these bytes, so
two implementations ordering keys differently seal one entry into two different
blobs and a relay keyed by content holds both. Neither can detect it alone:
each opens its own perfectly.

**The nonce** is `SHA-256(plaintext)` truncated to 24 bytes. Derived rather
than random, so the same entry always seals to the same blob: a relay stores it
once however many times it is pushed, and two devices that both hold an entry
produce byte-identical ciphertext.

Two *different* plaintexts never share a nonce, which is the one condition the
cipher requires — a single byte of difference gives a different digest.
Deriving it from the entry id instead would repeat a nonce whenever two
payloads carried one id, and a stream cipher under a repeated (key, nonce)
hands a relay the xor of two plaintexts it holds no key for. A random nonce
would avoid reuse and make every re-sync a fresh blob, growing the channel
without bound.

**The frame** is, in order: one version byte, the 24-byte nonce, then the
cipher's output — ciphertext followed by its tag. The whole is unpadded
base64url. A reader knowing the fixed nonce and tag lengths splits it apart
with no length fields.

The version is **1**. A reader MUST refuse one above its own with
`sealed_future_version` rather than guess, and one below 1, an unparseable
body, or a frame too short to hold a nonce and a tag with `sealed_malformed`.
**A version of zero is a malformed frame and not a future format:** telling
somebody their app is too old sends them to an update that will not help.

**The channel** a bill's blobs are pushed to and pulled from is the bill id's
SHA-256, lower-case hex. A digest rather than the id itself, because the id is
a live address printed in every invite; every participant knows it and computes
the same channel, and a relay that only ever sees traffic cannot run it
backwards.

**What a relay is not told, and what it still learns.** It never sees a name,
an amount or an address. It does see how many blobs a channel holds, how large
they are, and when each changed — which is a count of entries and a rhythm of
activity. A wallet that needs more than that needs a different transport, and
this protocol does not specify one.

**A blob that does not open** — wrong key, altered bytes, or a plaintext that
is not an entry — is skipped, not raised. One unopenable blob on a channel must
not stop the rest of a sync: anybody who has the channel can push one.

## 12. Error codes

`empty_weights`, `negative_weight`, `zero_weight_sum`, `allocation_overflow`,
`weight_sum_overflow`, `negative_share`, `amount_overflow`,
`exact_limit_too_large`, `unauthorized_entry`, `amend_kind_mismatch`,
`bill_ambiguous_entry`, `bill_missing_entry_payload`, `empty_split`,
`exact_total_mismatch`, `percentage_not_full_scale`, `itemized_no_items`,
`itemized_unassigned_item`, `itemized_total_mismatch`, `currency_mismatch`,
`unknown_participant`, `unknown_entry`, `duplicate_participant`,
`duplicate_payment`,
`self_payment`, `bill_bad_participant_id`,
`unknown_payment`, `unauthorized_confirmation`,
`confirmation_missing_reference`, `unauthorized_payment`,
`participant_still_named`, `ambiguous_create`,
`bill_unknown_confirmation_method`, `canonical_json_float`,
`rate_currency_mismatch`, `rate_not_positive`, `negative_amount`,
`rate_amount_too_large`, `balances_nonzero_residual`, `log_empty`,
`log_no_create`, `create_unbound`, `create_id_not_derived`,
`bill_missing_version`, `bill_future_version`, `bill_type_error`,
`bill_missing_currency`, `bill_bad_currency`, `bill_not_scalar_values`,
`entry_id_not_derived`,
`bill_unknown_split_type`,
`bill_unknown_split_mode`, `bill_unknown_entry_kind`,
`bill_unknown_settlement_method`, `bill_unknown_payout_method`,
`invite_not_an_invite`, `invite_missing_version`, `invite_future_version`,
`invite_missing_bill_id`, `invite_bad_bill_id`, `invite_missing_key`,
`invite_bad_expiry`, `payload_not_a_payload`, `payload_damaged`,
`payload_missing_body`, `payload_future_version`, `payload_too_large`,
`sealed_malformed`, `sealed_future_version`, `zip321_no_payments`,
`zip321_too_many_payments`, `zip321_amount_not_positive`,
`zip321_amount_too_large`, `zip321_memo_too_large`, `zip321_bad_address`,
`zip321_bad_currency_code`, `zip321_fiat_not_positive`,
`zip321_fiat_too_many_digits`, `zip321_no_address`.

**The code is part of the protocol; the message that accompanies it is prose
and is not.** A user-facing string MUST be derived from the code.

**Every code in this list MUST have at least one vector that produces it**, with
one exception, named below. A refusal nothing exercises is either unreachable,
misspelled at one of its two ends, or firing on the wrong input, and no lane
would say which.

`balances_nonzero_residual` cannot be raised by §5 from a bill. Section 4
requires every split to sum exactly to its expense total, so an expense moves
the sum of net balances by zero; a payment credits and debits the same amount;
and every step is checked arithmetic that refuses with `amount_overflow` rather
than wrap. It is reachable from §6, which takes balances directly rather than
deriving them, and a caller may hand it a set that does not sum to zero —
which is what its vectors in `settlement.json` do.

`bill_not_scalar_values` is that exception, and it is structural. The
input that produces it is a JSON document carrying a lone surrogate, and a
conformant JSON reader refuses such a document: a vector file containing one
could not be parsed by an implementation whose strings are UTF-8 by
construction, so the case would take the whole corpus down rather than test
one code. Each implementation whose string type admits a lone surrogate MUST
carry the case in its own suite instead, and one that cannot represent the
input at all is conformant without it.

## 13. What this protocol does not specify

- **Transport.** §11.3 fixes what a sealed entry looks like and the channel it
  belongs to; moving blobs — over what, with what retries, stored for how long
  — is the wallet's. So is running the cipher §11.3 names.
- **Address validation and network.** §8.3 checks only what the ZIP 321 grammar
  admits. A wallet MUST decode every address itself — **ZIP 316** defines the
  Unified Address format — and MUST check it is for the network it is
  transacting on. The vectors carry real mainnet Unified Addresses precisely so
  that a wallet running the corpus puts each one through its own decoder rather
  than through filler that would never reach one.
- **Transaction construction, fees, signing, broadcast.** §8's payment request
  is one input to that, and it is not the only shape a wallet may want: a
  partially-created transaction (PCZT) carries a transaction between a creator,
  an updater, signers, a prover and a combiner before an extractor turns it
  into something broadcastable, which is the route to take when several people
  contribute to one transaction rather than each sending their own. This
  protocol produces a URI and stops; which of the two a wallet builds from it
  is the wallet's.
- **The curve operation.** §10.6 fixes the message a signature covers and §10.1
  what a host that verifies must check; producing and checking the Ed25519
  signature itself is the host's, as is where the private half is kept.
- **Where a private key lives**, and how a participant comes by one. §10.7
  fixes what a published key binds and what a reader does with it; minting and
  storing the private half is the wallet's.
- **Storage.** Every participant id other than the bill creator's key is opaque
  and unauthenticated until §10.7 binds it. Anyone holding the invite can
  append any entry as anyone; §10.4 narrows what that achieves silently, and
  does not prevent it.
- **Surfacing a changed pay-to address.** §10.3 records every one; a wallet
  MUST show them before settling.
- **Not attaching a memo to a transparent recipient.** ZIP 321 requires a URI
  carrying a memo at the same parameter index as a transparent address to be
  refused in its entirety, which takes the unrelated shielded outputs with it.
  This protocol does not parse addresses, so the rule cannot live in §8; a
  wallet has a decoder and MUST spend it before setting a memo.
- **Honouring an invite's expiry.** §11.1 fixes what `x` looks like and parses
  it; comparing it to a clock is the wallet's, along with which clock and what
  to do about the two devices disagreeing. There is no refusal code for an
  expired invite because this protocol cannot tell one.
- **Rate discovery.** §7 specifies what a snapshotted rate does, not where it
  came from.

## 14. What a host must supply, show and refuse

§13 lists what this protocol leaves to a wallet. This section states what it
requires of one. Keeping the wire format is not sufficient: the rules below
decide what a person is asked to pay, and a wallet that keeps §1–§12 and
breaks these sends money to the wrong place or sends it twice.

### 14.1 What a host supplies

- **The participant id it speaks as.** Every entry it writes is authored by
  that id, and §10.4 decides what the id authorises.
- **The address it is paid at, or none.** A participant with no address is
  reported under §8.4 and MUST NOT be dropped from a request.
- **A clock** producing §9.3 instants. It is read when an entry is written and
  never while folding: §10.2 orders a log by instant, so a fold that consulted
  a clock would return different bills for one entry set.
- **Unpredictable randomness** for §9.4's nonce. A bill's id is the digest of
  the entry that opened it, so two bills opened in the same second by the same
  participant are one bill unless the nonce cannot be guessed.
- **A way to send** a §8 payment request, answering as §14.3 requires.
- **Optionally a signature and a verifier** over §10.6's message. A host that
  supplies neither leaves every participant unauthenticated, which §10.7
  admits and which is a different statement from a bill whose identities were
  checked and found sound.

### 14.2 What a host MUST put in front of a payer before settling

Each of these is an answer the protocol produces and discards nowhere. A
request that omits them looks, to the person paying, exactly like one that has
nothing to omit.

- Every recipient the request cannot carry, with the reason for each (§8.4).
- Every pay-to address the fold recorded as replaced (§10.3).
- Every debt with a payment recorded and not yet confirmed (§10.5).
- Every contested identity among the recipients (§10.7).
- The rate the request was priced at, who set it, and the ZEC amount and
  address of every output. A participant owed money can set the rate, and a
  request stated only in the bill's currency hides what that rate did to the
  ZEC it asks for.

### 14.3 A send has three outcomes, not two

A transaction may reach the network, may be refused before it is built, or may
be **built and signed and not handed to the network**. The third may still
land.

- A wallet MUST NOT record a payment (§10.5) for any outcome but the first.
- A wallet MUST NOT retry the third as though it had failed.
- A host's send therefore reports which of the three occurred, rather than
  returning a transaction id or raising.

Recording the third as paid settles a debt that nothing on chain settled.
Retrying it pays the debt twice, and this protocol has no remedy for an
overpayment.

### 14.4 A pending payment withholds the whole debt

§10.5 moves a balance only on confirmation, so a debt this payer has already
paid is still in the plan §6 produces.

- A wallet MUST NOT include such a debt in a request. A settlement carries
  such a debt when a payment the payer recorded — a record whose entry the
  payer wrote — and that is not confirmed, names its payee **or any creditor
  whose debt it covers** (§6.3). A record its payee wrote is their word, not
  a payment the payer has in flight, and holding a debt back on it would let
  any covered creditor stop the payer settling. Netting
  reroutes a debt the payer has already paid onto somebody else, and matching
  on the payee alone asks for it again.
- Where the amount pending is **less** than the debt, the **whole** debt is
  withheld rather than the remainder. Requesting the remainder overpays by the
  pending amount if that payment lands.
- What is owed and what is pending are reported as two quantities. On a part
  payment they differ, and presenting the debt as the amount in flight states
  something untrue. The pending amount is the sum over the payee and every
  creditor the settlement covers.

A payment that never lands is withheld by the same rule, and §10.8's
withdrawal of its record is what releases the debt.

### 14.5 A peer who is current and a peer who is behind are different answers

§11.2 caps a payload, so the entries a peer has not seen may not fit in one.
A wallet computing them MUST distinguish three states: the peer holds
everything; the peer is missing entries that fit; the peer is missing entries
that do not. Reporting the third as the first tells somebody their bill is up
to date while entries on it have never reached them.

## 15. The wallet seam

§13 lists what this protocol leaves to a wallet and §14 what it requires of
one. Both are about the protocol itself. This section specifies the seam one
layer out: the interfaces a wallet implements so that everything between the
protocol and a screen — the log a device keeps, the sync that moves a bill
between devices, the sealing of §11.3, the rate of §7 and the settlement of a
debt owed in another asset — is written once and adopted unchanged.

Seven interfaces. They are stated here in language-neutral terms; a binding
names them in its own idiom. A host conforms with this section when each
interface exists with the operations named for it and keeps the rules stated
under it. That is a separate claim from reproducing the corpus: §1's
conformance is about answers, this one is about the contract a wallet signs.

**An error is raised, never returned as a value that reads as success.** Where
an interface below has no answer to give — no price, no address, nothing
stored — it says so with an explicit empty answer that a caller must handle,
and that empty answer is an ordinary state rather than a failure. An interface
that fails silently is indistinguishable from one that is working, and the
difference is money.

### 15.1 `SplitsWallet`

**Operations.** `account`, `sender`, `secrets`, `now`, `randomBytes`.

The device itself: who it speaks as, what it can spend, where its secrets go.

- `account` carries the participant id every entry this device writes is
  authored by, and §10.4 decides what that id authorises. It MUST be stable for
  the life of an installed wallet; an id that changes between runs makes this
  device a new participant on every bill it has already touched.
- `account` MAY carry an identity secret and MAY carry none. When it carries
  one it MUST be derived from what only the account's owner holds — for a
  software wallet, its mnemonic and passphrase — so that reinstalling from the
  same mnemonic yields the same signing identity. It MUST NOT be anything the
  wallet shows or shares, such as a viewing key: whoever holds it can sign as
  this participant, bind uncontested (§10.7), and redirect what they are paid.
  An identifier the wallet's own database assigns MUST NOT be supplied in its
  place either: it is handed out at import time, so an identity filed under it
  is a stranger to every bill naming it after a restore. No secret — a
  hardware account keeps none on the phone — means the identity is random and
  unrecoverable; it still signs correctly.
- `now` MUST produce a §9.3 instant. It MUST be read when an entry is written
  and MUST NOT be read while folding: §10.2 orders a log by instant, so a fold
  that consulted a clock would answer differently for one unchanged entry set.
- `randomBytes` MUST be unpredictable. §9.4 derives a bill's id from a nonce,
  so two bills opened in the same second by the same participant are one bill
  when it can be guessed.

### 15.2 `WalletSender`

**Operations.** `send`, `payToAddress`.

- `send` takes one whole §8 payment request URI and MUST build, sign and
  broadcast a **single** transaction paying every output in it. One transaction
  per recipient is a different thing: it costs a fee and a proof each, and it
  lets a payer stop after the second while the bill records the first two as
  settled.
- `send` MUST report which of §14.3's three outcomes occurred rather than
  returning a transaction id or raising. Where an implementation distinguishes
  a refusal it chose from one the network gave, both are §14.3's second
  outcome — nothing was spent — and they differ only in what a person is told.
- A transaction id MUST be present when and only when the send succeeded. It
  becomes the id of the payment entry §10.5 records, so the record of a payment
  and the transaction that made it carry one identifier.
- `payToAddress` MAY be absent. A participant with no address is reported under
  §8.4 and MUST NOT be dropped from a request, so absence is a state to show
  and not an error.

### 15.3 `SecretStore`

**Operations.** `read`, `write`, `delete`.

Where this layer's secrets live — a platform keychain in a shipped build.

- A value written MUST be readable after the process that wrote it has ended.
  A bill key that does not outlive the process cannot decrypt that bill
  tomorrow, so an in-memory store is a test fixture and MUST NOT be the one a
  build ships.
- Reading a key that was never written, or was deleted, MUST answer empty
  rather than raising.

### 15.4 `BillStorage`

**Operations.** `read`, `write`, `delete`, `keys`, `sweepUnfinishedWrites`.

Durable storage for this device's bills, **as raw entries**.

- Entries, not folded bills. §10.2 merges by set union, so a device holds
  entries and derives everything else; a stored summary is a second source of
  truth that goes stale without announcing it.
- `keys` MUST answer with every key currently stored under the given prefix,
  and MUST NOT include one whose write did not finish.
- `sweepUnfinishedWrites` MUST remove whatever an unfinished write left behind
  and MUST report how many it removed. A store that cannot leave anything
  behind reports zero. It is called once when the feature loads: a leftover is
  already invisible to `keys`, and this is what stops them accumulating across
  a year of crashes.

### 15.5 `SplitsRelay`

**Operations.** `push`, `fetch`.

A store of opaque blobs grouped into per-bill channels, for the participant who
left before dessert and cannot be handed a code across the table.

- **Optional, and meant to stay so.** A bill is created, split and settled with
  no relay at all and shared by §11.2 payload; only asynchronous catch-up is
  missing without one.
- The channel a bill syncs under is the SHA-256 of its bill id, hex encoded —
  never the id itself, which is live in every invite and every scanned payload.
  Every participant knows the bill id and so derives the same channel, and an
  observer of relay traffic alone cannot run it back to the id.
- A relay holds no key. Every blob it carries is a §11.3 sealed entry and is
  opaque to it. What it may observe is the minimum: that a channel has blobs,
  how large they are, and when they last changed. It MUST NOT be given anything
  from which it can learn who owes whom.
- `push` MUST be idempotent: pushing a blob a channel already holds changes
  nothing, so a retry after a dropped connection cannot create a duplicate.
- `fetch` MUST answer with every blob the channel currently holds. The caller
  merges by entry id and §10.2's merge is idempotent, so returning blobs the
  caller already has is harmless — a relay has no caller identity to key
  per-caller state on and MUST NOT be asked to keep any.
- A relay that cannot be reached, refuses, or answers with something that is
  not a channel MUST raise, and the raised error MUST say whether retrying
  later could plausibly succeed. It MUST NOT be swallowed: a bill works with no
  relay, so a transport that quietly does nothing looks exactly like one that
  is working.

### 15.6 `ZecPrices`

**Operations.** `minorUnitsPerZec`.

- The answer is what one ZEC costs in the **minor units** of the named
  currency, as an integer — never a decimal. §7 snapshots that figure onto the
  bill as an integer, so the source's precision is rounded away once, here,
  where it is known, rather than at every place that reads it.
- A source that cannot price a currency MUST answer empty. That is an ordinary
  answer: a bill with no rate is an ordinary bill, there is no §12 code for an
  unpriced one, and an implementation MUST NOT invent a figure to avoid showing
  that state.

### 15.7 `SwapProvider`

**Operations.** `tradableAssets`, `quote`, `statusOf`.

Settles a debt whose payout is owed in some asset other than ZEC.

- `tradableAssets` MUST be read before quoting, so a payout naming an asset the
  provider does not carry is refused before a person is asked to send anything.
- `quote` MUST be given a refund address, and it is the payer's own. A quote
  arranged without one risks the deposit if the swap fails.
- `statusOf` reports whether the swap is in flight, delivered, or will not
  complete. A transaction hash it reports is on the **destination** chain, and
  MUST NOT be recorded as the §10.5 payment for a debt settled on this one.
- A provider a build has not configured MUST raise and say why. Every operation
  failing loudly is the alternative to a swap that silently does nothing.
