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

A figure a person types is read in integers, never through floating point
(`0.29 * 100` is not 29 in binary floating point, and that rounding is money):
`parseAmountIn` / `parse_amount_in` read it at the exponent the host's register
gives, and refuse a figure with no digit, more decimals than the currency has,
a value past a signed 64-bit integer, a `,` followed by exactly three digits
(`1,000` is a thousand to one reader and one to another), and any code the
register gives no exponent. Only ASCII white space is trimmed and only ASCII
digits read, so one string reads alike in every language.

Arithmetic between two currencies MUST be refused with `currency_mismatch`
rather than converted.

### 2.2 Bounds

Every amount MUST be representable in a signed 64-bit integer, and so MUST
every total, sum and intermediate product formed from amounts. An operation
whose result would exceed that range MUST be refused with `amount_overflow`, or
with the narrower code §3 and §7 name, and MUST NOT be allowed to wrap.

**One expense MUST carry at most `92233720368` minor units in magnitude**,
refused with `amount_too_large`. The check comes after the rest of the
expense payload has decoded (§9.1). The figure is the largest §7.1 can price:
`92233720368 × 100000000` fits a signed 64-bit integer and one unit more does
not. A balance reaches the 64-bit bound only after 100000001 expenses at the
cap, so the overflow refusals below guard a log no bill holds in practice;
they stay, because wrapping is never allowed. A debt formed from several
expenses can still exceed what one request prices, which §7.1 refuses with
`rate_amount_too_large`. Payments carry no cap of their own: a payment
settles a debt, and a debt is a sum of expenses.

**A balance MUST lie within ±(2^63 − 1)**, refused with `amount_overflow`.
The range of a signed 64-bit integer is one wider below zero than above, and
a balance of −2^63 has no magnitude that type can hold: §5.1's residual and
§6's matching both form one. A payment a participant records to themselves
and confirms as its recipient reaches that value with no other party's
agreement, so the fold (§10.3) sets aside the entry whose effect would leave
any balance there, and §5 refuses a document whose balances reach it.

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

4. **Each product `m · wᵢ` MUST be formed exactly**, in at least 126 bits:
   `m` and `wᵢ` are each below 2^63, so their product is below 2^126. A
   wrapped product allocates a plausible wrong number, which passes every
   check downstream of it; one refused at 64 bits turns away ordinary
   amounts whose every part fits — a 10% tip on 10,200,000,000 minor units
   is a product of 9.36 × 10^18. `pᵢ ≤ m` and `rᵢ < W`, so both fit 64 bits.

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
§14.4 sums the unconfirmed part of that total. Without these, a log long
enough to pass the 64-bit bound, written by anyone holding the invite, would
leave §5 refusing the whole bill on every device and nobody able to settle.

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

**`unexplained` is not evidence of a refund.** Coverage reads only direct
debts, so a confirmed payment that ended up larger than what was owed — an
expense corrected downward after it was paid — leaves the reverse settlement
unexplained on a bill that holds no refund at all. A host names a refund as
the cause only when negative expenses on the bill account for the
unexplained part (`refundsBehind` / `refunds_behind`); otherwise it says
only that no debt the bill records explains it.

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
  a memo shares a parameter index with a transparent address, which takes the
  unrelated shielded outputs with it. A payment carrying a memo — an empty one
  included — MUST therefore name an address §8.6 accepts (`address_invalid`)
  and whose answer says a memo can be delivered to it
  (`zip321_memo_undeliverable`). A payment carrying no memo is not decoded.

- An address is written verbatim. It MUST be present and non-empty
  (`zip321_no_address`) and **ASCII alphanumeric** — `A`–`Z`, `a`–`z`, `0`–`9`
  and nothing else — which is all the ZIP 321 grammar admits
  (`zip321_bad_address`): that grammar reads
  `zcashaddress = 1*( ALPHA / DIGIT )`, and RFC 3986's `ALPHA` and `DIGIT` are
  ASCII. A Unicode-aware test is a different rule: it admits U+00E9 and
  U+FF12, which are letters and digits and are not in the grammar. **For a
  payment with no memo this is a syntactic check, not validation:** §8.6
  decodes an address, and a wallet MUST check that the network it answers is
  the one it is transacting on.

  **The address is checked before any other parameter of the same payment**,
  the memo rule above included, so a payment invalid in two ways is refused
  with the same code everywhere.

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

**Each output to an address that can receive a memo (§8.6) carries one: the
UTF-8 bytes of `splitz:` and the bill's id**, as ZIP 321's `memo`. It is what
ties a send to the bill (§14.7): a transaction id proves money arrived, not
what it was sent for. An output to an address that takes no memo carries none,
and an address no reader decodes is taken as one that takes none.

**An obligation is one payer's.** Settlements naming more than one `from` are
refused whole with `obligation_mixed_payers`, before any output is rendered: a
request built from a whole plan asks the payer holding it to send every other
payer's debts too, and nothing on the payer's screen shows that it does.

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

**A debt past what one request can price is excluded the same way**, reported
with the reason `unpriceable`. A debt is a sum of expenses, so it can exceed
what §7.1 converts (`rate_amount_too_large`), or convert at a low rate to more
than one §8.1 output carries (`zip321_amount_too_large`), or, with `fiat`
included, to more digits than §8.4 writes (`zip321_fiat_too_many_digits`).
Each of those is about one output's size, so it excludes that recipient and
not the others; an implementation that refuses rather than reports refuses
with that code. A refusal about the rate itself — its currency, its sign — is
the same for every output and refuses the whole request.

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

### 8.6 Zcash addresses

Given a string, an implementation answers four things or refuses with
`address_invalid`:

- `network`: `main`, `test` or `regtest`;
- `kind`: `p2pkh`, `p2sh`, `tex`, `sapling` or `unified`;
- `receivers`: for `unified`, the typecode of every item, in encoding order.
  `0` is P2PKH, `1` P2SH, `2` Sapling and `3` Orchard; every other typecode is
  kept by its number. Empty for every other kind;
- `canReceiveMemo`: false for `p2pkh`, `p2sh` and `tex`; true for `sapling`;
  for `unified`, true when `receivers` holds `2` or `3`. Every Unified Address
  this section accepts holds one of them, so it is true for each.

The string is taken exactly. Nothing is trimmed, and a string with any byte
outside what the encoding it claims admits is refused.

| kind | bytes | encoding | prefix: main / test / regtest |
|---|---|---|---|
| `p2pkh` | 20 | Base58Check | lead `1C B8` / `1D 25` / `1D 25` |
| `p2sh` | 20 | Base58Check | lead `1C BD` / `1C BA` / `1C BA` |
| `tex` | 20 | Bech32m | `tex` / `textest` / `texregtest` |
| `sapling` | 43 | Bech32 | `zs` / `ztestsapling` / `zregtestsapling` |
| `unified` | items | Bech32m over F4Jumble | `u` / `utest` / `uregtest` |

Regtest shares testnet's Base58Check lead bytes, so a transparent address on
either answers `test`. Every lead byte and prefix is zcash_protocol 0.10's
(`constants/{mainnet,testnet,regtest}.rs`); ZIP 320 and ZIP 316 define the
main and test prefixes of `tex` and `unified` the same way. Base58Check is the lead bytes, the 20-byte hash and four checksum bytes, the
first four of SHA-256d over the rest; any other decoded length is refused.

**Bech32 and Bech32m** (ZIP 173, BIP 350) are read in lower case only. ZIP 173
has an encoder write lower case, and the decoder librustzcash wallets use
refuses upper case, so an address accepted here in upper case could be written
into a request a payer's wallet cannot read. The prefix is everything before
the last `1`; the checksum MUST verify under the constant the kind requires —
a Sapling address under Bech32m, or a TEX or Unified Address under Bech32, is
refused. The 5-bit groups regroup into bytes, and the bits left over MUST
number at most four and be zero. There is no length limit on a Unified
Address.

**A Unified Address** is ZIP 316 **revision 0**, the revision ZIP 316 marks
active. Revision 1 is withdrawn and revision 2 is a draft; its `zu` and `tu`
prefixes are not in the table and are refused. The decoded bytes MUST number
48 to 4194368, the lengths F4Jumble⁻¹ accepts. After F4Jumble⁻¹ (BLAKE2b,
personalized `UA_F4Jumble_H` and `UA_F4Jumble_G`, as ZIP 316 "Jumbling"
defines it) the last 16 bytes MUST be the prefix zero-padded to 16; the rest
is a sequence of items, each a typecode, a length and that many bytes. The
typecode and the length are compactSize values in their shortest encoding,
at most `0x2000000`. The address is refused when:

- an item runs past the end;
- a P2PKH or P2SH item is not 20 bytes, or a Sapling or Orchard item not 43;
- the typecodes are not strictly ascending, which refuses a repeat and a
  reordering alike;
- it carries both P2PKH and P2SH;
- it carries a typecode in `0xE0`–`0xFC`, which revision 0 forbids;
- it carries neither Sapling nor Orchard. ZIP 316 requires a revision 0
  address to carry typecode `0x02` or `0x03`, so an unknown typecode beside a
  transparent one does not satisfy it.

Sprout addresses are refused: ZIP 211 closed the Sprout pool to new funds.
**This section checks encodings, not keys.** A receiver's bytes are counted,
not decoded as a curve point; a wallet's own decoder does that when it builds
the transaction.

`vectors/address.json` carries every kind under every prefix in the table, the
Unified Address test vectors of zcash-test-vectors, and every refusal above.

### 8.7 Reading a request back

A request this protocol wrote is read back by rendering what was read and
comparing. Reading is structural: `zcash:`, an optional path address, a `?`,
and `&`-separated parameters named `address`, `amount`, `fiat`, `memo`,
`label` or `message`, each with the empty index or `.1` to `.9999` without a
leading zero, each at most once per index, the indices running from the empty
one without a gap. A path address stands for payment zero's `address`, which
then MUST NOT also be written. `amount` is up to eight digits, optionally a
period and one to eight digits; `fiat` is three upper-case letters, a colon
and one to eighteen digits; `memo` is unpadded base64url whose bits end on a
byte; `label` and `message` are percent-decoded, and a malformed escape, a raw
non-ASCII character or bytes that are not UTF-8 do not read.

The payments read are then rendered under §8.2, with `fiat` included when any
payment carries one, and the result MUST equal the input byte for byte.
Anything that does not read, and anything whose rendering differs, is refused
with `zip321_not_canonical`; a value the rendering itself refuses is refused
with that rendering's code.

This is not a general ZIP 321 reader, and does not replace one. It exists so a
host can hold the request it is about to send next to what the wallet made of
it (§14.6).

## 9. The bill wire format

```json
{
  "v": 1,
  "id": "weekend",
  "name": "Zcon7 weekend",
  "currency": "MXN",
  "splitMode": "equal",
  "participants": [{"id": "ana", "name": "Ana", "payTo": "u1…"}],
  "expenses": [{
    "id": "ana:dinner", "description": "dinner", "paidBy": "ana",
    "amount": 480000, "currency": "MXN",
    "at": "2026-10-28T19:30:00.000Z",
    "split": {"type": "equal", "among": ["ana", "ben"]}
  }],
  "payments": [{
    "id": "ben:p1", "from": "ben", "to": "ana",
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

**The bill's `currency`** MUST be present and non-empty
(`bill_missing_currency`). **Every `currency`** — the bill's, an expense's or
payment's own, an entry's own field, and a rate's — MUST be an ISO 4217
alpha-3 code in upper case (`bill_bad_currency`, §2.1). An empty one other
than the bill's is not a code, and is refused as `bill_bad_currency`.

A display **`name`** is chosen by whoever joins, and two people can choose
one a reader cannot tell apart, by accident or to pass as somebody already on
the bill. A host SHOULD show such a name qualified: the bill's creator as the
organiser, anybody else with the last eight characters of their id, or the
whole id when another's last eight match. Two names collide when their
skeletons match (`nameSkeleton` / `name_skeleton`, `displayNameOf` /
`display_name_of`, `display_names` in the binding). A skeleton is the name
with, in this order:

1. Greek and Cyrillic capitals that render as a Latin capital mapped to that
   Latin letter in lower case — Α Β Ε Ζ Η Ι Κ Μ Ν Ο Ρ Τ Υ Χ and Ѕ І Ј А В Е К
   М Н О Р С Т У Х — before anything is case folded, since folding first turns
   Ν and Υ into ν and υ, which render as v and u;
2. every other letter in U+0000–U+024F, U+0370–U+037E, U+0380–U+0523,
   U+0531–U+0556 and U+1E00–U+1FFF lower-cased, and none elsewhere, so every
   implementation folds alike whatever Unicode version its library carries;
3. invisible characters removed — U+00AD, U+034F, U+061C, U+115F–U+1160,
   U+17B4–U+17B5, U+180B–U+180F, U+200B–U+200F, U+202A–U+202E,
   U+2060–U+206F, U+3164, U+FE00–U+FE0F, U+FEFF, U+FFA0 and
   U+E0000–U+E0FFF — and combining marks in U+0300–U+036F, U+1AB0–U+1AFF,
   U+1DC0–U+1DFF, U+20D0–U+20FF and U+FE20–U+FE2F removed; every run of
   Unicode White_Space collapsed to one space, none leading;
4. fullwidth and mathematical alphanumerics mapped to the ASCII letter or
   digit they draw; the precomposed Latin letters in U+00C0–U+0233 that
   decompose to a plain letter and combining marks mapped to that letter
   (outside that range, a precomposed letter keeps its marks: `ẹ` U+1EB9 does
   not meet `e`); lower-case Cyrillic and Greek letters that render as Latin
   ones mapped to it; and `l`, `1`, `ı`, `ǀ` and `ӏ` to `i`, which a
   sans-serif capital I is drawn as.

These sets are the whole of steps 3 and 4. Characters outside them — other
combining marks, other format characters — are kept, so every implementation
folds alike whatever Unicode version its library carries.

The vectors in both host packages' naming tests pin the table.

**Participant ids** MUST be unique within a document (`duplicate_participant`).

**`identityKey`** on a participant is the Ed25519 public key that alone may
write as them (§10.7): 32 bytes, canonical unpadded base64url, refused with
`bill_type_error` otherwise, because a participant's id is derived from it.
Optional; absent means the identity is unclaimed.

**`payouts`** on a participant is how they want to be paid, most preferred
first: a list of `{"type": "zec", "address": …}`, `{"type": "swap", "asset": …,
"chain": …, "address": …}` or `{"type": "cash"}`. Optional; empty means none is
declared and `payTo` stands in. A host MUST NOT write a `zec` payout with no
`address`, or a `swap` payout missing its `asset`, `chain` or `address`, or
naming any of them as an empty or white-space string (`payout_incomplete`): a
payer takes such a payout as one it cannot pay and moves to the next
preference (§14.8), so the person is paid in a way they did not ask for first.

A reader MUST refuse a `type` it does not define
(`bill_unknown_payout_method`) rather than skip it. **Skipping settles to the
next preference down, which is a different address.** Order is the preference
order and MUST be preserved.

A preference takes no part in any arithmetic; §6 decides who owes what.
Honouring one — building the transaction, performing a swap, telling a person
to hand over cash — is the wallet's. A payer MAY settle one debt by a lower
preference the recipient declared, as its own choice for that payment and
never by rewriting the order (§14.8).

**Setting or changing one way of being paid** writes the whole list again, in
this order: the new payout first, then every payout the record already
declares that the new one does not replace, in its declared order. A new payout
replaces every declared payout of its own `type`, and a `swap` only those of
the same `asset` (compared case-insensitively; `chain` does not distinguish).
A record that declares no payouts declares its `payTo` as one `zec` payout for
this purpose, and one with neither declares nothing. `rankedPayouts` /
`ranked_payouts` takes a participant and the new payout and returns that list.

**A participant id MUST be a non-empty string holding no `:`**
(`bill_bad_participant_id`). `:` ends the part of an expense or payment id
that names its author (§10.3): an author whose id holds one mints nothing, so
could join a bill and never write to it.
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
for something. Any other value that is not a list — an empty string, an
object, `false` — is refused with `bill_type_error`, never read as empty: one
reader taking `""` for "no participants" and another refusing it open two
different documents from one text. **An optional member that is a scalar does not**: there `null`
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
what settles the debt, and neither takes any part in §5 or §6. A record of a
Zcash send SHOULD carry both. The payee confirms the record (§10.5), and a
fiat amount alone hides a rate lowered before paying: the record says the
debt was paid in full while the ZEC that arrived covers a fraction of it.

`zatoshi` MUST be an integer (`bill_type_error`) and greater than zero
(`negative_amount`). `paidAtRate` MUST be an object (`bill_type_error`),
decoded as the rate §7 describes with each member checked, and MUST
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
every swap. A host MUST NOT write a `swap` payment with no `reference`, or one
that is only white space (`swap_missing_reference`): neither side could find
the swap again.

A host MUST NOT write a payment whose `amount` is zero or less
(`payment_not_positive`). A reader still accepts a zero `amount`, so a log another
writer produced folds alike everywhere; but a payment of nothing records
nothing, and while unconfirmed it withholds the whole debt it names from its
payer's next request (§14.4).

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

**A number's code follows from its value, not its spelling.** A JSON reader may
hold an integer too large for 64 bits as a double, and then cannot tell
`9223372036854775808` from `9.223372036854775808e18`. So wherever this protocol
refuses a number that is not an integer a signed 64-bit value holds, one whose
magnitude reaches 2^63 — or that no double holds, such as `1e400` — is
`amount_overflow` however it was written, and any other non-integer is
`canonical_json_float`. `-0` is the integer 0: a reader whose parser keeps it
as a negative float reads it as zero, as every other reader does.

**Within range, a number written with a fraction or an exponent is not an
integer**, whatever its value: `9000.0` and `9e3` are `canonical_json_float`,
because §9.3's encoding writes an integer only as digits and a document holding
either was not built by a conforming writer. A reader MUST decide this from the
number as written — a parser that turns `9000.0` into the double 9000 before
anything looks at it has lost the distinction and applies an amount every other
reader refuses. This holds for the bill document as for an entry: one decoder
refusing a float with `bill_type_error` and another with `canonical_json_float`
names one fault two ways.

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

A `createBill` entry MAY carry, and a writer SHOULD:

- **`keyDigest`** — SHA-256 of the ASCII bytes `splitz-bill-key-v1` followed
  by the 32 bytes of the bill key (§11.1), unpadded base64url. Anything else in
  the member is `bill_type_error`.

**It binds the bill's key to the bill.** The id is the digest of this entry, so
an invite naming the bill names the key it was made with. Nothing else ties an
invite's key to the bill it names: somebody handing over a real bill's id with
a key of their own opens, on the joiner's device, whatever they seal under that
key — the bill as it is, plus an expense only the joiner sees — and everything
the joiner writes afterwards reaches nobody else. **A reader holding a key for a
bill whose create states `keyDigest` MUST refuse the key when the digest is not
its own** (`invite_key_mismatch`): a scanned bill code carrying such a key is
refused, and a log opened under such a key is not merged, nor is a held log
sealed under it. The key is then discarded while it is still the one held: it
opens only what its maker sealed for this device, and kept, it would refuse
the bill's real invite as a conflict. A create stating no digest commits to no key, and a reader can
check nothing.

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
`recordPayment`, `confirmPayment`, `setRate`, `closeBill`.

Each carries `id`, `author`, `kind`, `at`, and only the payload its kind uses.
An unrecognised kind is refused with `bill_unknown_entry_kind`.

**An entry MUST carry the payload its kind uses and no other.** One carrying
more than one of `rate`, `expense`, `payment`, `confirmation` and `close` is refused
with `bill_ambiguous_entry`, because the currency fallback of §9.1 and the fold
of §10.3 would otherwise read different ones.

One carrying none is refused with `bill_missing_entry_payload`, and MUST be
refused before it reaches a log: `joinBill` needs `participant`, `addExpense`
`expense`, `recordPayment` `payment`, `confirmPayment` `confirmation`, `setRate`
`rate`, `closeBill` `close`, and `voidEntry` and `amendEntry` a `targetId`.

**Admitting one is not the harmless no-op it appears to be.** Removing a member
makes an entry's canonical encoding sort *higher* than the same entry with it:
at the removed key's position the shorter form holds the next key, or `}`, both
of which are greater. §10.2 therefore keeps the stripped copy. Anyone holding
the invite can re-push a copy of an entry with one member removed and take the
expense, payment or withdrawal it carried off the bill, on every device, with
nothing set aside to show for it.

**An entry's `v`, when present, MUST be an integer from 1 to 2^63 − 1**,
refused with `bill_type_error`. §9.5 leaves `v` out of the id, so a copy carrying any
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

**The creator's latest live `setRate` decides**, by §10.2's order — `at`, then
`author`, then `id`, then canonical encoding — so the answer is a function of
the log and not of which device last spoke. **Only while the creator has set
none does the latest by anybody else decide.** `at` is whatever its author
wrote, so "the latest by anybody" hands the rate to whoever dates furthest
ahead, and every correction then loses to it; the creator is the one
participant every reader can verify (§10.7), and a creator's rate is replaced
by the creator's next one. A `setRate` may be amended and withdrawn like
any other entry; when none survives, the bill has no rate and §7's conversions
are unavailable rather than guessed at. While the creator has set none, the creator's host SHOULD
set one from a price it can read (`creatorRateMissing` /
`creator_rate_missing`), asking again just before writing and writing nothing
when the creator priced the bill by hand meanwhile: until then a rate somebody
else dated far ahead stands against every correction.

**A `setRate` MUST be in the bill's currency**, and one that is not is set
aside with `rate_currency_mismatch`. Every amount on the bill is in that
currency, so a rate in another refuses every request priced against it, and
one dated ahead would hold that refusal over every later correction.

**An `addExpense` MUST be authored by somebody who joined** — a participant
on the bill, or one whose `joinBill` was withdrawn (§10.8), whose earlier
entries still stand — and one that is not is set aside with
`unknown_participant`, after the check that its `paidBy` is on the bill. An
expense is somebody's word about who paid and who shared; one written by
anybody else holding the invite puts a debt on the bill under a name nobody
on it has seen, and reopens a closed bill (§10.9).

**A `setRate` MUST be authored by a participant on the bill**, and one that is
not is set aside with `unknown_participant`. The rate decides how much ZEC
every request carries, so a rate written by somebody who owes nothing and is
owed nothing turns every payer's request into whatever figure they chose.
**A fold given a verifier MUST also require that participant's key to be
bound (§10.7)**, and sets aside a `setRate` from any other with
`unauthorized_entry`: anybody holding the invite can put themselves on the
bill with an unsigned join, so "a participant" alone admits them. A wallet
MUST show a payer the rate a request was priced at (§14.2), and the bill's
creator may withdraw any `setRate` (§10.8).

**`sig`** carries the author's signature when the transport provides one.
When present it MUST be a string, and an entry whose `sig` is anything else is
refused with `bill_type_error`. An unsigned entry is admitted at ingress: a
bill among people at one table is consensus by agreement, not by
cryptography, and a reader that verifies nothing folds every entry as its
author wrote it, which §10.4 says the cost of.

A host that does verify signatures MUST do so over the bytes §10.6 fixes, and
MUST verify a `createBill` entry's `sig` against the `creatorKey` in that same
entry, refusing the entry when it does not verify. That key is not taken on
trust from elsewhere: §9.4 binds it to the id, so it is the one key on a bill
that needs no prior acquaintance to check. Every other entry is checked
against the key §10.7 binds to its author: an entry by a participant whose key
is bound applies only from a copy that verifies against it, and an unsigned
copy never speaks for them. An entry by a participant whose key is not bound
is admitted as written.

A `createBill` entry additionally carries `creatorKey` and `nonce`, and its `id`
MUST be their derivation (§9.4).

**The checks of this section run in this order**, so that an entry wrong in
two ways is refused for the same reason by every reader: the entry is an
object; it nests no deeper than 62 levels (§11.2); its kind is one of the nine;
its `sig`, when present, is a string; its `v`, when present, is in range;
every string is Unicode scalar values (§2.3); every number is an integer a
signed 64-bit value holds (§9.3); it carries at most one of `rate`, `expense`,
`payment`, `confirmation` and `close`; every payload member it carries is an
object;
`targetId`, when present, is a string; `basis`, when present, is a string;
the payload its kind uses is present;
every id inside that payload is a string; a `voidEntry` or `amendEntry` names
a non-empty target; `at`, `id` and `author` are well formed; and last, the
`createBill` members and the id's derivation (§9.4, §9.5) — a create's `name`
is a string, its `currency` is one (§2.1), its `splitMode` is a string and one
§4 defines, then `creatorKey` and `nonce` bind it (`create_unbound`), then a
stated `keyDigest` is 32 bytes as unpadded base64url (`bill_type_error`), then
its id derives.

**An `addExpense` may carry `targetId` and `basis`**, and is then a
restatement of the expense at `targetId` (§10.8). Any other kind carrying
`basis` ignores it.

**An entry naming a target the log does not hold is set aside with
`unknown_entry`.** This applies to `amendEntry`, `voidEntry` and a
restatement alike: a
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

**The total order is the instant `at` names, then `at` as written, then
`author`, then `id`, all ascending.** It never depends on arrival order or on
local state.

By the instant, normalised as §9.3 normalises one, rather than by the text:
§9.3 admits `t` and `z` and any number of fractional digits, and as bytes a
lower-case `t` sorts after every upper-case one, so the text of an instant
can sort after a later one. A `setRate` written with a lower-case `t` would
then outrank a correction made hours later. The text breaks ties between two
spellings of one instant.

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
   the set of voided targets. Each entry below is applied as amended; **an
   amendment that cannot be applied is set aside with the code that refuses
   it, and the entry it corrects is applied as written** (§10.4).

4. Apply every non-voided `joinBill`, replaced by its amendment if any, subject
   to §10.4. **A participant who rejoins replaces their earlier record** rather
   than being refused as a duplicate, and a replaced destination — the `payTo`,
   or the `address` of the first payout when the record declares any — MUST be
   recorded and reported to the caller.

5. Apply every non-voided `addExpense` and `recordPayment`, replaced by their
   amendments if any. **An expense id MUST be unique among the expenses
   applied** (`duplicate_expense`), and a payment id among the payments
   (`duplicate_payment`): an amendment or a withdrawal is written against the
   expense a reader shows, a confirmation names one record, and two under one
   id leave the reader to guess which.

   **Every expense and payment id is minted by the entry's author.** An id
   is *minted by* an author when it is that author's participant id, `:`, and
   anything after, and **an author whose participant id contains `:` mints
   nothing**: the id `<payer>:<txid>` would otherwise mint
   `<payer>:<txid>:<recipient>`, the id of the payer's own send record. An
   `addExpense` or `recordPayment` whose payload id its author did not mint is
   set aside with `id_not_minted`, and so is an amendment whose payload id its
   author did not mint (§10.4: the entry it corrects then applies as written).
   The check runs once the payload decodes and before the uniqueness check
   above; for a payment, after `unauthorized_payment`, `unknown_participant`,
   `self_payment` and the currency checks.

   Exactly one author mints a given id, so two entries under one id are by one
   author, and among them the first by §10.2 stands. §10.2's order is each
   author's to write, so letting it decide between authors hands an id to
   whoever backdates furthest: a copy of somebody's expense dated a minute
   earlier would replace it, and a copy of a payer's record written by the
   payee would set the payer's aside and ask them to pay again. A builder
   therefore writes every expense and payment id as `<author>:<local>`
   (`authoredId`), and a payment for one recipient of a send as
   `<payer>:<txid>:<recipient>`.

   **What one author has recorded one participant paying another stays in the
   64-bit range** (`amount_overflow`), summed per author: §14.4 sums a payer's
   own records, and a total shared by both parties lets a record the payee
   wrote carry the payer's out of range.

6. Apply every non-voided `confirmPayment` (§10.5), in a pass of its own once
   every payment is on the bill.

**The fold reaches these answers beyond the bill itself, and an implementation
MUST report them rather than discard them.** A consumer that re-derives one has
a second place for it to come from:

- the **creator's id**, which the withdrawal rules of §10.8 name;
- the **identities bound** under §10.7, which say whose entries are checked
  against a key;
- the ids of the **entries a void withdrew**, because a withdrawal is absent
  from the fold by design and is otherwise indistinguishable from an entry that
  was never written;
- the **entries set aside**, each with its code and the reason;
- for each expense and payment on the bill, **the entry that introduced it
  and its author**, and for the rate, **the `setRate` that set it and its
  author**. An amendment or a withdrawal targets that entry, §10.8 decides
  who may write one by that author, and §14.2 names who set the rate. A
  reader taking these from the log instead finds entries the fold set aside
  under the same ids.

**The fold takes the host's verifier, and binds nothing without one.** §13
makes the curve operation the host's, so a fold given no verifier cannot bind
a key and reports none bound — which is the honest answer, not a claim that
none exists. A caller that hands one in gets `bound` computed over the same
entry set the bill was materialised from, which is what §10.7's rule depends
on.

**A verifier also decides which entries are applied at all.** §10.1 requires a
host that verifies to check a `createBill` entry's `sig` against the
`creatorKey` in that same entry. When a verifier is supplied, a `createBill`
whose signature does not verify is set aside with `unauthorized_entry` and
opens no bill, an entry by a bound participant applies only from a copy that
verifies (§10.7), and a `setRate` applies only from a bound participant
(§10.1). Unsigned entries are still admitted at ingress — §10.1 says so and
says why — and a fold with no verifier binds no key: it applies every entry
as its author wrote it and takes a rate from any participant, which is what
a bill among people who checked nothing is.

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
  to correct. For the same reason a host MUST build a correction from what
  the bill applies now, after any amendment already standing, and not from
  the entry as first written, or a second correction undoes the first
  (`amendExpense` / `amend_expense`, `amend_expense_entry` in the binding).
- An `amendEntry` MUST keep the id its target is about — a join's
  `participant.id`, an expense's `id`, a payment's `id`, a confirmation's
  `paymentId` — and is otherwise set aside with `amend_kind_mismatch`. A
  correction that renames its subject is a different entry: a join renamed
  out from under an expense takes its author off the bill and the debt with
  them, and the §10.8 checks that read the target never see it.
- **An amendment that cannot be applied is set aside, and its target applies
  as written.** Whatever refuses the amended version — a member the decoder
  refuses, a split §4 refuses, a balance §2.2 refuses — refuses the
  amendment, not the entry it corrects. Setting the target aside instead lets
  a participant correct their own join into a record nobody can decode and
  leave the bill, with every expense naming them, where §10.8 would have
  refused to take them off.

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
  "v": 1, "id": "c1", "author": "ana", "kind": "confirmPayment",
  "at": "2026-10-28T19:34:00.000Z",
  "confirmation": {
    "paymentId": "ben:p1", "method": "recipientConfirmed",
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

`sig` is optional (§10.1), and a host that verifies checks it against the key
§10.7 binds. An implementation that verifies, and a wallet that signs so
another wallet can, need the same answer to one question: **which bytes?**

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

A participant publishes their key in their own join, under the participant id
that key derives:

```json
{"id": "<participant id>", "name": "Ben", "payTo": "u1…", "identityKey": "<32 bytes, base64url>"}
```

```
participant id = base64url( SHA-256( "splitz-participant-v1" || key )[0..16] )
```

where `key` is the 32 bytes the `identityKey` decodes to and `base64url` is
unpadded. **A key names its participant.** A wallet whose account publishes a
key writes every entry under the id that key derives.

**Binding.** A key is bound to a participant by a **self-claim**: a `joinBill`
whose `author` equals the `id` of the participant it carries, whose
participant states an `identityKey` deriving that id, and whose `sig` verifies
against that key. An entry naming somebody else's id proves nothing about
them, whoever signed it.

**A record stating a key under any other id is set aside** with
`participant_id_not_derived`, whoever wrote it and whether or not a verifier is
supplied — it is a structural check of the record, not of a signature. The
creator's own record is the one exception, below.

**Why the id is the key's.** An id anybody may choose is one a second key may
claim. Were a participant's id free and their key merely stated beside it, a
second self-claim for the same id under another key would verify as well as
the first, and nothing in the log could say which is the person: `at` is
whatever its author wrote, so resolving by time hands the identity to whoever
backdates furthest, and admitting both lets the impostor write as them —
confirming a payment to them that never arrived, withdrawing their expenses.
With the id derived from the key, a second key derives a different id: it is
a different participant, and the first one's entries still verify only
against the first key. Finding a second key for a given id is a preimage of a
128-bit digest.

**The creator is bound by the invite, not by a join.** The bill's id is the
digest of the entry that opened it (§9.4), and that entry states `creatorKey`.
A reader MUST bind the creator's participant id to that key when the entry's
own `sig` verifies against it, whatever that id is, and MUST NOT bind any
other key to it: a join claiming the creator's id is refused under the rule
below. The derivation rule does not apply to the creator's record, whose key
the invite already fixes.

The signature requirement is not ornamental: absent it, `creatorKey` is a
number the bill's author typed, and anyone could open a bill naming somebody
else's public key and be taken for them on it.

**A withdrawal does not undo a claim.** Identity is resolved over every entry
in the set, including ones §10.8 withdrew. A signed self-claim is evidence that
was made; taking the entry off the bill does not unmake it. A reader MUST
resolve identity over the whole entry set and MUST NOT resolve it over the
live set §10.3 folds.

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

**An unbound identity is admitted unverified**, exactly as if this section did
not exist: a participant somebody else added, who has no key, or one who has
published none. §10.4's rules then bind the honest and inconvenience nobody
else, and a wallet that wants them to mean something has every participant
publish a key on joining.

**A device is on a bill as itself only once the fold binds its id.** A record
under a device's id is not enough: anybody holding the invite can write an
unsigned `joinBill` under an id they know — ids are the same on every bill,
so anybody who has shared one with the person knows it — carrying their own
payout, before the person joins. A host MUST NOT treat itself as joined while
the fold binds no key to its id, and writes its own signed join, which binds
its key and takes the record back: the planted copy, unsigned and authored as
a now-bound participant, is set aside. `joinedAsMe` / `joined_as_me` in both
host packages answer it, and `join_needed` in the binding says why a
device must still join: `not_joined` or `not_bound`.

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
| `setRate` | its author, **or the bill's creator** |
| `createBill`, `confirmPayment`, `amendEntry`, `voidEntry` | its author |

Anything else is set aside with `unauthorized_entry`.

**Why the creator, for an expense.** An expense is entered by hand and
duplicated by accident constantly, and the person who entered it may be asleep.
The creator is the one role every reader can verify without acquaintance: the
bill's id is the digest of the entry that states their key (§9.4, §10.7). A
withdrawal is visible in the log and the expense is re-addable, so the cost of
a wrong one is low — which is not true of the next two.

**Why the creator, for a rate.** The latest `setRate` by §10.2's order
decides, and `at` is whatever its author wrote: one dated a year ahead
outranks every correction until its author takes it back. The creator is the
one participant every reader can verify, and a rate taken back is visible and
set again in one entry.

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
refused with `participant_still_named` when any surviving entry that
participant did not write names them — as an expense's `paidBy` or in its
split, or as a payment's `from` or `to`. An amended entry names them if either
the amendment or the entry it corrects does: the amendment may yet be set
aside when it is applied (§10.4), and the entry then applies as written.

An entry the participant wrote themselves never holds back their removal.
Counting it would let them undo it with one entry naming themselves, written
after it: their join comes back with its old payout and a debt owed to them,
and no join is written that anybody sees. Their own entries then name
somebody not on the bill and are set aside. Coming back is a new `joinBill`.
**A restated expense stays its author's to correct.** A restatement is
written by the creator, or by the expense's author, taking somebody off; an
`amendEntry` of it MAY also be authored by whoever wrote the expense it
restates — following a chain of restatements to the first — and §10.4
otherwise holds. Withdrawing the restatement stays its author's and the
creator's alone: withdrawn, it puts back the expense it replaced, which still
names whoever was taken off. The expense's author takes the expense off by
withdrawing its first entry, which leaves the restatement stale and the
removal standing, and so does the creator, who may withdraw any expense
(`expenseWithdrawalTarget` / `expense_withdrawal_target` name the entry to
withdraw, and `expenseCorrectors` / `expense_correctors` who may correct it). A host MUST NOT write an `amendEntry` of an entry the
log holds but does not have in force — a withdrawn entry, or one a
restatement replaced — and refuses with `unauthorized_entry`: the fold admits
it, the restatement goes stale, and the person taken off is back (`refusalOf`
/ `refusal_of`).

A removal plan (below) still counts the person's own entries, so an honest
removal sets aside no record they wrote of money they paid or received; the
fold's rule is what stands against one written after it. A removal written
past the plan sets the person's own entries aside, each reported with its
code: no more than their author or the creator could already do by
withdrawing them (§10.8's table).

The fold cannot apply an entry naming somebody who is not on the bill, so
without this rule removing somebody silently drops every expense another
participant entered naming them, as payer or in the split. The check runs after every other
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
nothing. `BillLog.refusalOf` / `refusal_of` in both host packages, and
`entry_refusal` in the binding, answer it for any entry by folding the log
with the entry in it, so the answer is the fold's own: the §12 code it would
be set aside with, or nothing. `unknown_participant`, `unknown_entry` and
`unknown_payment` wait on an entry the device may not hold yet and apply once
it arrives (`codesAnEntryOutgrows` / `CODES_AN_ENTRY_OUTGROWS`); a host writes
nothing on any other.

A withdrawal the fold refuses is otherwise still written, still synced, and
looks to its author exactly like one that worked — the entry is in the log and
the thing it meant to remove is still on the bill.

**Asking before taking somebody off.** For a `voidEntry` of a `joinBill` the
question is what still names the participant, and what of it the author may
change first. An implementation SHOULD answer it before writing that
withdrawal. `planRemoval` / `plan_removal` in both host packages, and
`plan_removal` in the binding, answer it from the log, the bill it folds to,
the creator, the author and the participant. They read the log as the rule
above reads it, and from the fold's own answer to it: the entries the fold
holds in force (`FoldedBill.inForce` / `in_force`) — admitted at ingress, not
withdrawn, not replaced and not a restatement that does not apply, whether
the fold went on to apply each one or set it aside — so an entry a verifying
reader refused, such as one signed by a key other than its author's, names
nobody; an amended entry through both the entry and the amendment the fold
chose for it (`amendmentOf` / `amendment_of`); and one reading per entry
id, since §10.2's union keeps copies of one id under different signatures. They return, in log order:

- **the expenses the author can write again without them** — applied by the
  fold, not paid for by them, written by the author or on a bill the author
  created, and whose split still divides the amount once they are taken out
  (`splitWithout` / `split_without`): an `equal` or `shares` split shared
  among the rest, an `itemized` split with them taken off each item, and
  every member the split's `type` does not read losing them too, since the
  rule above reads every member whatever the type. An
  expense named only by the entry an amendment corrects is written again as
  it reads now;
- **every other entry that names them**, and why: an expense the fold does
  not apply, one they paid for, one written by somebody else on a bill the
  author did not create (with its author), one whose split needs a person's
  choice — `exact` and `percentage` figures that must still add up, an item
  only they shared, shares that leave nobody a share — a payment from or to
  them, and a confirmation they wrote;
- **every `joinBill` not withdrawn that states them** (`joins`): each change
  to how somebody is paid restates their record in another join, and they
  stay on the bill while any one of them stands, so taking them off withdraws
  every one.

Each expense is written again as a restatement (below), never as an
amendment: the rule above counts an amended entry as naming them while the
entry it corrects does. The answer goes stale as soon as another entry
arrives, so a host SHOULD plan again immediately before writing and write
nothing when the plan differs from the one the person agreed to
(`RemovalPlan.sameAs` / `same_as`, `same_removal_plan` in the binding).

**Whole or not at all.** A host SHOULD write the restated expenses only when
nothing else names the participant and it may withdraw their joins — the
bill's creator or the participant themselves (`mayWithdrawJoins` /
`may_withdraw_joins`); `RemovalPlan.complete` / `complete()`, and
`complete` on the binding's plan, hold both — and then withdraw every join
with them, **in one write**: written apart, a sync between them leaves the
expenses restated and the participant on the bill.
`removalEntries` / `removal_entries` in both host packages, and
`removal_entries` in the binding, build exactly those entries for a complete
plan and refuse any other — `unauthorized_entry` when this device may not
withdraw the joins, `participant_still_named` when something else names
them. Written alone they leave the participant on the bill, still owed what
they paid and sharing in nothing else; taking somebody out of one expense is
an edit of that expense. What writing them moves is
`RemovalPlan.shareChanges` / `share_changes` (`removal_share_changes` in the
binding): for each participant, how much more they owe in the bill's minor
units, every restated expense split by §4 before and after. The participant
taken out has minus their share, the figures sum to zero, an unchanged
participant is left out, and a running total past §2.2's bounds is refused
with `amount_overflow`.

**Restating an expense.** An `addExpense` carrying `targetId` restates the
expense that entry added: it replaces it, so the target is not applied, and
the restatement applies in its place. `basis` names the `amendEntry` the
fold applied to the target when the restatement was written, and is absent
when none was. A restatement applies only when, after every withdrawal is
resolved:

- its target is an `addExpense` (`amend_kind_mismatch` otherwise) the log
  holds (`unknown_entry`);
- its author may withdraw the target by the table above — the target's
  author or the bill's creator (`unauthorized_entry`);
- the target is on the bill — not withdrawn, and, when the target is itself a
  restatement, the one that applies to its own target — and the amendment the
  fold applies to the target is the one `basis` names, or none when `basis`
  is absent (`restatement_stale`);
- and no earlier restatement of the same target, by §10.2's order, also
  passes these checks (`restatement_superseded`).

A restatement that does not apply is set aside with its code, leaves its
target as it stood, and names nobody for the still-named check. These checks
choose the one restatement that replaces its target **before** its own expense
is applied: one that passes them replaces the target and supersedes every
later one even when its expense is then set aside — a split §4 refuses, a
`paidBy` not on the bill — and the expense is then off the bill until it is
written again. A host MUST NOT write a restatement its own trial fold sets
aside (§10.8, "Asking before writing"), so an honest removal never does this. A withdrawn
restatement does not apply, so withdrawing one puts its target back, or lets
the next restatement of it apply. A restatement's target can be a
restatement; ids are digests of their entries, so the chain ends.

**Why one, and why `basis`.** A restatement written as a new `addExpense`
beside a withdrawal of the old one is two entries a second device can write
again: the creator on a phone and a tablet, or the creator and the expense's
author, each taking one person off at once, leave two expenses where there was
one — every one of them is applied, and every remaining participant owes the
expense twice. Naming the target lets the fold apply one. A restatement also
copies what its writer read, so one written while the author corrected the
expense would put the uncorrected figure back and discard the correction with
nothing set aside to show for it; `basis` makes that restatement stale
instead, the correction stands, and the participant stays on the bill until
the removal is planned again from the corrected figure.

**What removal does not do.** Taking somebody off the bill withdraws their
joins; it does not change the bill's key (§11.1). They still hold the key, so
they can read every entry written afterwards, and they can write a new
`joinBill` that puts them back. A wallet that offers removal SHOULD say both,
and SHOULD show everybody on the bill a join from somebody whose earlier join
was withdrawn.

**What all of this rests on.** These rules name participant **ids**, and an id
is unauthenticated until §10.7 binds a key to it. For a participant who has
published no key, an entry authored as them is admitted unverified, so the
rules above bind the honest and inconvenience nobody else. The creator is
bound by the invite, and every participant who joins with a key is bound by
the id that key derives. A wallet that wants these rules to mean something
must have every participant publish a key on joining.

### 10.9 Closing a bill for settling

A bill is paid once, after everything on it is known. The creator says when
that is by writing a `closeBill`:

```json
{"kind": "closeBill", "close": {"covers": "<digest>"}}
```

```
covers = base64url( SHA-256( "splitz-close-v2" || canonical({"balances": B}) )[0..16] )
```

where `B` maps each participant to what the bill's expenses alone leave them
— the amounts they paid less their shares under §4, summed over every
expense §10.3 applies, in minor units as a JSON integer — and holds only the
participants whose figure is not zero. Payments and confirmations play no
part. `canonical` is §9's encoding, which orders `B`'s members. The fold
reports this digest as what a close written now covers (`closedOver` /
`closed_over`), and reports the bill **closed**, naming the close
(`closeEntry`), while a close in force:

- was written by the creator — any other author's is set aside with
  `unauthorized_entry`, and so is the creator's when a verifying fold has not
  bound the creator's key;
- carries `covers` as 16 bytes in §9.4's canonical unpadded base64url — a
  padded, non-canonical or other-length value is set aside with
  `bill_type_error`;
- covers what the expenses now leave everybody owing;
- and is the creator's latest close by §10.2's order, counting one withdrawn.

A withdrawn close is a reopen: it stays the latest, so the bill reads open
even when its expenses return to what an earlier close covered, and two
closes over one set of expenses take one reopen. A withdrawn close reopens
whether or not it was well formed: withdrawing it is the creator saying the
bill is open. A close set aside and not withdrawn decides nothing. A close is
amended as any entry is (§10.4: by its author, the creator), and the
amendment's `covers` is what it covers. A later close over fewer expenses than the bill now holds leaves it
open, whatever an earlier close covered.

**A close is not dated against the expenses.** Only one close against another is put in §10.2's order. Any expense added, corrected or withdrawn
after the creator closed the bill that changes what somebody owes gives
another digest, and the bill is open again, whatever `at` that expense
carries. Deciding by time would let a clock set back keep a bill closed over
an expense it never saw. An expense that moves nobody's figure — one paid by
and shared by the same person, say — leaves it closed: what the creator
closed is what everybody owes, and that has not changed. A close is withdrawn
by the creator alone (§10.8's default: its author), which reopens the bill; a
fresh close over the expenses as they stand closes it again.

**A close or reopen is dated after the last one.** The latest close decides,
so a creator writing from a second device whose clock trails the reopen it
read would write a close that sorts before that reopen and never closes,
however often it is written. A host MUST date a close, and the withdrawal of
a close, no earlier than one millisecond after the latest `at` (§9.3) of any
close in the log or any withdrawal of one, when that is later than its own
clock. `closeFor` / `close_for` and `reopenFor` / `reopen_for` do, from
`FoldedBill.lastCloseAt` / `last_close_at`.

Payments and confirmations do not change what a close covers. A payment is
admitted to an open bill exactly as to a closed one: one somebody already made
is a fact, and §14.9 is what keeps a host from starting one.

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
missing or empty `k`, or one that is not the 32 bytes of a bill key (§11.3) as
unpadded base64url, is `invite_missing_key`; an unparseable `x` is
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
`isInviteExpired` / `is_invite_expired` answers the hint for a host that
shows it: an invite is expired when it carries `x` and the host's clock, in
whole Unix seconds, is past it. Seconds, because `x` may be any of nineteen
digits and scaling it to milliseconds overflows.

**An invite may travel as an https link.** `https://<rest>#<invite URI>`: the
invite URI above, whole, as the fragment of a link whose `<rest>` the sharer
chooses. A browser never sends a fragment to the link's host, so the key
reaches nobody but whoever holds the link, and a chat app shows an https link
as one a person can tap. After scan padding is stripped, a reader that meets
`https://` — matched case-sensitively — takes everything after the first `#`
and reads it under every rule above; a link with no `#` is
`invite_not_an_invite`, and padding inside the fragment is content. An encoder
refuses a `<rest>` that is empty, begins with `/`, or holds anything but
printable ASCII (`!` to `~`) or a `#`, with `invite_bad_link`.

### 11.2 Scanned payloads

`splitz1:<base64url>` carries an invite together with the log, so a joiner who
scans it holds a bill rather than an id. `splitzd1:<base64url>` carries a
joiner's answer: the entries the inviter has not seen. Two scans, no network.

**A payload is capped at 2322 characters of encoded body in both
directions** — the base64url after the prefix, not the whole scanned string.
The prefix is fixed and known, so measuring it would make the two prefixes
carry different amounts; the cap leaves room for the longer of them instead. Refusing to encode
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
**§10.1 bounds a single entry at 62**, with `bill_type_error`: the two levels
a payload wraps an entry in, the body and its `log`, put an entry of 62 at
64, and one admitted any deeper would make every payload carrying the bill
unreadable for good. The bound is applied at §10.1 because an entry arriving
over a relay (§11.3) never passes through this section at all — and every pass that touches it walks it, deriving its id by
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
M, and the scanned string is the prefix and the body together: 2322 is that
less the 9 characters of `splitzd1:`, so a body at the cap renders at level M
behind either prefix. A body of 2331 behind `splitz1:` is 2339 characters,
which no version-40 code at M holds. **The cap is that ceiling rather than a
fraction of it, because the format starts near it.** Every entry carries an 86-character signature; the
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
| two participants with payout addresses, one expense | `two_payable_participants_and_one_expense` | **2111** |
| three participants with payout addresses, no expenses | `three_payable_participants_and_no_expenses` | **2188** |
| three participants with payout addresses, one expense | `three_payable_participants_and_one_expense` | refused |

The headroom on the third row is 211 characters, so the free text on an
expense is part of the budget: the same bill with a 165-character description
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
does not decode is `payload_damaged`; one that decodes to anything but RFC 8259
JSON — a `NaN` or `Infinity` literal, a number no double holds such as
`1e400`, a string carrying a lone surrogate — is `payload_damaged` too,
because a reader that took `1e400` as infinity and one that refused it hold
different logs from one scan; one that decodes to an object carrying no
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
`weight_sum_overflow`, `negative_share`, `amount_overflow`, `amount_too_large`,
`exact_limit_too_large`, `unauthorized_entry`, `amend_kind_mismatch`,
`bill_ambiguous_entry`, `bill_missing_entry_payload`, `empty_split`,
`exact_total_mismatch`, `percentage_not_full_scale`, `itemized_no_items`,
`itemized_unassigned_item`, `itemized_total_mismatch`, `currency_mismatch`,
`unknown_participant`, `unknown_entry`, `duplicate_participant`,
`duplicate_payment`, `duplicate_expense`, `participant_id_not_derived`,
`self_payment`, `bill_bad_participant_id`,
`unknown_payment`, `unauthorized_confirmation`,
`confirmation_missing_reference`, `unauthorized_payment`, `id_not_minted`,
`participant_still_named`, `restatement_stale`, `restatement_superseded`,
`ambiguous_create`,
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
`invite_bad_expiry`, `invite_bad_link`, `invite_key_mismatch`,
`payload_not_a_payload`, `payload_damaged`,
`payload_missing_body`, `payload_future_version`, `payload_too_large`,
`sealed_malformed`, `sealed_future_version`, `zip321_no_payments`,
`zip321_too_many_payments`, `zip321_amount_not_positive`,
`zip321_amount_too_large`, `zip321_memo_too_large`, `zip321_bad_address`,
`zip321_bad_currency_code`, `zip321_fiat_not_positive`,
`zip321_fiat_too_many_digits`, `zip321_no_address`,
`zip321_not_canonical`, `zip321_memo_undeliverable`, `address_invalid`,
`payout_not_declared`, `obligation_mixed_payers`, `payment_not_positive`,
`payout_incomplete`, `swap_missing_reference`, `bill_not_closed`,
`bill_closed`.

**The code is part of the protocol; the message that accompanies it is prose
and is not.** A user-facing string MUST be derived from the code.

Every refusal a host function returns for a person to read carries one of
these codes. The failures that carry none are not refusals of what a person
asked for, and a host says them in its own words: a transport or a provider
that did not answer as it should — the relay, a price source, a swap
provider (`SplitsRelayException`, `SplitsSyncException`, `ZecPriceException`,
`SwapException` in the Dart host; `Relay`, `Sync`, `Price`, `Swap` in the
Rust host's `HostError`); storage that would not read or write
(`BillStorageUnreadable`; `Storage`, `Unreadable`); a send still under way
(`SendInFlight`, `Unrecordable`; `SendInFlight`); a held key the bill does
not match (`BillKeyConflict`; `KeyConflict`, `ForeignKey`); and a caller's
own error, an argument no honest flow passes (`ArgumentError`; `Malformed`);
a blob this device holds that will not seal or open, before any peer's
content is read (`SealingException`; `Sealing`); and a fault in the host
itself (`UnansweredSignatureQuestion`, and `SeedDriverException` in
development builds). `SwapRefused` / `SwapRefused` carries a typed refusal,
which a host words.

`vectors/messages.json` gives one plain-language sentence per code, which a
host MAY show as it stands. Both implementations return it from
`describeCode` / `describe_code`, and answer empty for a code they do not
define rather than a generic sentence: a code from a newer version is itself
something to tell a person.

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
- **Which network a wallet transacts on.** §8.6 answers the network an address
  belongs to; comparing it with the network the wallet is on is the wallet's,
  and so is decoding a receiver's bytes as a key when it builds the
  transaction.
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
  reported under §8.5 and MUST NOT be dropped from a request.
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

- Every recipient the request cannot carry, with the reason for each (§8.5).
- Every pay-to address the fold recorded as replaced (§10.3).
- Every debt with a payment recorded and not yet confirmed (§10.5).
- Every recipient paid by a preference other than their first (§14.8).
- Every recipient paid more than the debts the bill records explain, and that
  it is so (§6): a refund somebody else attributed to the payer is the one
  thing coverage cannot account for.
- The rate the request was priced at, and the ZEC amount and address of
  every output. A participant owed money can set the rate, and a
  request stated only in the bill's currency hides what that rate did to the
  ZEC it asks for.

A host SHOULD also compare the rate with a live price where it can read one,
and warn when they are 5% or more apart: the difference as a percentage of
the live price, truncated toward zero, 5 or more either way — so a rate of
1050 against a live 1000 warns (5) and 1000 against a live 1050 does not (−4)
(`ratePercentOff` / `rate_percent_off`,
`rateFarFromLive` / `rate_far_from_live`), saying so when it has no live price
to compare; and SHOULD warn a payer when whoever set the rate is somebody the
request pays. Both are figures a person with a stake in them chose.

A payee MUST be shown the same before confirming a payment: the ZEC the
record says was sent, the rate it was priced at, and its reference. A
confirmation settles the debt in the bill's currency, so a payee who confirms
a record without reading its ZEC accepts whatever the payer's rate made of it.

`checkPayerReview` / `check_payer_review` in both host packages, and in the
binding, runs the payer's list against the text a review screen shows. It is
given the obligation about to be sent, the folded bill and the strings the
screen displays, and answers each fact above that the text does not contain:
every unpayable recipient's name and the wallet's words for its reason, the
name of every participant whose address was replaced together with the
wallet's words that it was (given under `replaced_address`, and found once
for each such participant: a name is on every review that pays its holder
and says nothing about a change), every participant a
pending payment is owed to or went to, every recipient the payer chose to pay
by a lower preference with the wallet's words for it, every recipient paid
more than the bill's debts explain with the wallet's words for that, the rate
figure, and each output's ZEC amount and address.

**An address or a reference counts as shown** when the text holds it whole,
or holds its first 10 characters and then stops agreeing with it on a
character that is not an ASCII letter or digit — an ellipsis, a space, the end
of the line. A character is a Unicode scalar value (§2.3): counting UTF-8
bytes or UTF-16 units gives two answers for one screen.

`checkPayeeReview` / `check_payee_review` runs the payee's list against the
text of the screen a payment is confirmed on. Each of the record's ZEC, rate
and reference that it carries must be shown, written and matched as above. A
`shieldedZec` or `swap` record that lacks one must say so in the wallet's own
words for a missing figure. A `cash` record carries none of them and needs
nothing shown.

**What was shown is what is sent.** Immediately before the wallet is called,
a host MUST work the request out again from the bill as it holds it then, with
the payout choices the payer made, and MUST NOT send when it differs from the
request the payer reviewed (`requestStands` / `request_stands`; through the
binding, `obligation_via` again and its `uri` compared). An entry merged in
between — an expense, a rate, a replaced address — changes what is owed or
where it goes, and the payer would send a request they never saw.

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

A host therefore writes a send down **before** calling its wallet, in storage
that outlives the process, and refuses every further send from that bill while
the note stands — including a second send started in the same process before
the first note is written. A note that is there and will not read blocks
exactly as a readable one does. When the wallet answers, the note goes if the
send was refused, or if it reached the network and its records are on the
bill; otherwise it stays, carrying the transaction id when the wallet named
one, until a person says which way the send went. `PendingSends` implements
this in both host packages.

**A person's word that nothing left the wallet is checked before the note
goes.** A host MUST NOT remove a note on that word while the wallet is still
sending any transaction, or while it holds a transaction it built itself at or
after the note's `at`, compared to the second, that may be this send: one
that sent out of the account — the balance it took less its fee — what the
note's request sends in all, or one, or a note, that does not say. A send
killed after its broadcast and mined before the app came back is no longer
waiting, and its note may name no transaction: clearing it pays the debt
twice. A transaction that sent out something else is another payment, and
holding the note on it leaves the bill unpayable for good, with recording
that transaction as this payment the only way out. A note that
will not read names no instant and is held only by what is still sending.
`unsentClaimRefusal` implements the check in both host packages. A note that
names its transaction is decided by where the wallet's history shows that
transaction instead: the host MUST NOT remove it on that word while the
transaction is neither mined nor expired, since the wallet may still broadcast
it, or once it is mined, since it went through and is recorded rather than
cleared; expired, or absent from a history the wallet read, it can no
longer land. **A history that could not be read is neither**, and MUST NOT
be taken for "absent": the note stays (`unread`). Taking a failed read for
absent clears the note while the transaction may still go out, and the debt
is sent a second time (`namedSendRefusal` / `named_send_refusal`,
`pending_send_named_refusal` in the binding).

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
- A pending payment counts first against the payer's own settlement to the
  person it paid. Only what was paid **beyond** that settlement withholds
  another settlement covering that person; a payer with no settlement to
  them has all of it beyond. Paying several people exactly what their
  settlements ask therefore holds back no other debt — counting every
  payment against every settlement that covers its payee would leave a
  payer who owes three people unable to pay the third after paying two
  (`withholdings` in both packages; `vectors/withholdings.json`).
- What a request carries, plus every payment the payer recorded that is not
  confirmed, MUST NOT exceed the payer's debt in the plan. Netting can move
  a debt already paid onto a creditor no settlement's `covers` names, and
  the rules above then hold nothing back. A host takes the payer's
  settlements in the plan's order and carries each one only while the debt
  less everything pending and everything already carried covers it; one
  that does not fit is withheld whole, and names the recipients paid beyond
  their own settlement. Later expenses can leave more pending than is owed,
  and the request then carries nothing.
- What other payers have in flight to a payee counts too. A settlement MUST
  be withheld when the payments recorded to its payee by **other** payers —
  each record written by its own payer, none confirmed — together with what
  this request already carries to that payee, leave less than the
  settlement owing on what the plan still owes the payee in total. §6 plans
  from confirmed balances, so a confirmation can move a debt onto a payee
  another payer is already paying: asked for it, the payer pays them twice,
  and somebody else is left short until the payee passes it on. The held
  debt reports what others have in flight to the payee (`othersPaid` /
  `others_paid`), and the payee releases it by withdrawing a record of money
  that never came (§10.8).
- Where the amount pending is **less** than the debt, the **whole** debt is
  withheld rather than the remainder. Requesting the remainder overpays by the
  pending amount if that payment lands.
- What is owed and what is pending are reported as two quantities. On a part
  payment they differ, and presenting the debt as the amount in flight states
  something untrue. The pending amount is what is pending to the payee plus,
  for every other creditor the settlement covers, what was paid to them
  beyond their own settlement. For a settlement the bound alone withholds,
  it is what was paid beyond their own settlement to every creditor.
- Each withheld debt names the recipients its pending records were paid to.
  Under §6.3 that can be somebody other than the payee, and a payer told only
  "pending to Ana" looks for a payment to Ana that was sent to Ben.

A payment that never lands is withheld by the same rule, and §10.8's
withdrawal of its record is what releases the debt.

**A payer does not withdraw a payment its wallet shows on its way.** A host
MUST NOT withdraw its own record of a `shieldedZec` payment while its wallet
shows the transaction the record names mined or still sending: withdrawn, the
debt is offered again while the first payment has reached, or may yet reach,
the payee. Once that transaction has expired unmined, or when a history the
wallet read holds no such transaction, §10.8 alone decides; a history that
could not be read leaves the record held (`unread`), as for a note above.
`ownPaymentWithdrawalRefusal` implements the check in both host packages.

### 14.5 A peer who is current and a peer who is behind are different answers

§11.2 caps a payload, so the entries a peer has not seen may not fit in one.
A wallet computing them MUST distinguish three states: the peer holds
everything; the peer is missing entries that fit; the peer is missing entries
that do not. Reporting the third as the first tells somebody their bill is up
to date while entries on it have never reached them.

**A peer names what it holds by copy, not by id**: an entry's `id`, and `|`
and its `sig` when it carries one. §10.2's union keeps copies by id and
signature, so a peer holding only a copy whose signature fails holds the id
and still lacks the entry; asked by id, it is told nothing is missing and a
delta never repairs it. An id is §9.5's digest and holds no `|`.

### 14.6 What is signed is what was shown

A wallet reads a request with its own ZIP 321 reader, and a reader that keeps
only the first of several payments — or drops one it does not understand —
builds a transaction paying less than the payer was shown, with nothing on the
wallet's screen to say so.

Before signing, a host SHOULD hand the payments its wallet's reader produced,
without change, to `checkProposal` / `check_proposal` with the request it sent.
Each payment of the request (§8.7) is matched to one proposed payment with the
same address and the same zatoshi. Order is not significant, and one proposed
payment cannot answer for two. The answer lists the requested payments nothing
matched and the proposed payments that match nothing; a host MUST NOT sign
unless both are empty.

A reader refuses a request whole when it cannot read one of its addresses, and
§8.3's alphabet admits strings no reader decodes. A host whose wallet reads
fewer addresses than §8.3 admits SHOULD say which through `BillHost`'s
`readsAddress` / `reads_address`. The obligation then reports a recipient whose
address it refuses as unpayable with reason `bad_address` and carries the rest,
so one participant's unreadable address does not stop a payment to everybody
else. A host that says nothing is taken to read every address §8.3 admits.

### 14.7 A payment the payee's wallet has already seen

A payee's wallet that received the transaction a `shieldedZec` record names
holds the evidence §10.5 asks the payee for. A host MAY propose confirming it
with `walletReceived`, and computes the proposals with `arrivalsFor` /
`arrivals_for` from every bill it holds, its own participant id, and the
transactions it received, each as a transaction id and the zatoshi that
transaction paid this account.

- Only a transaction mined in a block and not expired is received. A host
  MUST NOT pass one still in the mempool, or one that expired unmined: it may
  never be mined, and confirming a debt against it settles the bill with
  money that never arrived.
- A transaction id is the hex of its id in the byte order a send reports it
  and a block explorer shows it, which is the reverse of the order its
  digest is computed in. A wallet that stores ids in digest order MUST reverse
  them first (`txidInSendOrder` / `txid_in_send_order`): a record carries the
  order a send reported, and an id in the other order matches nothing, with
  nothing to say so.
- A record is a candidate when it is to this participant, its method is
  `shieldedZec`, it is not confirmed, and its `reference` names a received
  transaction. Transaction ids are compared with ASCII space, tab, carriage
  return and line feed removed from both ends and ASCII letters lower-cased,
  and nothing wider: each language's own trim and lower-casing reach
  different Unicode characters.
- **A transaction named by records from more than one payer is evidence for
  none of them.** Every such record is reported as disputed and none is
  proposed. A payer is the key §10.7 bound to the record's `from` on its
  bill, or, where none is bound, that bill and that participant id together:
  an id is chosen by whoever joins, so one string on two bills can be two
  people, and an unbound one on two bills counts as two payers. A shielded
  transaction does not say who sent it, and any
  participant can copy a reference they have read off the bill: matching in
  any fixed order hands the payment to whoever sorts first, and the payee
  confirms a debt as settled by money somebody else sent.
- **A record's ZEC must pay for what it settles.** A candidate stating
  `zatoshi` is `underpriced`, and uses none of the transaction, unless its
  bill has a rate (§7) in the record's currency and
  `zatoshi × minorUnitsPerZec × 100 ≥ amount × 95 × 10^8`, compared exactly.
  A transaction bringing what a record states proves the ZEC arrived, not
  that it pays the debt: a payer who sends 1 zatoshi and records it as
  settling 100.00 EUR names a transaction that did arrive. The 5% leaves room
  for rounding and a rate set between pricing and sending.
- **A transaction's memos, where the wallet read them, must name the
  record's bill.** Each received transaction MAY carry the text memos it
  brought this account; a list, even an empty one, is what the wallet read,
  and no list is a wallet that cannot say. A candidate naming a transaction
  with a list that does not hold `splitz:` and the record's bill id (§8.5) is
  `unbound`, and uses none of the transaction: a payer may name one they sent
  the payee for something else. With no list, nothing is decided by memo.
- **A transaction's zatoshi is counted once, across every bill.** Records
  already confirmed that name it use their stated `zatoshi` first; the
  candidates then use theirs in order of bill id and payment id (§2.3). A
  candidate is `arrived` when what is left covers its `zatoshi`, `short` when
  it does not, and `unstated` when it states none. Only `arrived` may be
  proposed. Without this, one payment recorded on two bills is evidence for
  both.
- Amounts are held inside zero and 21000000 ZEC throughout: no transaction
  brings more than exists, and using up a share floors at zero.

§14.2 still applies: a proposal is shown to the payee — its ZEC, its rate and
its reference — before a confirmation is written. A host SHOULD also show
when the transaction arrived: a payment sent from a wallet that wrote no
memo, or one whose memos the host cannot read, is told apart from one made
before the debt existed by that alone. A host MAY write every
`arrived` confirmation on one acceptance, and SHOULD leave out of it every
payment a concern makes worth the payee's own look: the payer set the bill's
rate, the record is priced at a rate other than the bill's, or that rate is 5%
or more from a live price (`concernsBeforeConfirming` /
`concerns_before_confirming`).

**The payee does not withdraw what arrived.** A payee's host MUST NOT write a
`voidEntry` (§10.8) of a record its wallet proposes as `arrived`
(`Arrivals.covering` / `covering` in both host packages,
`arrived_withdrawal_refusal` in the binding): the transaction reached the payee carrying
what the record states, and withdrawing it asks the payer to pay the debt a
second time. A `short`, `unstated`, `disputed`, `underpriced` or `unbound`
record is not evidence, and stays the payee's to withdraw.

**Neither side withdraws a confirmed record.** A host MUST NOT write a
`voidEntry` of a payment record the bill holds confirmed (§10.5): the payee
has said it arrived, and withdrawn, the debt is asked for again. §10.8 would
admit it; a host refuses.

**Each of these is decided when the withdrawal is written.** A host decides
§14.4's, §14.7's and the confirmed rule against the bill as it folds it in
the same step that writes the `voidEntry` — not against the bill a screen
showed when it offered the withdrawal. A confirmation or an arrival that syncs
in while a person reads a dialog would otherwise be withdrawn on their tap,
and the debt asked for again.

### 14.8 Paying by a lower preference

A recipient's first payout decides how they are paid (§9.1). A payer MAY
settle a debt by any other payout the recipient declared, chosen for that
payment alone and for any reason: a swap to an asset their wallet cannot
reach, an address no request can carry, cash to somebody far away, or simply
preferring it. A wallet MAY make that choice itself when it cannot pay by the
first payout, taking the next one it can in the recipient's order; it MUST
show the payer which payout it passed over and why (§14.2). What it can pay
is the wallet's to say; the order is the protocol's: `payoutFallback` /
`payout_fallback` takes the wallet's reason for each declared payout, or none
where it can pay, and answers the next it can pay and why the first was passed
over.

A payer MUST NOT change how a debt is paid while a send that carries it is
unresolved (§14.3): it may still land, and a second way would pay it twice. A
note held for the bill is the test (`PendingSends.of`, `pending_send_blocks`
in the binding): while one stands, no record of paying another way is written
and no other lane is opened.

`choosePayouts` / `choose_payouts` takes a bill and the payer's choices,
each a participant id and the index of one of that participant's declared
payouts, and returns the bill with each chosen payout moved to the front of
that participant's list and the others in their declared order. A request
rendered from them (§8.5) carries a chosen `zec` payout's address; a chosen
`swap` or `cash` payout leaves the recipient reported as `payout_not_zec`, for
the wallet to settle in that lane. Choices are checked in §2.3's order of
their ids: an id not on the bill is refused with `unknown_participant`, an
index that is not one of that participant's declared payouts with
`payout_not_declared`.

The choice changes no entry. Every other device keeps reading the order the
recipient declared, and a payment made this way is recorded as any other.
A choice for somebody this payer does not currently owe is not a refusal:
it changes nothing the request carries.

### 14.9 Settling waits for the creator's close

A host MUST NOT start a payment on a bill that is not closed (§10.9): it MUST
NOT send a request, send a swap deposit, or write a record of a cash payment,
and refuses each with `bill_not_closed` (`settleRefusal` /
`settle_refusal`). A debt read off an open bill can still change, and money
sent against it cannot be taken back.

`swapSendRefusal`, `swapDeposit` and `combinedSend` read a bill, not a fold,
and do not know whether it is closed: a host MUST ask `settleRefusal` of the
folded bill before calling any of them, as it does before a request. The
binding's `swap_send_refusal` and `combined_send` fold the entries they are
handed and refuse an open bill with `bill_not_closed` themselves.

A host MUST NOT write an expense, a correction or a withdrawal of one on a
closed bill, and refuses with `bill_closed` (`expenseRefusal` /
`expense_refusal`). The fold would admit it and reopen the bill (§10.9); a
host refuses so that what is owed changes only when the creator reopens it.
Taking somebody off, or merging them (§14.11), restates expenses, so a plan
for either is refused with `bill_closed` while the bill is closed whenever it
would restate one and nothing else holds it back (`planRemoval` /
`plan_removal`, `planMerge` / `plan_merge`). Somebody on no expense still
comes off a closed bill; a plan a payment or another blocker holds back is
returned with that blocker, and writes nothing.

A host writes a close only for the bill's creator (`closeFor` / `close_for`),
over the digest its own fold reports (`closedOver` / `closed_over`), and a
reopening as the withdrawal of the close in force (`reopenFor` /
`reopen_for`). Both refuse anybody else with `unauthorized_entry`: every fold
sets their entry aside, and writing it would tell them the bill was closed
when it is not.

### 14.10 One transaction for a request and a swap

A payer who owes some people in ZEC and one person in another asset MAY pay
all of them in one transaction: the request's outputs, and the swap's deposit
as one more output (`combinedSend` / `combined_send`). One review shows every
output, ZEC and deposit alike, and one send settles them.

- The deposit is added only when it needs no memo — a request carries none to
  a deposit, and one sent without the memo its provider requires is lost — and
  never for a payee the request already pays.
- The swap leg is held to everything a deposit sent alone is (`swapSendRefusal`
  / `swap_send_refusal`): the quote is unexpired, the payee still asks to be
  paid that way, and the bill still owes that amount at the bill's rate.
- One §14.3 note covers the transaction. Its zatoshi are the request's outputs
  and the deposit together, which is what a person's word that nothing left is
  checked against. Once it lands, the request's payees are recorded under the
  transaction's id and the swap under its provider's reference, from the note
  alone if the app restarted in between (`PendingSends.recordsFor` records the
  request's half; the swap's record is written as for a deposit sent alone).

A payer whose swap cannot join the request — a memo, an expired quote, a
second payee in another asset — sends them one after another; §14.4 holds none
of them back on account of the others when each pays exactly its own
settlement.

### 14.11 Somebody added before they joined

A creator may put a name on a bill for somebody who has not joined yet: a
`joinBill` written for them, which states no key and which §10.7 binds to
nobody. When the person joins from their own device, under the id their key
derives and perhaps another name, the bill holds them twice. A host MAY offer
the creator to merge the added name into them (`planMerge` / `plan_merge`):
every expense naming the added name restated naming the person — as payer,
and in the split — then the added name's joins withdrawn, written in one
merge as §10.8's removal is.

- A host MUST refuse to merge a participant whose record states an
  `identityKey`, or to whom §10.7 binds one, with `unauthorized_entry`: they
  joined as themselves, and folding them into somebody else hands their debts
  and credits to another person. A merge naming somebody not on the bill, or
  one person twice, is refused with `unknown_participant`.
- Only the creator's merge is complete. Anybody else's would restate
  expenses they did not write and withdraw a join they may not (§10.8).
- In a split, a figure (`amounts`, `basisPoints`, `shareCounts`) is added
  onto the person's, so every other figure and the total stand. A list
  (`among`, an item's `sharedBy`) names the person in the added name's place;
  one that already names both is left to a person to edit, because one place
  for two names changes everybody else's share.
- A restatement MUST leave every other participant's share of that expense,
  as §4 splits it, exactly as it was, and give the person the added name's
  share and their own summed. §3 gives leftover units by id and by largest
  remainder, so a name or a figure moved onto the person can carry a unit
  across somebody else; such an expense is left to a person to edit.
- An expense the creator cannot restate, a payment to or from the added name,
  or a confirmation by them holds the merge back, as each holds back a
  removal. A merge restates expenses, so §14.9 refuses it with `bill_closed`
  while the bill is closed.

**A payment to the added name is confirmed by the creator, as them.** The
added name holds no key, so nobody can confirm a payment to them as
themselves, and a payment made to them before the person joined holds the
merge back for good: without a confirmation the bill never settles. A host
MUST let the creator confirm a payment whose payee states no `identityKey`
and is bound to no key (§10.7), written as that payee and unsigned, as their
`joinBill` was — the fold admits it as it admits any unbound author's entry.
It MUST NOT offer this to anybody but the creator, nor for a payee who
states or has bound a key, nor for a payment the creator made: a payer who
confirms their own payment asserts twice that they paid it (§10.5). `confirmerFor` / `confirmer_for` answer who writes
a confirmation of a payment, and `awaitingConfirmationFor` /
`awaiting_confirmation_for` list what a device may confirm;
`confirm_payment_for_entry` and `awaiting_my_confirmation` in the binding.

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

- `account` carries an id the account's identity is filed under. A device
  that signs speaks as the participant id its identity key derives (§10.7),
  and every entry it writes is authored by that id; a device that holds no
  identity speaks as the account's id. §10.4 decides what either authorises.
  Both MUST be stable for the life of an installed wallet; an id that changes
  between runs makes this device a new participant on every bill it has
  already touched.
- `account` MAY carry an identity secret and MAY carry none. When it carries
  one it MUST be derived from what only the account's owner holds — for a
  software wallet, its mnemonic and passphrase — so that reinstalling from the
  same mnemonic yields the same signing identity. It MUST NOT be anything the
  wallet shows or shares, such as a viewing key: whoever holds it can sign as
  this participant (§10.7), and redirect what they are paid.
  An identifier the wallet's own database assigns MUST NOT be supplied in its
  place either: it is handed out at import time, so an identity filed under it
  is a stranger to every bill naming it after a restore. No secret — a
  hardware account keeps none on the phone — means the identity is random and
  unrecoverable; it still signs correctly.
- A wallet whose account comes from a BIP39 mnemonic MUST supply as its
  identity secret the mnemonic and the BIP39 passphrase, each in Unicode NFKC
  as Unicode 17.0 defines it, the mnemonic's words joined by one ASCII space
  whatever Unicode White_Space separated them, each UTF-8 encoded, joined by
  one zero byte, and for any ZIP 32 account index other than 0, one
  further zero byte and the index as four big-endian bytes
  (`identitySecretFromMnemonic` / `identity_secret_from_mnemonic`; the binding
  answers the seed directly with `identity_seed_from_mnemonic`). Two texts
  share an NFKC form exactly when they share the NFKD form BIP39 hashes, so
  every spelling of one wallet's words is one participant, and text as a
  keyboard types it is unchanged. A code point a later Unicode version gave a
  decomposition is kept as written, so the identity does not move with a
  library's tables. The index is
  below 2^31; an empty mnemonic derives an identity anyone can compute, and a
  zero byte inside either text would let two inputs join to one secret, so
  both are refused. One layout in every wallet is what makes one person one participant
  whichever wallet they restore into, and the index is what keeps two accounts
  of one mnemonic two people rather than one key linking them on every bill.
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
- A transaction id MUST be present when the send succeeded, and MAY be present
  for the third outcome when the wallet knows the transaction it built. On
  success it becomes the reference of the payment entry §10.5 records, so the
  record of a payment and the transaction that made it carry one identifier.
  On the third it is recorded nowhere; it is what a person looks up to learn
  which way the send went.
- `payToAddress` MAY be absent. A participant with no address is reported under
  §8.5 and MUST NOT be dropped from a request, so absence is a state to show
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
- **Each key holds its own value, whole.** Two keys written hold two values,
  and a value reads back byte for byte at any length this layer writes. A
  store with one slot for every key hands one bill's key to every bill and
  makes the account's signing seed the bytes of some bill's key, which
  anybody holding that bill's invite then holds.
- **Each wallet account's secrets are its own.** A device holding several
  accounts MUST keep their secrets apart — one store per account, or every
  name scoped to it (`AccountSecretStore`, which writes `<name>@<account>`). A
  bill key is named by its bill alone, so on a store the accounts share, one
  account forgetting a bill deletes the key every other account opens it with.

### 15.4 `BillStorage`

**Operations.** `read`, `write`, `delete`, `keys`, `sweepUnfinishedWrites`.

Durable storage for this device's bills, **as raw entries**.

- Entries, not folded bills. §10.2 merges by set union, so a device holds
  entries and derives everything else; a stored summary is a second source of
  truth that goes stale without announcing it.
- `read` MUST answer empty for a key never written and MUST raise for a value
  that is there and cannot be read. A caller that took an unreadable log for
  an empty one would write its next entry over every entry the file held.
- `keys` MUST answer with every key currently stored under the given prefix,
  and MUST NOT include one whose write did not finish.
- `sweepUnfinishedWrites` MUST remove whatever an unfinished write left behind
  and MUST report how many it removed. A store that cannot leave anything
  behind reports zero. It is called once when the feature loads: a leftover is
  already invisible to `keys`, and this is what stops them accumulating across
  a year of crashes. It MUST NOT remove a write still in flight in the process
  that calls it: a sweep that raced a write would delete the entry being
  saved.
- Entries are stored in clear: §11.3 seals what a relay carries, not what a
  device holds. A host SHOULD keep its bill storage out of the platform's
  cloud backup and device-to-device transfer, or encrypt it under a key that
  stays on the device. A backup restored elsewhere hands every bill, and who
  owes whom on it, to whoever holds the backup, without the bill keys the
  keychain kept back.

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
- A rate that is fixed onto a bill prices every request on it, so a wallet
  SHOULD read each currency from two independent markets and use the figure
  only when they agree: both positive and differing by at most 200 basis
  points of the lower, compared in exact integers as `(high - low) * 10000 <=
  low * tolerance`. A market that fails or cannot price the currency defers to
  the other; with neither answering the currency is unpriced. Two markets that
  disagree are not a price, and an implementation MUST NOT pick one of them.
  When they agree the figure used is the second market's, so every
  implementation asked the same two markets in the same order fixes the same
  rate. `AgreeingZecPrices` and `agreedPrice` / `agreed_price` are this rule.

### 15.7 `SwapProvider`

**Operations.** `tradableAssets`, `quote`, `statusOf`.

Settles a debt whose payout is owed in some asset other than ZEC.

- `tradableAssets` MUST be read before quoting, so a payout naming an asset the
  provider does not carry is refused before a person is asked to send anything.
- `quote` MUST be given a refund address, and it is the payer's own. A quote
  arranged without one risks the deposit if the swap fails.
- Where a quote states the least the recipient is guaranteed, a host SHOULD
  show it to the payer in whole tokens (`formatBaseUnits` /
  `format_base_units`) before the deposit is sent: the record claims the whole
  debt, and slippage can deliver less. The record's `note` carries it too.
- The asset a quote sells is native ZEC, as the provider's own token list names
  it: symbol `ZEC` on chain `zec` (`zecAssetIn` / `zec_asset_in`). A provider
  lists ZEC wrapped on other chains too, and quoting one of those asks for a
  deposit the wallet cannot send.
- `statusOf` reports whether the swap is in flight, delivered, or will not
  complete. A transaction hash it reports is on the **destination** chain, and
  MUST NOT be recorded as the §10.5 payment for a debt settled on this one.
- A provider a build has not configured MUST raise and say why. Every operation
  failing loudly is the alternative to a swap that silently does nothing.
- A deposit MUST NOT be sent until the quote is checked again against the bill
  as the device holds it at that moment, and it is refused when the quote has
  expired; when its deposit needs a memo, which a payment request cannot
  carry; when the payout it was asked for is no longer declared; when a
  payment the payer already sent covers the debt and is unconfirmed (§14.4);
  when the bill no longer owes exactly the quoted amount to that payee; when
  the payee's payout no longer names the address the quote delivers to, or
  the asset and chain it buys; and when the bill's rate (§7.1) no longer
  converts the debt to the quote's ZEC. A quote taken before any of these
  changed sends money the bill no longer asks for, or to an address the payee
  no longer uses, and a deposit cannot be taken back.
- The deposit is sized at the bill's rate, rounded up (§7.1;
  `fiat_to_zatoshi` in the binding), and sent as one payment request to the
  quote's deposit address for the quote's zatoshi, with the note §14.3 asks
  for written first and carrying the swap, so its record can be written after
  a restart from the note alone (`swapDeposit` / `swap_deposit`). Its record
  is a `swap` payment whose id and `reference` are the provider's reference,
  stating the zatoshi sent, the bill's rate as `paidAtRate`, and a `note`
  naming the asset and chain and, when the quote stated a floor, the least the
  recipient is guaranteed (`swapRecordNote` / `swap_record_note`; the binding
  writes the whole entry with `swap_payment_entry`).
- When `statusOf` reports a swap will not complete, the payer's device MUST
  withdraw (§10.8) each unconfirmed `swap` payment record it wrote carrying
  that swap's `reference`. While such a record stands the debt reads as paid
  and waiting (§14.4), and nothing can pay it again. A record the payee has
  confirmed is left as it is.
