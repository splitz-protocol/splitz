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

A whole program, run by `tools/examples/run.sh`. It plans one payer's share,
renders it as one request, and records what the send paid — one record per
payee, each under its own id (§10.5).

```dart file=dart/example/integrating.dart
import 'package:splitz_core/splitz_core.dart';

// A bill: Ben and Cai each paid 30.00 split with Ana, so Ana owes each of
// them 15.00, and it carries the rate §7 snapshotted onto it. A wallet has
// this from a fold.
final bill = decodeBill({
  'v': 1,
  'id': 'b1',
  'currency': 'EUR',
  'participants': [
    {'id': 'ana', 'name': 'Ana', 'payTo': 'u1ana0000000000000000000'},
    {'id': 'ben', 'name': 'Ben', 'payTo': 'u1ben0000000000000000000'},
    {'id': 'cai', 'name': 'Cai', 'payTo': 'u1cai0000000000000000000'},
  ],
  'expenses': [
    {
      'id': 'x1',
      'paidBy': 'ben',
      'amount': 3000,
      'at': '2026-10-28T19:30:00.000Z',
      'split': {
        'type': 'equal',
        'among': ['ana', 'ben'],
      },
    },
    {
      'id': 'x2',
      'paidBy': 'cai',
      'amount': 3000,
      'at': '2026-10-28T19:31:00.000Z',
      'split': {
        'type': 'equal',
        'among': ['ana', 'cai'],
      },
    },
  ],
  'rate': {
    'currency': 'EUR',
    'minorUnitsPerZec': 300000,
    'at': '2026-10-28T19:32:00.000Z',
  },
});

const me = 'ana';

void main() {
  final plan = settleBill(bill); // fewest payments
  // A bill with no `setRate` entry is an ordinary bill, so this is a branch
  // and not a `!`. There is no refusal code for "unpriced": it is not an
  // error, and there is nothing for a `SplitError` handler to catch.
  final rate = bill.rate;
  if (rate == null) return;

  final payments = <Zip321Payment>[];
  final paidTo = <Settlement>[];
  for (final settlement in plan.settlements) {
    if (settlement.from != me) continue;

    final address = bill.participant(settlement.to)?.payableAddress;
    if (address == null) {
      // Reported, never dropped: a dropped output settles less than the plan
      // says it does, and the payer cannot tell.
      print('cannot pay ${settlement.to} here');
      continue;
    }

    payments.add(
      Zip321Payment(
        address: address,
        zatoshi: fiatToZatoshi(settlement.amount, rate),
        fiat: FiatPrice(bill.currency, settlement.amount),
        label: bill.participant(settlement.to)?.name,
      ),
    );
    paidTo.add(settlement);
  }

  final uri = renderUri(payments); // one transaction
  print('request: $uri');
  const txid = 'tx-from-the-wallet'; // what the wallet's broadcast returned

  // A bill closes because a payment is confirmed, not because one was sent.
  // One transaction paying two people is two records, and §10.5 requires each
  // to carry its own id: under one id the second is set aside and its payee
  // asked to be paid again. The transaction goes in `reference`, and each
  // record states what it sent in ZEC and the rate it was priced at, so the
  // payee confirms against a figure they can compare with what arrived.
  //
  // The protocol import exports no entry builder: at this level the wallet
  // assembles the map and derives its §9.5 id with `deriveEntryId`.
  // `package:splitz_core/host.dart` has `recordSend`, which does exactly this.
  // `canonicalInstant` takes a string, not a clock value: this library
  // exports nothing that reads a clock, because a clock is the host's (§13).
  final at = canonicalInstant('2026-10-28T19:40:00.000Z');
  final records = <Map<String, dynamic>>[];
  for (var i = 0; i < paidTo.length; i++) {
    final settlement = paidTo[i];
    final entry = <String, dynamic>{
      'v': 1,
      'author': me,
      'kind': 'recordPayment',
      'at': at,
      'payment': {
        'id': '$txid:${settlement.to}',
        'from': me,
        'to': settlement.to,
        'amount': settlement.amount,
        'method': 'shieldedZec',
        'at': at,
        'reference': txid,
        'zatoshi': payments[i].zatoshi,
        'paidAtRate': rateToJson(rate),
      },
    };
    entry['id'] = deriveEntryId(entry); // §9.5: the id IS the digest
    records.add(checkEntry(entry));
  }

  final ids = {for (final r in records) (r['payment'] as Map)['id']};
  print('records: ${records.length}, ids: ${ids.join(', ')}');
  if (records.length != 2 || ids.length != 2) {
    throw StateError('one record per payee, each its own id');
  }
}
```

## Rust

```rust file=rust/splitz-core/examples/integrating.rs
use splitz_core::{decode_bill, fiat_to_zatoshi, render_uri, settle_bill, FiatPrice};
use splitz_core::{RateRounding, Zip321Payment, DEFAULT_EXACT_LIMIT};

fn main() -> splitz_core::Result<()> {
    // Ben paid 30.00 split with Ana, so Ana owes him 15.00; the bill carries
    // the rate §7 snapshotted onto it. A wallet has this from a fold.
    let bill = decode_bill(&serde_json::json!({
        "v": 1, "id": "b1", "currency": "EUR",
        "participants": [
            {"id": "ana", "name": "Ana", "payTo": "u1ana0000000000000000000"},
            {"id": "ben", "name": "Ben", "payTo": "u1ben0000000000000000000"}
        ],
        "expenses": [{
            "id": "x1", "paidBy": "ben", "amount": 3000,
            "at": "2026-10-28T19:30:00.000Z",
            "split": {"type": "equal", "among": ["ana", "ben"]}
        }],
        "rate": {"currency": "EUR", "minorUnitsPerZec": 300000,
                 "at": "2026-10-28T19:32:00.000Z"}
    }))?;

    let plan = settle_bill(&bill, DEFAULT_EXACT_LIMIT)?;
    // A bill with no `setRate` entry is an ordinary bill; this is a branch,
    // not an `expect`.
    let Some(rate) = bill.rate.as_ref() else {
        return Ok(());
    };
    let mut payments = Vec::new();
    for settlement in plan.settlements.iter().filter(|s| s.from == "ana") {
        let who = bill
            .participant(&settlement.to)
            .expect("the plan names participants");
        // Reported, never dropped: a dropped output settles less than the
        // plan says it does, and the payer cannot tell.
        let Some(address) = who.payable_address() else {
            println!("cannot pay {} here", settlement.to);
            continue;
        };
        payments.push(Zip321Payment {
            address: address.to_owned(),
            zatoshi: fiat_to_zatoshi(
                settlement.amount,
                rate,
                Some(&bill.currency),
                RateRounding::Up,
            )?,
            fiat: Some(FiatPrice {
                currency: bill.currency.clone(),
                minor_units: settlement.amount,
            }),
            label: Some(who.name.clone()),
            ..Default::default()
        });
    }
    let uri = render_uri(&payments, true)?; // one transaction
    println!("request: {uri}");
    assert!(uri.starts_with("zcash:u1ben"), "{uri}");
    Ok(())
}
```

## The wallet seam

One import above the protocol, in both languages. It holds no key, opens no
socket and reads no clock of its own: everything it cannot do is declared as
one interface the wallet implements.

```dart file=dart/example/seam.dart
import 'dart:typed_data';

import 'package:splitz_core/host.dart';
import 'package:splitz_core/splitz_core.dart' as splitz;

class MyWallet extends BillHost {
  @override
  String get me => 'ana';
  @override
  Clock get now => DateTime.now;
  @override
  Randomness get randomBytes => secureRandom;
  @override
  Broadcast get broadcast => (uri) async => Sent.sent(await send(uri));
  // `sign` and `verify` default to null. Without them §10.7 binds no key and
  // a folded bill reports no identity binding rather than claiming one.

  /// The address this wallet is paid at, which its join states.
  String get myAddress => 'u1ana000000000000000000';

  /// Stands in for the platform's secure random source.
  Uint8List secureRandom(int n) =>
      Uint8List.fromList(List<int>.generate(n, (i) => i * 7 + 1));

  /// Stands in for the wallet's send path.
  Future<String> send(String uri) async => 'tx-${uri.length}';
}

Future<void> main() async {
  final host = MyWallet();
  final myEd25519PublicKeyBase64Url = base64UrlNoPad(
    List<int>.generate(creatorKeyBytes, (i) => i),
  );

  final log = BillLog(host)
    ..add([
      createBill(
        host: host,
        name: 'Dinner',
        currency: 'EUR',
        creatorKey: myEd25519PublicKeyBase64Url,
      ),
    ]);

  // A bill nobody has joined and nothing has been spent on still folds.
  log.add([joinBill(host: host, name: 'Ana', payTo: host.myAddress)]);

  final folded = log.fold(); // §10.3, plus what it refused
  final owed = obligationFor(host, folded); // null when the bill has no rate
  if (owed != null) await settle(host, log, owed);

  print('bill       ${folded.bill.id}');
  print('entries    ${log.entries.length}');
  print('setAside   ${folded.setAside.length}');
  print('obligation ${owed == null ? 'no rate yet' : owed.uri}');
  print('identities ${folded.identities.bound.length} bound');
  print('splitz     ${splitz.billVersion}');
}
```

```rust file=rust/splitz-core/examples/seam.rs
use splitz_core::host::{
    base64url_no_pad, create_bill, join_bill, obligation_for, settle, BillHost, BillLog, Sent,
    CREATOR_KEY_BYTES,
};

struct MyWallet;

impl MyWallet {
    /// The address this wallet is paid at, which its join states.
    fn my_address(&self) -> Option<&str> {
        Some("u1ana000000000000000000")
    }
}

impl BillHost for MyWallet {
    fn me(&self) -> &str {
        "ana"
    }
    // An RFC 3339 instant, not a date type: this crate depends on no calendar
    // library, and `canonical_instant` refuses anything that is not one.
    fn now(&self) -> String {
        "2026-10-28T19:30:00.000Z".to_owned()
    }
    fn random_bytes(&self, n: usize) -> Vec<u8> {
        (0..n).map(|i| (i * 7 + 1) as u8).collect()
    }
    // Synchronous, because this crate pulls in no async runtime and so cannot
    // own the executor a future would need. Block here, where you know yours.
    fn broadcast(&self, uri: &str) -> Sent {
        Sent::sent(format!("tx-{}", uri.len()))
    }
}

fn main() -> splitz_core::Result<()> {
    let host = MyWallet;
    let my_key = base64url_no_pad(&[0u8; CREATOR_KEY_BYTES]);

    let mut log = BillLog::new(&host);
    log.add(vec![create_bill(&host, "Dinner", "EUR", "equal", &my_key)?])?;
    log.add(vec![join_bill(
        &host,
        Some("Ana"),
        host.my_address(),
        None,
        None,
    )?])?;

    let folded = log.fold()?; // §10.3, plus what it set aside
    let owed = obligation_for(&host, &folded)?;
    if let Some(owed) = &owed {
        settle(&host, &mut log, owed)?;
    }

    println!("bill       {}", folded.bill.id);
    println!("entries    {}", log.entries().len());
    println!("setAside   {}", folded.set_aside.len());
    println!(
        "obligation {}",
        owed.as_ref()
            .and_then(|o| o.uri().map(str::to_owned))
            .unwrap_or_else(|| "no rate yet".to_owned())
    );
    println!("identities {} bound", folded.identities.bound.len());
    Ok(())
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
| `me` | the participant id every entry this device writes is authored by. A wallet that publishes an identity key speaks as the id that key derives — `participantIdForKey` — or the key binds nothing (§10.7) |
| `now` | a §9.3 instant. Read when an entry is written, never while folding: §10.2 orders a log by instant, so a fold that read a clock would answer differently for one unchanged entry set |
| `nonce` | sixteen bytes nobody can predict, for §9.4. Two bills opened in one second by one person are one bill when this can be guessed |

Storage, the keychain and the send stay in the wallet's own language. So does
the relay, and each package carries a client for it written in that language —
see "A relay client" below. An entry crosses as the JSON §9.3 canonicalises — it is the
protocol's own wire format and a wallet never inspects one — and everything a
person is shown crosses as a typed record. A refusal crosses as its §12 code.

The three samples below are one program in three languages, and each is a file
a lane runs: `tools/ffi/kotlin.sh`, `tools/ffi/dart.sh` and `tools/ffi/node.sh`
compile and execute exactly these, and `tools/docs/blocks.py` fails if what is
printed here and what is run ever differ.

Two names to expect. The generated module follows each language's convention —
`identityKeyFromSeed` in Kotlin and Dart, `identity_key_from_seed` in
JavaScript — and the JavaScript generator spells record *fields* as Rust does,
so a record reads `withheld_minor_units` there and `withheldMinorUnits` in the
other two.

### Kotlin

```kotlin file=tools/ffi/kotlin/Doc.kt
import uniffi.splitz_ffi.*

/// The facts §15.1 says a wallet owns, for one call. A §9.3 instant and
/// sixteen unpredictable bytes are the wallet's to supply: this library reads
/// no clock (§13) and owns no entropy.
fun facts(me: String, at: String, nonce: Int) =
    HostFacts(me, at, ByteArray(16) { (nonce + it).toByte() })

/// The Ed25519 seed a wallet keeps in the platform keychain, as §9.4 writes a
/// key: 32 bytes, unpadded base64url.
fun seed(first: Int): String = java.util.Base64.getUrlEncoder().withoutPadding()
    .encodeToString(ByteArray(32) { (first + it).toByte() })

fun main() {
    val anaSeed = seed(1)
    val benSeed = seed(90)
    // A wallet that publishes a key speaks as the participant id that key
    // derives (§10.7), or the key binds nothing.
    val anaKey = identityKeyFromSeed(anaSeed)
    val benKey = identityKeyFromSeed(benSeed)
    val ana = participantIdForKey(anaKey)
    val ben = participantIdForKey(benKey)

    // Ana's device writes four entries. Each comes back as the JSON §9.3
    // canonicalises, with §9.5's id already derived and signed; the wallet
    // stores the string and never inspects it.
    val create = createBillEntry(facts(ana, "2026-10-28T19:31:00.000Z", 1),
        "Dinner", "EUR", "equal", anaKey, anaSeed)

    // The bill these entries belong to, read back from the entry that opened
    // it. Every other entry is signed on it (§10.6), and every fold names it,
    // so a create for another bill pushed into the channel cannot make this
    // one unopenable.
    val billId = Regex("\"id\":\"([^\"]+)\"").find(create)!!.groupValues[1]

    val anaLog = listOf(
        create,
        joinBillEntry(facts(ana, "2026-10-28T19:32:00.000Z", 2), billId,
            "Ana", "u1ana", anaKey, listOf(), anaSeed),
        addExpenseEntry(facts(ana, "2026-10-28T19:33:00.000Z", 3), billId,
            "x1", ana, 9000, """{"type":"equal","among":["$ana","$ben"]}""",
            "dinner", anaSeed),
        // §7 snapshots one rate onto the bill, so six devices do not price one
        // dinner six ways. 300000 minor units per ZEC is €3000.00.
        setRateEntry(facts(ana, "2026-10-28T19:34:00.000Z", 4), billId,
            "EUR", 300000, "a fixed feed", anaSeed),
    )

    // Ben's own device writes Ben's join: §10.4 decides what an entry's author
    // may say, and a participant joins for themselves.
    val benLog = listOf(
        joinBillEntry(facts(ben, "2026-10-28T19:35:00.000Z", 5), billId,
            "Ben", "u1ben", benKey, listOf(), benSeed),
    )

    // Merging is how two devices come to agree (§10.2). It is a set union by
    // id, in either direction, any number of times.
    val log = mergeEntries(anaLog, benLog).entries

    val benFacts = facts(ben, "2026-10-28T19:36:00.000Z", 6)
    val folded = foldEntries(benFacts, billId, log)
    println("on the bill: " + folded.bill.participants.joinToString { it.name })
    // Render these. An entry the fold set aside is one a person cannot see
    // otherwise, and its §12 code is what a wallet turns into a sentence.
    println("set aside: " + folded.setAside)

    // Null when the bill carries no rate: an unpriced bill is an ordinary
    // bill, not a refusal.
    val owed = obligationOf(benFacts, billId, log)
        ?: error("a bill with a rate owes something")

    val settlement = owed.settlements.single()
    println("ben pays ${settlement.amount} to ana")
    // The wallet broadcasts this; sending is not the library's (§13.3).
    println("request: ${owed.request.uri}")
    // Never dropped. A request that silently covers three debts of four is
    // indistinguishable, to the payer, from one that covers all of them.
    println("withheld: ${owed.request.withheldMinorUnits}")

    check(settlement.to == ana && settlement.amount == 4500L) {
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
HostFacts facts(String me, String at, int nonce) => HostFacts(
  me: me,
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
  // A wallet that publishes a key speaks as the participant id that key
  // derives (§10.7), or the key binds nothing.
  final anaKey = identityKeyFromSeed(anaSeed);
  final benKey = identityKeyFromSeed(benSeed);
  final ana = participantIdForKey(anaKey);
  final ben = participantIdForKey(benKey);

  // Ana's device writes four entries. Each comes back as the JSON §9.3
  // canonicalises, with §9.5's id already derived and signed; the wallet
  // stores the string and never inspects it.
  final create = createBillEntry(
    facts(ana, '2026-10-28T19:31:00.000Z', 1),
    'Dinner',
    'EUR',
    'equal',
    anaKey,
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
      facts(ana, '2026-10-28T19:32:00.000Z', 2),
      billId,
      'Ana',
      'u1ana',
      anaKey,
      const [],
      anaSeed,
    ),
    addExpenseEntry(
      facts(ana, '2026-10-28T19:33:00.000Z', 3),
      billId,
      'x1',
      ana,
      9000,
      jsonEncode({
        'type': 'equal',
        'among': [ana, ben],
      }),
      'dinner',
      anaSeed,
    ),
    // §7 snapshots one rate onto the bill, so six devices do not price one
    // dinner six ways. 300000 minor units per ZEC is €3000.00.
    setRateEntry(
      facts(ana, '2026-10-28T19:34:00.000Z', 4),
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
      facts(ben, '2026-10-28T19:35:00.000Z', 5),
      billId,
      'Ben',
      'u1ben',
      benKey,
      const [],
      benSeed,
    ),
  ];

  // Merging is how two devices come to agree (§10.2). It is a set union by
  // id, in either direction, any number of times.
  final log = mergeEntries(anaLog, benLog).entries;

  final benFacts = facts(ben, '2026-10-28T19:36:00.000Z', 6);
  final folded = foldEntries(benFacts, billId, log);
  print(
    'on the bill: ${folded.bill.participants.map((p) => p.name).join(', ')}',
  );
  // Render these. An entry the fold set aside is one a person cannot see
  // otherwise, and its §12 code is what a wallet turns into a sentence.
  print('set aside: ${folded.setAside}');

  // Null when the bill carries no rate: an unpriced bill is an ordinary bill,
  // not a refusal.
  final owed = obligationOf(benFacts, billId, log);
  if (owed == null) throw StateError('a bill with a rate owes something');

  final settlement = owed.settlements.single;
  print('ben pays ${settlement.amount} to ana');
  // The wallet broadcasts this; sending is not the library's (§13.3).
  print('request: ${owed.request.uri}');
  // Never dropped. A request that silently covers three debts of four is
  // indistinguishable, to the payer, from one that covers all of them.
  print('withheld: ${owed.request.withheldMinorUnits}');

  if (settlement.to != ana || settlement.amount != 4500) {
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
// (§13) and owns no entropy.
const facts = (me, at, nonce) => ({
  me,
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
// A wallet that publishes a key speaks as the participant id that key derives
// (§10.7), or the key binds nothing.
const anaKey = splitz.identity_key_from_seed(anaSeed);
const benKey = splitz.identity_key_from_seed(benSeed);
const ana = splitz.participant_id_for_key(anaKey);
const ben = splitz.participant_id_for_key(benKey);

// Ana's device writes four entries. Each comes back as the JSON §9.3
// canonicalises, with §9.5's id already derived and signed; the wallet stores
// the string and never inspects it.
const create = splitz.create_bill_entry(facts(ana, "2026-10-28T19:31:00.000Z", 1),
  "Dinner", "EUR", "equal", anaKey, anaSeed);

// The bill these entries belong to, read back from the entry that opened it.
// Every other entry is signed on it (§10.6), and every fold names it, so a
// create for another bill pushed into the channel cannot make this one
// unopenable.
const billId = JSON.parse(create).id;

const anaLog = [
  create,
  splitz.join_bill_entry(facts(ana, "2026-10-28T19:32:00.000Z", 2), billId,
    "Ana", "u1ana", anaKey, [], anaSeed),
  splitz.add_expense_entry(facts(ana, "2026-10-28T19:33:00.000Z", 3), billId,
    "x1", ana, 9000, JSON.stringify({ type: "equal", among: [ana, ben] }), "dinner",
    anaSeed),
  // §7 snapshots one rate onto the bill, so six devices do not price one
  // dinner six ways. 300000 minor units per ZEC is €3000.00.
  splitz.set_rate_entry(facts(ana, "2026-10-28T19:34:00.000Z", 4), billId,
    "EUR", 300000, "a fixed feed", anaSeed),
];

// Ben's own device writes Ben's join: §10.4 decides what an entry's author may
// say, and a participant joins for themselves.
const benLog = [
  splitz.join_bill_entry(facts(ben, "2026-10-28T19:35:00.000Z", 5), billId,
    "Ben", "u1ben", benKey, [], benSeed),
];

// Merging is how two devices come to agree (§10.2). It is a set union by id,
// in either direction, any number of times.
const log = splitz.merge_entries(anaLog, benLog).entries;

const benFacts = facts(ben, "2026-10-28T19:36:00.000Z", 6);
const folded = splitz.fold_entries(benFacts, billId, log);
console.log("on the bill: " + folded.bill.participants.map((p) => p.name).join(", "));
// Render these. An entry the fold set aside is one a person cannot see
// otherwise, and its §12 code is what a wallet turns into a sentence.
console.log("set aside: " + JSON.stringify(folded.set_aside));

// Undefined when the bill carries no rate: an unpriced bill is an ordinary
// bill, not a refusal.
const owed = splitz.obligation_of(benFacts, billId, log);
if (owed === undefined) throw new Error("a bill with a rate owes something");

const settlement = owed.settlements[0];
console.log(`ben pays ${settlement.amount} to ana`);
// The wallet broadcasts this; sending is not the library's (§13.3).
console.log(`request: ${owed.request.uri}`);
// Never dropped. A request that silently covers three debts of four is
// indistinguishable, to the payer, from one that covers all of them.
console.log(`withheld: ${owed.request.withheld_minor_units}`);

if (settlement.to !== ana || Number(settlement.amount) !== 4500) {
  throw new Error(`half of 9000 is 4500 to ana, saw ${settlement.amount} to ${settlement.to}`);
}
if (!owed.request.uri.startsWith("zcash:u1ana")) throw new Error(owed.request.uri);
if (Number(owed.request.withheld_minor_units) !== 0) {
  throw new Error(`${owed.request.withheld_minor_units}`);
}
console.log("DOC RESULT: javascript");
```

### A relay client

Each package carries `SplitzRelay`, a §15.5 client in its own language, beside
the binding: the Android module in package `uniffi.splitz_ffi`, the Swift
package in module `SplitzFFI`, the npm package from its entry point. Sources are
`tools/package/relay/`; the package scripts copy them in.

| language | construct | push | fetch |
|---|---|---|---|
| Kotlin | `SplitzRelay(origin, timeoutMillis = 30_000)` | `push(channel, blobs)`, blocking | `fetch(channel): List<String>`, blocking |
| Swift | `try SplitzRelay(origin:session: = .shared)` | `try await push(channel:blobs:)` | `try await fetch(channel:) -> [String]` |
| JavaScript | `new SplitzRelay(origin, { fetch })` | `await push(channel, blobs)` | `await fetch(channel)` |

The channel is `channelForBill(billId)` (`channel_for_bill` in JavaScript).
Blobs are what `blobsToPush` returns, and what `fetch` returns goes to
`openBlobs`. The wire is `POST <origin>/c/<channel>` with `{"blobs":[…]}` and
`GET <origin>/c/<channel>`, as `tools/relay/server.py` serves it; the binding's
`relayChannelUrl`, `relayPushBody`, `relayPushAnswer` and `relayFetchAnswer`
decide the URL, the body and what an answer means, so every client — and
`splitz_host`'s `HttpSplitsRelay` — refuses alike. A client written for another
network route calls those four directly.

Every failure is the binding's host error (`SplitzException.Host`,
`SplitzError.Host`, `SplitzErrorHost`) with `transient` saying whether a retry
could succeed:

- transient: the relay could not be reached; it answered with anything but
  `{"ok":true}` to a push — whatever the HTTP status, a 4xx included; it
  answered a fetch without a `blobs` list, or with something that is not JSON.
- not transient: an origin carrying a query or a fragment, refused at
  construction; a blob over 65536 characters, refused before anything is sent;
  in JavaScript, a runtime with no `fetch` and none passed.

The Kotlin client blocks, so on Android it runs off the main thread. It uses
`HttpURLConnection`, which API 21 carries; a wallet that sends its traffic over
another route writes its own client over the four functions above. The Swift
client uses the `URLSession` it is given, and the JavaScript one the `fetch` it
is given, the global one of Node 18 and later by default.

`tools/ffi/kotlin.sh`, `tools/ffi/node.sh` and `tools/ffi/swift.sh` each push a
bill's sealed log through a live `tools/relay/server.py` with one client, fetch
it with a second, and open it; each also drives the refusals above.

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

## Nine things the wallet owns

`SPEC.md` §13 lists these so they are not mistaken for gaps. Two carry a
MUST: checking an address is for the network the wallet is on (2), and
surfacing a changed pay-to address (7).

1. **Transport.** §11.3 fixes what a sealed entry looks like and the channel it
   belongs to. Moving blobs — over what, with what retries, stored for how long
   — is the wallet's, as is running the cipher §11.3 names.
2. **Which network a wallet transacts on.** `parseAddress` (§8.6) answers an
   address's network, kind, Unified receivers and whether it can take a memo,
   and `renderUri` refuses a memo it cannot deliver. **A wallet MUST check the
   network it answers is the one it is transacting on**; nothing here knows
   which that is. Decoding a receiver's bytes as a key is the wallet's too,
   when it builds the transaction.
3. **Transaction construction, fees, signing, broadcast.**
4. **The curve operation.** §10.6 fixes the bytes a signature covers; producing
   and checking the Ed25519 signature is the host's.
   `signingMessage(entry, billId)` returns exactly those bytes, so a wallet
   signs and verifies what every other implementation does. The bill is part
   of the message: a signature made on one bill does not verify on another,
   so verify on the bill being folded, never on one taken from the entry.

   **Hand the verifier to the fold**: `foldLog(entries, verify: ...)` takes a
   `bool Function(entry, key)`. With one, a `createBill` whose signature fails
   opens no bill (§10.1), an entry by a participant whose key is bound applies
   only from a copy that verifies against that key, and
   `FoldResult.identities.bound` names who is bound (§10.7). Without one it is
   empty — the fold reports no binding rather than claiming there is none.

   **A participant who publishes a key is named by it.** Their id is
   `participantId(key)` — the digest of the key — so no second key can claim
   them. A wallet whose account publishes an identity key writes every entry
   under that id; a join stating a key under any other id is set aside with
   `participant_id_not_derived`. The creator is the exception: the invite binds
   them to `creatorKey` whatever their id.
5. **Where a private key lives**, and how a participant comes by one.
6. **Storage.**
7. **Surfacing a changed pay-to address.** The fold reports every one.
   **A wallet MUST put a changed address in front of the payer before settling
   to it** — this is what stops a relayed entry silently redirecting a payout.
8. **Honouring an invite's expiry.** §11.1 fixes what `x` looks like and
   parses it. **There is no refusal code for an expired invite, because this
   protocol cannot tell one** — an invite that expired in 1970 parses without
   complaint. `isInviteExpired(invite, nowUnixSeconds)` /
   `is_invite_expired` compares it with the clock you pass, in whole seconds;
   which clock, and whether an expired invite is shown or refused, are yours.
9. **Rate discovery.** §7 specifies what a snapshotted rate does, not where it
   came from. `CoinGeckoZecPrices` (Dart and Rust) is one source, reading
   CoinGecko's `/simple/price` for any currency the ISO 4217 register gives an
   exponent, exactly and rounding halves up; the bindings expose the same as
   `zec_price_request` and `zec_price_from_response`. Where it points is yours:
   CoinGecko itself, or a proxy you run.

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
person says which way it went. `PendingSends` (Dart and Rust host) does exactly
this over your `BillStorage`: `begin` before the wallet is called, `end` with
the outcome, `recordsFor` when a person says the transaction landed, and
`resolve` once the records are on the bill. A note it cannot read still
blocks. If the transaction turns up, `recordSend` / `record_send` writes the
same records `settle` would have, so a payee confirms the same payment either
way.

A wallet on the binding keeps the note as one string per bill, in its own
storage, and the binding carries the rules. `pendingSendBlocks(billId, note)`
answers the send that blocks the bill, or none; a note that does not read
comes back `damaged` and blocks all the same. `pendingSendNote(billId,
obligation, at)` is the string to store **before** the wallet is called, in
the same step that checked — a second send started between the check and the
write is the one this exists to stop. `pendingSendAfter(billId, note, how,
txid, recorded)` is what to store once the wallet answers, or none to delete
it. `pendingSendRecords(facts, billId, entries, note, txid, seed)` writes a
kept send's records once a person says it landed; merge them, then delete the
note. `tools/ffi/kotlin/Consumer.kt` and `tools/ffi/node/consumer.mjs` run
all four.

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
1.234 KWD in KWD. A wallet that renders or accepts major units needs an ISO
4217 register — `currencyExponent` / `currency_exponent` in the host packages
is one, from ISO 4217 List One — and **must refuse a code that register gives
no exponent for**: `XAU` is well-formed and has no minor unit, so a figure
typed in major units means nothing.

**Check what the wallet is about to sign.** A wallet reads a request with its
own ZIP 321 reader, and a reader that keeps only the first of several payments
pays one person while the payer was shown them all. Hand the payments your
reader produced to `checkProposal(uri, outputs)` / `check_proposal` before
signing, and sign only when both `missing` and `unexpected` are empty
(§14.6). It reads back only what this protocol wrote (§8.7).

**A payment you received can be confirmed from what arrived.**
`arrivalsFor(bills, me, received)` / `arrivals_for` matches the transactions
your wallet received against the unconfirmed ZEC records to you, across every
bill at once, so one transaction is evidence once (§14.7). Show the payee each
`arrived` record — its ZEC, its rate, its reference — and on their word write
`walletReceived` confirmations from it. `short` and `unstated` are records the
money does not back.

**Say a refusal in words.** `describeCode(code)` / `describe_code` answers a
short sentence for every §12 code, from `vectors/messages.json`, and nothing
for a code it does not know — show the code itself then.

**Totals across bills.** `totalsAcross(bills, me)` / `totals_across` answers,
per person and currency, what their plans ask each side to pay and what is
recorded and awaiting confirmation. Currencies are never added together, and a
bill that cannot be counted is named, not partly counted.

**An invite as a link.** `renderInviteLink(invite, base)` /
`render_invite_link` puts the invite in the fragment of an https link, which a
chat app shows as tappable and a browser never sends to `base`'s host; every
reader here accepts it (§11.1).

**Check your seams before you ship them.** `checkSecretStore`,
`checkBillStorage`, `checkSplitsRelay` and `checkZecPrices` (Dart and Rust
host) run §15's rules against your own keychain, store, relay and price source
and answer what each did that §15 says it must not. Run them in your test
suite; an empty answer is the only passing one.

**Check your review screen the same way.** `checkPayerReview` /
`check_payer_review` (Dart and Rust host) takes the `PayerObligation` you are
about to send, the `FoldedBill`, the text your review screen shows as a list of
strings, and your words for each §8.5 reason, and answers every §14.2 fact that
text does not show. Amounts must appear as `renderAmount` writes them, the rate
as `rateFigure` / `rate_figure` writes it, and an address whole or by a prefix
of at least 10 characters; matching is case-sensitive.

**Paying somebody by a lower preference.** When a payer cannot use a
recipient's first payout — a swap their wallet cannot reach, cash to somebody
far away — `obligationVia(host, folded, via)` / `obligation_via` renders the
request from the payout the payer chose instead, for that payment alone (§14.8).
`via` maps a participant id to the index of one of their declared payouts.
Who owes what does not move and no entry is written; every other device keeps
the order the recipient declared. Pass the same `via` to `checkPayerReview`,
with your words for "paid by a lower preference", and it asks that every such
recipient be named with them.

A wallet on the binding runs the same check with `checkPayerReview(facts,
billId, entries, obligation, visibleText, reasonWords, via, lowerWords)`: the
bill crosses as its entries, as it does for `obligationOf`, and `obligation` is
the one `obligationOf` or `obligationVia` gave. `renderAmount` and
`rateFigure` write the figures. An
obligation the binding did not write — a request that does not read, outputs
it does not carry, a reason §8.5 does not give — is refused.
`tools/ffi/kotlin/Consumer.kt`, `tools/ffi/node/consumer.mjs`,
`tools/ffi/swift/Consumer.swift` and `tools/ffi/dart/consumer.dart` run it against a screen that shows every fact
and one that leaves out the output's address.

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
set aside with its own id and code and the rest of the bill opens, and an
amendment that cannot be applied is set aside while the entry it corrects
stands. Do not write recovery code for an unopenable bill; write code that
shows `setAside`.

**One debt can be past what a request prices.** A debt summed from many
expenses can exceed what §7.1 converts, or what one §8.1 output carries at a
low rate. `renderObligation` with `skipUnpayable` reports that recipient as
`unpriceable` and carries the rest; without it the whole request is refused.

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
