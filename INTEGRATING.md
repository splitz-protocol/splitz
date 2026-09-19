# Integrating splitz into a wallet

The whole surface is four calls: net the bill, plan the settlement, price it,
render one payer's obligation as a payment request.

## Dart

```dart
import 'package:splitz/splitz.dart';

final plan = settleBill(bill);                  // fewest payments
final rate = bill.rate!;                        // snapshotted into the bill

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
appendEntry(recordPayment(id: txid, from: me, to: settlement.to, ...));
```

## Rust

```rust
let plan = splitz::settle_bill(&bill, splitz::DEFAULT_EXACT_LIMIT)?;
let zatoshi = splitz::fiat_to_zatoshi(
    settlement.amount,
    bill.rate.as_ref().expect("a priced bill"),
    Some(&bill.currency),
    splitz::RateRounding::Up,
)?;
let uri = splitz::render_uri(&payments, true)?;
```

## Nine things the wallet owns

`SPEC.md` §13 lists these so they are not mistaken for gaps. Two are MUSTs.

1. **Transport.** §11.3 fixes what a sealed entry looks like and the channel it
   belongs to. Moving blobs — over what, with what retries, stored for how long
   — is the wallet's, as is running the cipher §11.3 names.
2. **Address validation and network.** §8.3 checks only what the ZIP 321
   grammar admits: non-empty and alphanumeric. **A wallet MUST decode every
   address itself and MUST check it is for the network it is transacting on.**
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
9. **Rate discovery.** §7 specifies what a snapshotted rate does, not where it
   came from.

## Five things that are easy to get wrong

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

**`fiat` is advisory.** It records what a payment's amount was priced as, not
the price of one ZEC. It MUST NOT be used to compute or adjust any output
value. It is also a *proposed* ZIP 321 parameter, so emitting it is opt-in and
off by default; a parser that predates it ignores it and constructs exactly the
payment `amount` specifies.

**The exponent is not carried.** `1234` is €12.34 in EUR, ¥1234 in JPY, and
1.234 KWD in KWD. A wallet that renders or accepts major units needs its own
ISO 4217 register, and **must refuse a code that register gives no exponent
for** — `XAU` is well-formed and has no minor unit, so a figure typed in major
units means nothing.

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
