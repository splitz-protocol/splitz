# Integrating splitz into a wallet

The whole surface is four calls: net the bill, plan the settlement, price it,
render one payer's obligation as a payment request.

## Dart

```dart
import 'package:splitz/splitz.dart';

final plan = settleBill(bill);                  // fewest payments
// A bill with no `setRate` entry is an ordinary bill, so this is a branch and
// not a `!`. There is no refusal code for "unpriced": it is not an error, and
// there is nothing for a `SplitError` handler to catch.
final rate = bill.rate;                         // snapshotted into the bill
if (rate == null) return yourOwnUnpricedBillPath();

for (final settlement in plan.settlements) {
  if (settlement.from != me) continue;

  final address = bill.participant(settlement.to)?.payableAddress;
  if (address == null) {
    // Reported, never dropped: a dropped output settles less than the plan
    // says it does, and the payer cannot tell.
    reportUnpayable(settlement.to);
    continue;
  }

  payments.add(Zip321Payment(
    address: address,
    zatoshi: fiatToZatoshi(settlement.amount, rate),
    fiat: FiatPrice(bill.currency, settlement.amount),
    label: bill.participant(settlement.to)?.name,
  ));
}

broadcast(renderUri(payments));                 // one transaction

// A bill closes because a payment is confirmed, not because one was sent.
// This library exports no entry builder: the wallet assembles the map and
// derives its §9.5 id with `deriveEntryId`, then appends it to its own log.
// `canonicalInstant` takes a string, not a clock value: this library exports
// nothing that reads a clock, because a clock is the host's (§13).
final nowIso = DateTime.now().toUtc().toIso8601String();
final entry = <String, dynamic>{
  'v': 1,
  'author': me,
  'kind': 'recordPayment',
  'at': canonicalInstant(nowIso),
  'payment': {
    'id': txid,
    'from': me,
    'to': settlement.to,
    'amount': settlement.amount,
    'method': 'shieldedZec',
    'at': canonicalInstant(nowIso),
  },
};
entry['id'] = deriveEntryId(entry);            // §9.5: the id IS the digest
```

## Rust

```rust
let plan = splitz::settle_bill(&bill, splitz::DEFAULT_EXACT_LIMIT)?;
let zatoshi = splitz::fiat_to_zatoshi(
    settlement.amount,
    // A bill with no `setRate` entry is an ordinary bill; this is a branch,
    // not an `expect`.
    bill.rate.as_ref().ok_or(NoRateYet)?,
    Some(&bill.currency),
    splitz::RateRounding::Up,
)?;
let uri = splitz::render_uri(&payments, true)?;
```

## Ten things the wallet owns

`SPEC.md` §13 lists these so they are not mistaken for gaps. Three carry a
MUST: decoding an address (2), surfacing a changed pay-to address (7), and not
attaching a memo to a transparent recipient (8).

1. **Transport.** §11.3 fixes what a sealed entry looks like and the channel it
   belongs to. Moving blobs — over what, with what retries, stored for how long
   — is the wallet's, as is running the cipher §11.3 names.
2. **Address validation and network.** §8.3 checks only what the ZIP 321
   grammar admits: non-empty and alphanumeric. **A wallet MUST decode every
   address itself and MUST check it is for the network it is transacting on.**
   ZIP 316 defines the Unified Address format; `zcash_address` in Rust and the
   equivalent in your stack are what a decoder looks like. The corpus carries
   real mainnet Unified Addresses so that running it exercises yours.
3. **Transaction construction, fees, signing, broadcast.**
4. **The curve operation.** §10.6 fixes the bytes a signature covers; producing
   and checking the Ed25519 signature is the host's. `signingMessage(entry)`
   returns exactly those bytes, so a wallet signs and verifies what every other
   implementation does.

   **Hand the verifier to the fold**: `foldLog(entries, verify: ...)` takes a
   `bool Function(entry, key)`. With one, a `createBill` whose signature fails
   opens no bill (§10.1), and `FoldResult.identities` carries `bound` and
   `contested` (§10.7). Without one both are empty — the fold reports no
   binding rather than claiming there is none. **A wallet MUST NOT settle to a
   contested participant's address without putting it in front of the payer
   first**, and that is the call that tells it which those are.
5. **Where a private key lives**, and how a participant comes by one.
6. **Storage.**
7. **Surfacing a changed pay-to address.** The fold reports every one.
   **A wallet MUST put a changed address in front of the payer before settling
   to it** — this is what stops a relayed entry silently redirecting a payout.
8. **Not attaching a memo to a transparent recipient.** ZIP 321 requires a URI
   carrying a memo at the same parameter index as a transparent address to be
   refused *in its entirety*, which takes the unrelated shielded outputs with
   it. This protocol does not parse addresses, so the rule cannot live in §8.
   A wallet has a decoder and must spend it before setting a memo.
9. **Honouring an invite's expiry.** §11.1 fixes what `x` looks like and
   parses it; comparing it to a clock is yours, along with which clock and
   what to do when two devices disagree. **There is no refusal code for an
   expired invite, because this protocol cannot tell one** — an invite that
   expired in 1970 parses without complaint. Nothing in the library will tell
   you; this line is the only warning you get.
10. **Rate discovery.** §7 specifies what a snapshotted rate does, not where it
   came from.

## Things that are easy to get wrong

**A recorded payment is a claim, not a settlement.** It moves no balance until
a `confirmPayment` entry from the person paid stands for it (§10.5). A wallet
that treats "I paid Ana" as settled clears a debt on the word of the only party
with a reason to misstate it. Equally, a wallet **must show a payer that a
payment of theirs is awaiting confirmation** rather than asking them to pay it
again — settlement reads balances, so a pending payment is not deducted and the
same debt appears in the next plan.

**The rate belongs to the bill.** Snapshot it once and put it on the bill. Six
people applying six live rates to one dinner compute six different amounts and
the bill never closes.

**A recipient the request cannot carry must be reported.** Either refuse the
whole request with `zip321_no_address`, or render the payable outputs and
report the rest alongside the URI. Doing neither is the one failure mode a
payer cannot detect: a URI that silently covers three of four debts is
indistinguishable, to the person sending it, from one that settles all four.
The same holds for a recipient whose preferred payout is `swap` or `cash` — it
cannot become an output of this URI, and the reason is not a missing address.

**`renderObligation` / `render_obligation` already does this**, and the
samples above hand-roll the loop only to show the parts. It takes the
settlements and the bill, plus the rate, and returns the URI together with the
recipients it could not carry:
`renderObligation(plan.settlements, bill, rate: rate, skipUnpayable: true)`. With `skipUnpayable` it
renders the payable outputs and reports the rest; without it a recipient it
cannot carry refuses the whole request. Prefer it to writing the loop: this
is the hazard the loop exists to get wrong.

**`fiat` is advisory.** It records what a payment's amount was priced as, not
the price of one ZEC. It MUST NOT be used to compute or adjust any output
value. It is also a *proposed* ZIP 321 parameter, so emitting it is opt-in and
off by default; a parser that predates it ignores it and constructs exactly the
payment `amount` specifies.

**Raising `exactLimit` buys minimality with wall-clock time.** The partition
search allocates four arrays of 2ⁿ and walks 3ⁿ submasks, so each participant
past the default of 14 roughly triples the work. Measured on one desktop CPU,
medians over a warmed run, Dart under `dart run`:

| participants | 14 | 16 | 18 | 20 |
|---|---|---|---|---|
| median | 5.2 ms | 47 ms | 445 ms | 4.3 s |

The shape of the balances barely moves it — the submask walk runs to
completion whatever the values are. **It is a synchronous call.** A wallet that
raises the limit runs it off the thread that draws the UI, or the app stops
responding for those seconds. Build profile matters more than you would
expect: the same search in a Rust debug build is several times slower again,
so budget against the profile you ship. §6.2 caps the limit at 20
(`exact_limit_too_large`); past it the plan treats the whole set as one group
and reports `isOptimal: false`, which is a worse plan and not a wrong one.

**The exponent is not carried.** `1234` is €12.34 in EUR, ¥1234 in JPY, and
1.234 KWD in KWD. A wallet that renders or accepts major units needs its own
ISO 4217 register, and **must refuse a code that register gives no exponent
for** — `XAU` is well-formed and has no minor unit, so a figure typed in major
units means nothing.

**A refusal is reported, not thrown away, and it does not converge.**
`foldLog` returns `setAside` and `mergeLogs` returns `refused` — each row an
entry id and a §12 code. Render them; an entry that vanished silently is
indistinguishable from one that was never sent. But do not compare the lists
across devices: one that received a malformed entry twice reports it twice,
and one that merged before folding reports it from the merge instead. The
bill, the balances and the withdrawals converge (§10.2); the refusal report is
per occurrence (§10.3).

**One entry can never take the bill down.** Every payload member this
library's own decoder requires is decided when the entry is applied, so a
document the fold returns is one `decodeBill` accepts. A malformed expense is
set aside with its own id and code and the rest of the bill opens. Do not
write recovery code for an unopenable bill; write code that shows `setAside`.

## What a payer sees

Netting reroutes payments, so a payer is often asked to pay someone they never
transacted with. Each settlement carries the debts it discharges, so a UI can
answer the obvious question.

```
Cai pays $162.50 in ONE transaction:
    $70.04     to Ana
    $92.46     to Dee   <- rerouted: covers Ana $92.46
```

A bill that shows only the result cannot explain why someone owes $100 to a
person who never lent them money.

## Conformance

A third implementation is conformant when it reproduces every case in
`vectors/`. Refusing an input for the wrong reason is a failure: the code is
what a wallet turns into a sentence for its user, and the remedy differs.
`payload_future_version` means update the app; `payload_damaged` means the data
is broken.
