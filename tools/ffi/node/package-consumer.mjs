// A wallet that reached this library the way every other npm consumer will:
// `npm install splitz-ffi`, then `import * as splitz from "splitz-ffi"`.
//
// The difference from `consumer.mjs` is the whole point of this file. There is
// no relative path into a generated directory and no `load()` call naming a
// `.dylib` on disk: the package carries its own native library and resolves it
// on import. A consumer that has to be handed a library path is not a package,
// it is a directory.
//
// It runs from outside the package, in a directory whose only dependency is
// the packed tarball, and drives one bill from opening it to a confirmed
// payment.
import * as splitz from "splitz-ffi";

let failures = 0;
const check = (name, ok, saw) => {
  console.log(`  ${ok ? "PASS" : "FAIL"}  ${name} — ${saw}`);
  if (!ok) failures += 1;
};

/// One device: its log, its clock, its randomness. This library reads no clock
/// (§13) and owns no entropy, so a wallet supplies both.
class Device {
  constructor(me, payTo, seedByte) {
    this.me = me;
    this.payTo = payTo;
    this.entries = [];
    this.minute = 0;
    // The Ed25519 seed this account signs with, as §9.4 writes a key: 32
    // bytes, unpadded base64url. A shipped wallet keeps this in the platform
    // keychain.
    this.seed = Buffer.from(
      Array.from({ length: 32 }, (_, i) => (seedByte + i) & 0xff),
    ).toString("base64url");
    this.seedByte = seedByte;
  }

  /// A §9.3 instant: UTC, exactly three fractional digits, fixed width.
  now() {
    this.minute += 1;
    const total = 19 * 60 + 30 + this.minute;
    const hh = String(Math.floor(total / 60)).padStart(2, "0");
    const mm = String(total % 60).padStart(2, "0");
    return `2026-10-28T${hh}:${mm}:00.000Z`;
  }

  /// §9.4 derives a bill's id from this. A shipped wallet uses the platform's
  /// own entropy.
  nonce() {
    return Uint8Array.from(
      { length: 16 },
      (_, i) => (this.seedByte + this.minute + i) & 0xff,
    );
  }

  facts() {
    return { me: this.me, pay_to: this.payTo, now: this.now(), nonce: this.nonce() };
  }

  add(entry) {
    this.entries = splitz.merge_entries(this.entries, [entry]).entries;
  }

  take(other) {
    this.entries = splitz.merge_entries(this.entries, other.entries).entries;
  }
}

const ana = new Device("ana", "u1ana", 1);
const ben = new Device("ben", "u1ben", 90);
const anaKey = splitz.identity_key_from_seed(ana.seed);
const benKey = splitz.identity_key_from_seed(ben.seed);

console.log(`the package resolved its own native library on import`);
check("the surface is callable with no library path given",
      anaKey.length === 43, anaKey);

console.log("ana opens a bill, and both join it");
const create = splitz.create_bill_entry(ana.facts(), "Dinner", "EUR", "equal",
    anaKey, ana.seed);
ana.add(create);
const billId = JSON.parse(create).id;
ana.add(splitz.join_bill_entry(ana.facts(), "Ana", "u1ana", anaKey, ana.seed));
ben.take(ana);
ben.add(splitz.join_bill_entry(ben.facts(), "Ben", "u1ben", benKey, ben.seed));
ana.take(ben);

console.log("ana adds an expense they share, and prices it");
ana.add(splitz.add_expense_entry(ana.facts(), "x1", "ana", 9000,
    '{"type":"equal","among":["ana","ben"]}', "dinner", ana.seed));
// §7 snapshots one rate onto the bill, so two devices do not price one dinner
// two ways. 300000 minor units per ZEC is €3000.00.
ana.add(splitz.set_rate_entry(ana.facts(), "EUR", 300000, "a fixed feed", ana.seed));

const folded = splitz.fold_entries(ana.facts(), ana.entries);
check("the bill is the one ana opened", folded.bill.id === billId, folded.bill.id);
check("both people are on it", folded.bill.participants.length === 2,
      folded.bill.participants.map((p) => p.id).join(", "));
check("nothing was set aside", folded.set_aside.length === 0,
      JSON.stringify(folded.set_aside));

console.log("ben owes half of it");
ben.take(ana);
const owed = splitz.obligation_of(ben.facts(), ben.entries, []);
check("ben has an obligation", owed !== undefined, owed?.request?.uri ?? "none");
check("it is four and a half thousand to ana",
      owed.settlements[0].to === "ana" && Number(owed.settlements[0].amount) === 4500,
      `${owed.settlements[0].to} ${owed.settlements[0].amount}`);
check("the request is a ZIP 321 URI naming ana's address",
      owed.request.uri.startsWith("zcash:u1ana"), owed.request.uri);
check("nothing was withheld from the request",
      Number(owed.request.withheld_minor_units) === 0,
      `${owed.request.withheld_minor_units}`);

console.log("the wallet sends, then records what §14.3 allows");
const records = splitz.payment_entries_for_send(ben.facts(), owed, "tx-ben-1", ben.seed);
check("one record, for what the request carried", records.length === 1,
      `${records.length} record(s)`);
for (const record of records) ben.add(record);

ana.take(ben);
const afterPayment = splitz.fold_entries(ana.facts(), ana.entries);
check("ana sees the payment", afterPayment.bill.payments.length === 1,
      JSON.stringify(afterPayment.bill.payments.map((p) => p.id)));
check("and it is not confirmed", afterPayment.bill.confirmed_payments.length === 0,
      JSON.stringify(afterPayment.bill.confirmed_payments));
check("so ben is asked for nothing twice",
      splitz.obligation_of(ben.facts(), ben.entries, []).settlements.length === 0,
      JSON.stringify(splitz.obligation_of(ben.facts(), ben.entries, []).settlements));

// A payee confirms a payment they can see, by the id the bill carries. One
// transaction paying several people writes one record each, so the id is not
// the transaction's — the transaction is in `reference`.
const toConfirm = afterPayment.bill.payments[0].id;
ana.add(splitz.confirm_payment_entry(ana.facts(), toConfirm, "recipientConfirmed",
    undefined, ana.seed));
ben.take(ana);
const settled = splitz.obligation_of(ben.facts(), ben.entries, []);
check("once confirmed, the debt is gone",
      settled.settlements.length === 0 && settled.awaiting.length === 0,
      `settlements=${settled.settlements.length} awaiting=${settled.awaiting.length}`);

console.log("a refusal still crosses as a §12 code");
check("a scan that is nothing is refused by its code",
      typeof splitz.read_scanned("not a bill").refused_code === "string",
      `${splitz.read_scanned("not a bill").refused_code}`);

console.log(failures === 0
  ? `PACKAGE CONSUMER RESULT: an installed npm package drives a whole bill, ${failures} failures`
  : `PACKAGE CONSUMER RESULT: ${failures} check(s) failed`);
process.exit(failures === 0 ? 0 : 1);
