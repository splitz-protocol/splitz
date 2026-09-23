# Integrating splitz into a wallet

The whole surface is four steps: net the bill, plan the settlement, price it,
render one payer's obligation as a payment request. `renderObligation` does the
last two of those, so the shortest wallet makes three calls.

There are two imports, and a wallet usually wants both. The protocol —
`package:splitz_core/splitz_core.dart`, `splitz_core::` — is what this page's first two
sections show. The wallet seam — `package:splitz_core/host.dart`,
`splitz_core::host::` — is the layer above it: something that assembles an entry
and derives §9.5's id, a log that merges and folds, the two scans that move a
bill between phones, and one payer's obligation from a folded bill. See **The
wallet seam** below.

A wallet in Kotlin, Swift or JavaScript reaches the same protocol through
`splitz-ffi`, which is those two layers across a foreign function boundary.
See **Kotlin, Dart and JavaScript**.

How to add the dependency, by git and pinned to a commit, is in the README
under **Use it in your wallet**.

## Dart

```dart
import 'package:splitz_core/splitz_core.dart';

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
// The protocol import exports no entry builder: at this level the wallet
// assembles the map and derives its §9.5 id with `deriveEntryId`, then
// appends it to its own log. `package:splitz_core/host.dart` has `recordPayment`
// and the rest, which do exactly this and hand back an entry whose id is
// already the digest; see **The wallet seam**.
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
let plan = splitz_core::settle_bill(&bill, splitz_core::DEFAULT_EXACT_LIMIT)?;
let zatoshi = splitz_core::fiat_to_zatoshi(
    settlement.amount,
    // A bill with no `setRate` entry is an ordinary bill; this is a branch,
    // not an `expect`.
    bill.rate.as_ref().ok_or(NoRateYet)?,
    Some(&bill.currency),
    splitz_core::RateRounding::Up,
)?;
let uri = splitz_core::render_uri(&payments, true)?;
```

## The wallet seam

One import above the protocol, in both languages. It holds no key, opens no
socket and reads no clock of its own: everything it cannot do is declared as
one interface the wallet implements.

```dart
import 'package:splitz_core/splitz_core.dart' as splitz;
import 'package:splitz_core/host.dart';

class MyWallet extends BillHost {
  @override String get me => 'ana';
  @override String? get payToAddress => 'u1ana…';
  @override Clock get now => DateTime.now;
  @override Randomness get randomBytes => secureRandom;
  @override Broadcast get broadcast => (uri) async => Sent.sent(await send(uri));
  // `sign` and `verify` default to null. Without them §10.7 binds no key and
  // a folded bill reports no identity binding rather than claiming one.
}

final host = MyWallet();
final log = BillLog(host)
  ..add([createBill(host: host, name: 'Dinner', currency: 'EUR',
                    creatorKey: myEd25519PublicKeyBase64Url)]);

final folded = log.fold();                     // §10.3, plus what it refused
final owed = obligationFor(host, folded);      // null when the bill has no rate
if (owed != null) await settle(host, log, owed);
```

```rust
use splitz_core::host::{create_bill, obligation_for, settle, BillHost, BillLog, Sent};

impl BillHost for MyWallet {
    fn me(&self) -> &str { "ana" }
    fn pay_to_address(&self) -> Option<&str> { Some("u1ana…") }
    // An RFC 3339 instant, not a date type: this crate depends on no calendar
    // library, and `canonical_instant` refuses anything that is not one.
    fn now(&self) -> String { self.clock.now_rfc3339() }
    fn random_bytes(&self, n: usize) -> Vec<u8> { self.rng.fill(n) }
    // Synchronous, because this crate pulls in no async runtime and so cannot
    // own the executor a future would need. Block here, where you know yours.
    fn broadcast(&self, uri: &str) -> Sent { Sent::sent(self.send(uri)) }
}
```

What the seam adds, in both:

| | Dart | Rust |
|---|---|---|
| The wallet's obligations | `BillHost` | `splitz_core::host::BillHost` |
| Entries, with §9.5's id already derived | `createBill`, `joinBill`, `addExpense`, `recordPayment`, `confirmPayment`, `setRate`, `voidEntry` | the same names in snake case |
| A signature over §10.6's message | `signEntry` | `sign_entry` |
| One bill's log, merged and folded | `BillLog`, `FoldedBill` | `BillLog`, `FoldedBill` |
| What a camera produced | `readScan`, `inviteFor`, `shareableBill`, `deltaFor`, `acceptScan` | the same names in snake case |
| One payer's obligation, and sending it | `obligationFor`, `settle` | `obligation_for`, `settle` |

`tools/parity/surface.py` diffs the two seams the way it diffs the two
protocol surfaces, and records every divergence with its reason in
`tools/parity/allow-host.txt`.

Three differences are idiom and are stated at the declarations: Dart names
each obligation as a typedef and Rust declares it as a trait method; Dart's
`broadcast` and `sign` return futures and Rust's are synchronous; Dart's scan
outcomes are a sealed class hierarchy and Rust's are one enum.

## Kotlin, Dart and JavaScript

A wallet in a third language reaches the same protocol through `splitz-ffi`, a
cdylib with a uniffi binding generated from it. Build it, generate the module
for your language, and link the library:

```
cargo build -p splitz-ffi --release
cargo run --bin uniffi-bindgen -p splitz-ffi -- generate --no-format \
    --library target/release/libsplitz_ffi.dylib --language kotlin --out-dir .
```

That writes `uniffi/splitz_ffi/splitz_ffi.kt`. `--no-format` skips a ktlint
pass that warns rather than fails when ktlint is not installed. The languages
uniffi's own generator carries are `kotlin`, `swift`, `python` and `ruby`;
Dart and JavaScript come from third parties, below.

**Nothing in it calls back.** The seam above declares seven interfaces a
wallet implements, and across a foreign boundary those are seven sets of
callbacks — the part of a binding every generator gets wrong differently. So a
wallet passes the facts it owns and gets an answer:

| `HostFacts` | what it is |
|---|---|
| `me` | the participant id every entry this device writes is authored by |
| `payTo` | the address this device is paid at, or none |
| `now` | a §9.3 instant. Read when an entry is written, never while folding: §10.2 orders a log by instant, so a fold that read a clock would answer differently for one unchanged entry set |
| `nonce` | sixteen bytes nobody can predict, for §9.4. Two bills opened in one second by one person are one bill when this can be guessed |

Storage, the keychain, the relay and the send stay in the wallet's own
language. An entry crosses as the JSON §9.3 canonicalises — it is the
protocol's own wire format and a wallet never inspects one — and everything a
person is shown crosses as a typed record. A refusal crosses as its §12 code.

The three samples below are one program in three languages, and each is a file
a lane runs: `tools/ffi/kotlin.sh`, `tools/ffi/dart.sh` and `tools/ffi/node.sh`
compile and execute exactly these, and `tools/docs/blocks.py` fails if what is
printed here and what is run ever differ.

Two names to expect. The generated module follows each language's convention —
`identityKeyFromSeed` in Kotlin and Dart, `identity_key_from_seed` in
JavaScript — and the JavaScript generator spells record *fields* as Rust does,
so a wallet passes `pay_to` there and `payTo` in the other two.

### Kotlin

```kotlin file=tools/ffi/kotlin/Doc.kt
import uniffi.splitz_ffi.*

/// The facts §15.1 says a wallet owns, for one call. A §9.3 instant and
/// sixteen unpredictable bytes are the wallet's to supply: this library reads
/// no clock (§13) and owns no entropy.
fun facts(me: String, payTo: String, at: String, nonce: Int) =
    HostFacts(me, payTo, at, ByteArray(16) { (nonce + it).toByte() })

/// The Ed25519 seed a wallet keeps in the platform keychain, as §9.4 writes a
/// key: 32 bytes, unpadded base64url.
fun seed(first: Int): String = java.util.Base64.getUrlEncoder().withoutPadding()
    .encodeToString(ByteArray(32) { (first + it).toByte() })

fun main() {
    val anaSeed = seed(1)
    val benSeed = seed(90)

    // Ana's device writes four entries. Each comes back as the JSON §9.3
    // canonicalises, with §9.5's id already derived; the wallet stores the
    // string and never inspects it.
    val create = createBillEntry(facts("ana", "u1ana", "2026-10-28T19:31:00.000Z", 1),
        "Dinner", "EUR", "equal", identityKeyFromSeed(anaSeed), anaSeed)

    // The bill these entries belong to, read back from the entry that opened
    // it. Every other entry is signed on it (§10.6), and every fold names it,
    // so a create for another bill pushed into the channel cannot make this
    // one unopenable.
    val billId = Regex("\"id\":\"([^\"]+)\"").find(create)!!.groupValues[1]

    val anaLog = listOf(
        create,
        joinBillEntry(facts("ana", "u1ana", "2026-10-28T19:32:00.000Z", 2), billId,
            "Ana", "u1ana", identityKeyFromSeed(anaSeed), anaSeed),
        addExpenseEntry(facts("ana", "u1ana", "2026-10-28T19:33:00.000Z", 3), billId,
            "x1", "ana", 9000, """{"type":"equal","among":["ana","ben"]}""",
            "dinner", anaSeed),
        // §7 snapshots one rate onto the bill, so six devices do not price one
        // dinner six ways. 300000 minor units per ZEC is €3000.00.
        setRateEntry(facts("ana", "u1ana", "2026-10-28T19:34:00.000Z", 4), billId,
            "EUR", 300000, "a fixed feed", anaSeed),
    )

    // Ben's own device writes Ben's join: §10.4 decides what an entry's author
    // may say, and a participant joins for themselves.
    val benLog = listOf(
        joinBillEntry(facts("ben", "u1ben", "2026-10-28T19:35:00.000Z", 5), billId,
            "Ben", "u1ben", identityKeyFromSeed(benSeed), benSeed),
    )

    // Merging is how two devices come to agree (§10.2). It is a set union by
    // id, in either direction, any number of times.
    val log = mergeEntries(anaLog, benLog).entries

    val benFacts = facts("ben", "u1ben", "2026-10-28T19:36:00.000Z", 6)
    val folded = foldEntries(benFacts, billId, log)
    println("on the bill: " + folded.bill.participants.joinToString { it.id })
    // Render these. An entry the fold set aside is one a person cannot see
    // otherwise, and its §12 code is what a wallet turns into a sentence.
    println("set aside: " + folded.setAside)

    // Null when the bill carries no rate: an unpriced bill is an ordinary
    // bill, not a refusal. The second argument names the contested
    // participants the payer has been shown and chosen to pay anyway (§10.7).
    val owed = obligationOf(benFacts, billId, log, listOf())
        ?: error("a bill with a rate owes something")

    val settlement = owed.settlements.single()
    println("ben pays ${settlement.amount} to ${settlement.to}")
    // The wallet broadcasts this; sending is not the library's (§13.3).
    println("request: ${owed.request.uri}")
    // Never dropped. A request that silently covers three debts of four is
    // indistinguishable, to the payer, from one that covers all of them.
    println("withheld: ${owed.request.withheldMinorUnits}")

    check(settlement.to == "ana" && settlement.amount == 4500L) {
        "half of 9000 is 4500 to ana, saw ${settlement.amount} to ${settlement.to}"
    }
    check(owed.request.uri!!.startsWith("zcash:u1ana")) { "${owed.request.uri}" }
    check(owed.request.withheldMinorUnits == 0L) { "${owed.request.withheldMinorUnits}" }
    println("DOC RESULT: kotlin")
}
```

### Dart

`configureDefaultBindings` is the generated entry point: it takes the path to
the library and must be called before anything else. It works the same under
Flutter's test harness as under the standalone VM — `tools/ffi/flutter.sh`
asserts that, because the two are different hosts and only one of them is what
a wallet ships.

Cross-compiling for a phone is `tools/package/ios.sh` and
`tools/package/android.sh`. The first writes
`dist/ios/splitz_ffi.xcframework` — a device slice and a fat simulator slice,
which cannot be one archive because both are arm64 and `lipo` refuses two
slices of one architecture. The second writes `dist/android/jniLibs/<abi>/`
for `arm64-v8a`, `armeabi-v7a`, `x86_64` and `x86`, against NDK API 21.

Each script also writes the package a wallet actually declares, rather than
loose artefacts. `dist/ios/SplitzFFI` is a Swift package — a consumer adds
`.package(path:)` and writes `import SplitzFFI`. `dist/android/splitz` is a
Gradle module — `gradle assembleRelease` there produces an AAR carrying all
four ABIs under `jni/` and the generated Kotlin compiled into `classes.jar`,
and a consumer writes one `implementation` line. Neither publishes anything;
where the artefact goes is still the wallet's build system.

`tools/package/npm.sh` writes `dist/npm` and packs it. A JavaScript wallet
installs the tarball and imports by name — `import * as splitz from
"splitz-ffi"` — with no library path, because the package carries its own
under `prebuilds/`. **One run produces one platform: the machine it ran on.**
The manifest's `os` and `cpu` say which, so a foreign platform is refused at
install time rather than at the first call. A real release needs the build
repeated per platform with the `prebuilds/` directories merged, and nothing
in this tree does that.

Both mobile libraries are executed, not merely built.
`tools/ffi/swift-simulator.sh` runs the bill on a booted iOS simulator, and
`tools/ffi/aar-device.sh` runs it as an instrumented test on an Android
emulator, where the platform extracts `jni/<abi>/libsplitz_ffi.so` from the
APK and loads it. Each has a JVM or macOS twin that proves the packaged
surface; these two prove the shipped binary.

Two costs the Android side carries, and they are not obvious from the file:

- The AAR declares `minCompileSdk=36`, so a wallet compiling against an older
  SDK cannot depend on it at all.
- A wallet that resolves it from a repository gets JNA transitively, because
  `gradle publishToMavenLocal` writes a POM that names it. A wallet that drops
  the bare `.aar` into a directory gets no POM, and must declare
  `net.java.dev.jna:jna:5.17.0@aar` itself or the binding's own types will not
  resolve.

```dart file=tools/ffi/dart/doc.dart
import 'dart:convert';
import 'dart:typed_data';

import 'package:splitz_dart_consumer/splitz_ffi.dart';

/// The facts §15.1 says a wallet owns, for one call. A §9.3 instant and
/// sixteen unpredictable bytes are the wallet's to supply: this library reads
/// no clock (§13) and owns no entropy.
HostFacts facts(String me, String payTo, String at, int nonce) => HostFacts(
  me: me,
  payTo: payTo,
  now: at,
  nonce: Uint8List.fromList(List.generate(16, (i) => (nonce + i) & 0xff)),
);

/// The Ed25519 seed a wallet keeps in the platform keychain, as §9.4 writes a
/// key: 32 bytes, unpadded base64url.
String seed(int first) => base64Url
    .encode(List.generate(32, (i) => (first + i) & 0xff))
    .replaceAll('=', '');

void main(List<String> args) {
  configureDefaultBindings(libraryPath: args[0]);

  final anaSeed = seed(1);
  final benSeed = seed(90);

  // Ana's device writes four entries. Each comes back as the JSON §9.3
  // canonicalises, with §9.5's id already derived; the wallet stores the
  // string and never inspects it.
  final create = createBillEntry(
    facts('ana', 'u1ana', '2026-10-28T19:31:00.000Z', 1),
    'Dinner',
    'EUR',
    'equal',
    identityKeyFromSeed(anaSeed),
    anaSeed,
  );

  // The bill these entries belong to, read back from the entry that opened
  // it. Every other entry is signed on it (§10.6), and every fold names it,
  // so a create for another bill pushed into the channel cannot make this
  // one unopenable.
  final billId = (jsonDecode(create) as Map)['id'] as String;

  final anaLog = [
    create,
    joinBillEntry(
      facts('ana', 'u1ana', '2026-10-28T19:32:00.000Z', 2),
      billId,
      'Ana',
      'u1ana',
      identityKeyFromSeed(anaSeed),
      anaSeed,
    ),
    addExpenseEntry(
      facts('ana', 'u1ana', '2026-10-28T19:33:00.000Z', 3),
      billId,
      'x1',
      'ana',
      9000,
      '{"type":"equal","among":["ana","ben"]}',
      'dinner',
      anaSeed,
    ),
    // §7 snapshots one rate onto the bill, so six devices do not price one
    // dinner six ways. 300000 minor units per ZEC is €3000.00.
    setRateEntry(
      facts('ana', 'u1ana', '2026-10-28T19:34:00.000Z', 4),
      billId,
      'EUR',
      300000,
      'a fixed feed',
      anaSeed,
    ),
  ];

  // Ben's own device writes Ben's join: §10.4 decides what an entry's author
  // may say, and a participant joins for themselves.
  final benLog = [
    joinBillEntry(
      facts('ben', 'u1ben', '2026-10-28T19:35:00.000Z', 5),
      billId,
      'Ben',
      'u1ben',
      identityKeyFromSeed(benSeed),
      benSeed,
    ),
  ];

  // Merging is how two devices come to agree (§10.2). It is a set union by
  // id, in either direction, any number of times.
  final log = mergeEntries(anaLog, benLog).entries;

  final benFacts = facts('ben', 'u1ben', '2026-10-28T19:36:00.000Z', 6);
  final folded = foldEntries(benFacts, billId, log);
  print('on the bill: ${folded.bill.participants.map((p) => p.id).join(', ')}');
  // Render these. An entry the fold set aside is one a person cannot see
  // otherwise, and its §12 code is what a wallet turns into a sentence.
  print('set aside: ${folded.setAside}');

  // Null when the bill carries no rate: an unpriced bill is an ordinary bill,
  // not a refusal. The third argument names the contested participants the
  // payer has been shown and chosen to pay anyway (§10.7).
  final owed = obligationOf(benFacts, billId, log, const []);
  if (owed == null) throw StateError('a bill with a rate owes something');

  final settlement = owed.settlements.single;
  print('ben pays ${settlement.amount} to ${settlement.to}');
  // The wallet broadcasts this; sending is not the library's (§13.3).
  print('request: ${owed.request.uri}');
  // Never dropped. A request that silently covers three debts of four is
  // indistinguishable, to the payer, from one that covers all of them.
  print('withheld: ${owed.request.withheldMinorUnits}');

  if (settlement.to != 'ana' || settlement.amount != 4500) {
    throw StateError(
      'half of 9000 is 4500 to ana, saw '
      '${settlement.amount} to ${settlement.to}',
    );
  }
  if (!owed.request.uri!.startsWith('zcash:u1ana'))
    throw StateError(owed.request.uri!);
  if (owed.request.withheldMinorUnits != 0) {
    throw StateError('${owed.request.withheldMinorUnits}');
  }
  print('DOC RESULT: dart');
}
```

### JavaScript

```javascript file=tools/ffi/node/doc.mjs
import * as splitz from "./splitz_ffi.js";
import { load } from "./splitz_ffi-ffi.js";

load(process.argv[2]);

// The facts §15.1 says a wallet owns, for one call. A §9.3 instant and sixteen
// unpredictable bytes are the wallet's to supply: this library reads no clock
// (§13) and owns no entropy. The generated record spells its fields as Rust
// does, so it is `pay_to` here and `payTo` in Kotlin and Dart.
const facts = (me, payTo, at, nonce) => ({
  me,
  pay_to: payTo,
  now: at,
  nonce: Uint8Array.from({ length: 16 }, (_, i) => (nonce + i) & 0xff),
});

// The Ed25519 seed a wallet keeps in the platform keychain, as §9.4 writes a
// key: 32 bytes, unpadded base64url.
const seed = (first) =>
  Buffer.from(Array.from({ length: 32 }, (_, i) => (first + i) & 0xff)).toString(
    "base64url",
  );

const anaSeed = seed(1);
const benSeed = seed(90);

// Ana's device writes four entries. Each comes back as the JSON §9.3
// canonicalises, with §9.5's id already derived; the wallet stores the string
// and never inspects it.
const create = splitz.create_bill_entry(facts("ana", "u1ana", "2026-10-28T19:31:00.000Z", 1),
  "Dinner", "EUR", "equal", splitz.identity_key_from_seed(anaSeed), anaSeed);

// The bill these entries belong to, read back from the entry that opened it.
// Every other entry is signed on it (§10.6), and every fold names it, so a
// create for another bill pushed into the channel cannot make this one
// unopenable.
const billId = JSON.parse(create).id;

const anaLog = [
  create,
  splitz.join_bill_entry(facts("ana", "u1ana", "2026-10-28T19:32:00.000Z", 2), billId,
    "Ana", "u1ana", splitz.identity_key_from_seed(anaSeed), anaSeed),
  splitz.add_expense_entry(facts("ana", "u1ana", "2026-10-28T19:33:00.000Z", 3), billId,
    "x1", "ana", 9000, '{"type":"equal","among":["ana","ben"]}', "dinner", anaSeed),
  // §7 snapshots one rate onto the bill, so six devices do not price one
  // dinner six ways. 300000 minor units per ZEC is €3000.00.
  splitz.set_rate_entry(facts("ana", "u1ana", "2026-10-28T19:34:00.000Z", 4), billId,
    "EUR", 300000, "a fixed feed", anaSeed),
];

// Ben's own device writes Ben's join: §10.4 decides what an entry's author may
// say, and a participant joins for themselves.
const benLog = [
  splitz.join_bill_entry(facts("ben", "u1ben", "2026-10-28T19:35:00.000Z", 5), billId,
    "Ben", "u1ben", splitz.identity_key_from_seed(benSeed), benSeed),
];

// Merging is how two devices come to agree (§10.2). It is a set union by id,
// in either direction, any number of times.
const log = splitz.merge_entries(anaLog, benLog).entries;

const benFacts = facts("ben", "u1ben", "2026-10-28T19:36:00.000Z", 6);
const folded = splitz.fold_entries(benFacts, billId, log);
console.log("on the bill: " + folded.bill.participants.map((p) => p.id).join(", "));
// Render these. An entry the fold set aside is one a person cannot see
// otherwise, and its §12 code is what a wallet turns into a sentence.
console.log("set aside: " + JSON.stringify(folded.set_aside));

// Undefined when the bill carries no rate: an unpriced bill is an ordinary
// bill, not a refusal. The third argument names the contested participants the
// payer has been shown and chosen to pay anyway (§10.7).
const owed = splitz.obligation_of(benFacts, billId, log, []);
if (owed === undefined) throw new Error("a bill with a rate owes something");

const settlement = owed.settlements[0];
console.log(`ben pays ${settlement.amount} to ${settlement.to}`);
// The wallet broadcasts this; sending is not the library's (§13.3).
console.log(`request: ${owed.request.uri}`);
// Never dropped. A request that silently covers three debts of four is
// indistinguishable, to the payer, from one that covers all of them.
console.log(`withheld: ${owed.request.withheld_minor_units}`);

if (settlement.to !== "ana" || Number(settlement.amount) !== 4500) {
  throw new Error(`half of 9000 is 4500 to ana, saw ${settlement.amount} to ${settlement.to}`);
}
if (!owed.request.uri.startsWith("zcash:u1ana")) throw new Error(owed.request.uri);
if (Number(owed.request.withheld_minor_units) !== 0) {
  throw new Error(`${owed.request.withheld_minor_units}`);
}
console.log("DOC RESULT: javascript");
```

### Generators

The crate pins `uniffi = "=0.31.0"`, because every bindings generator outside
uniffi's own tree targets 0.31. The third-party ones carry two requirements
their own documentation does not state:

- The crate must build with uniffi's `scaffolding-ffi-buffer-fns` feature, or
  the Dart generator's output links against symbols the library does not
  export.
- `uniffi-bindgen-dart` needs `--crate <name>`, or it looks up
  `ffi_uniffi_<name>_rustbuffer_*` where the library exports
  `ffi_<name>_rustbuffer_*`.

`tools/ffi/swift.sh` drives a whole bill from Swift. It builds its consumer
against the package `tools/package/ios.sh` writes — `.package(path:)` and
`import SplitzFFI` — rather than against this source tree, because what a
wallet reaches is the package and not the tree.

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
   and checking the Ed25519 signature is the host's.
   `signingMessage(entry, billId)` returns exactly those bytes, so a wallet
   signs and verifies what every other implementation does. The bill is part
   of the message: a signature made on one bill does not verify on another,
   so verify on the bill being folded, never on one taken from the entry.

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

**A send that has not resolved blocks the next one.** A wallet reports three
outcomes, not two (§14.3), and the third — built, not yet on the network — may
still land. `settle` records nothing for it, so the debt stays in the plan and
a second tap sends it again. Write down what the request carried
(`PayerObligation.carriedTo` / `carried_to()`) **before** calling the wallet,
keep it across a restart, and refuse another send from that bill until the
person says which way it went. If the transaction turns up, `recordSend` /
`record_send` writes the same records `settle` would have, so a payee confirms
the same payment either way.

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

**Show the part nothing explains.** `Settlement.unexplained` is the amount no
direct debt of the payer's accounts for. §4 admits a negative expense, so a
peer can write a "refund" that makes somebody owe them money they never
borrowed; that excess is exactly this figure, and a screen that shows only the
total asks the payer to send it.

## Conformance

A third implementation is conformant when it reproduces every case in
`vectors/`. Refusing an input for the wrong reason is a failure: the code is
what a wallet turns into a sentence for its user, and the remedy differs.
`payload_future_version` means update the app; `payload_damaged` means the data
is broken.
