// The smallest JavaScript wallet that renders a payment request.
//
// INTEGRATING.md quotes everything below the marker verbatim;
// `tools/docs/blocks.py` fails when the two drift, and `tools/ffi/node.sh`
// runs it, so the document's sample is a sample that executed.
// docs:begin
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
