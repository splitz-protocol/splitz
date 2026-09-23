// A JavaScript wallet over the generated Node package.
//
// It holds its own log, its own clock and its own randomness, and hands the
// library the facts it owns. Nothing calls back into JavaScript: §15's seven
// interfaces would be seven sets of callbacks across a foreign boundary, and
// this is the language with no static types to catch one going wrong.
import * as splitz from "./splitz_ffi.js";
import { load } from "./splitz_ffi-ffi.js";

load(process.argv[2]);

let failures = 0;
const check = (name, ok, saw) => {
  console.log(`  ${ok ? "PASS" : "FAIL"}  ${name} — ${saw}`);
  if (!ok) failures += 1;
};

/// One device: its log, its clock, its randomness.
class Device {
  constructor(me, payTo, seedByte) {
    this.me = me;
    this.payTo = payTo;
    this.entries = [];
    this.minute = 0;
    // The Ed25519 seed this account signs with, as §9.4 writes a key. A
    // shipped wallet keeps this in the platform keychain.
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

/// A relay holding ciphertext, shared by both devices. It holds no key.
class Relay {
  constructor() { this.channels = new Map(); }
  push(channel, blobs) {
    const held = this.channels.get(channel) ?? [];
    for (const blob of blobs) if (!held.includes(blob)) held.push(blob);
    this.channels.set(channel, held);
  }
  fetch(channel) { return this.channels.get(channel) ?? []; }
}

const relay = new Relay();
const ana = new Device("ana", "u1ana", 1);
const ben = new Device("ben", "u1ben", 90);
const anaKey = splitz.identity_key_from_seed(ana.seed);
const benKey = splitz.identity_key_from_seed(ben.seed);

console.log("a wallet passes facts, not callbacks");
check("an identity key is 43 unpadded base64url characters", anaKey.length === 43, anaKey);
check("a key of the wrong length is named, not accepted",
      splitz.bill_key_problem("AAAA") === "wrong_length" &&
      splitz.bill_key_problem(anaKey) === undefined,
      `${splitz.bill_key_problem("AAAA")}`);

console.log("ana opens a bill and joins it");
const create = splitz.create_bill_entry(ana.facts(), "Dinner", "EUR", "equal", anaKey, ana.seed);
ana.add(create);
const billId = JSON.parse(create).id;
ana.add(splitz.join_bill_entry(ana.facts(), "Ana", "u1ana", anaKey, ana.seed));

console.log("ana shares it, and ben takes it from the code");
const billKey = "-_" + "A".repeat(41);
const payload = splitz.shareable_bill_payload(ana.facts(), billId, ana.entries, billKey);
check("the whole bill fits in one code", typeof payload === "string",
      `${payload?.length} characters`);
const scanned = splitz.read_scanned(payload);
check("the scan names the same bill", scanned.bill_id === billId, `${scanned.bill_id}`);
ben.entries = splitz.merge_entries(ben.entries, scanned.entries).entries;
ben.add(splitz.join_bill_entry(ben.facts(), "Ben", "u1ben", benKey, ben.seed));

console.log("the two logs move through a relay that holds only ciphertext");
const channel = splitz.channel_for_bill(billId);
check("the channel is the bill id's hash, never the id",
      channel !== billId && channel.length === 64, channel.slice(0, 16) + "…");
relay.push(channel, splitz.blobs_to_push(ben.entries, billKey, ben.seed, "ben"));
const opened = splitz.open_blobs(relay.fetch(channel), billKey);
check("every blob opened", Number(opened.unopenable) === 0, `unopenable=${opened.unopenable}`);
ana.entries = splitz.merge_entries(ana.entries, opened.entries).entries;

console.log("ana adds an expense they share, and prices it");
ana.add(splitz.add_expense_entry(ana.facts(), "x1", "ana", 9000,
    '{"type":"equal","among":["ana","ben"]}', "dinner", ana.seed));
ana.add(splitz.set_rate_entry(ana.facts(), "EUR", 300000, "a fixed feed", ana.seed));

const folded = splitz.fold_entries(ana.facts(), billId, ana.entries);
check("both people are on the bill", folded.bill.participants.length === 2,
      folded.bill.participants.map((p) => p.id).join(", "));
check("nothing was set aside", folded.set_aside.length === 0, JSON.stringify(folded.set_aside));

console.log("ben owes half of it");
ben.take(ana);
const owed = splitz.obligation_of(ben.facts(), billId, ben.entries, []);
check("ben has an obligation", owed !== undefined, owed?.request?.uri ?? "none");
check("it is four and a half thousand to ana",
      owed.settlements[0].to === "ana" && Number(owed.settlements[0].amount) === 4500,
      `${owed.settlements[0].to} ${owed.settlements[0].amount}`);
check("the request is a ZIP 321 URI naming ana's address",
      owed.request.uri.startsWith("zcash:u1ana"), owed.request.uri);

console.log("the wallet sends, then records what §14.3 allows");
const records = splitz.payment_entries_for_send(ben.facts(), owed, "tx-ben-1", ben.seed);
check("one record, for what the request carried", records.length === 1,
      `${records.length} record(s)`);
for (const record of records) ben.add(record);

ana.take(ben);
const afterPayment = splitz.fold_entries(ana.facts(), billId, ana.entries);
check("ana sees the payment", afterPayment.bill.payments.length === 1,
      JSON.stringify(afterPayment.bill.payments.map((p) => p.id)));
check("and it is not confirmed", afterPayment.bill.confirmed_payments.length === 0,
      JSON.stringify(afterPayment.bill.confirmed_payments));
check("so ben is asked for nothing twice",
      splitz.obligation_of(ben.facts(), billId, ben.entries, []).settlements.length === 0,
      JSON.stringify(splitz.obligation_of(ben.facts(), billId, ben.entries, []).settlements));

// A payee confirms a payment they can see, by the id the bill carries. One
// transaction paying several people writes one record each, so the id is not
// the transaction's — the transaction is in `reference`.
const toConfirm = afterPayment.bill.payments[0].id;
ana.add(splitz.confirm_payment_entry(ana.facts(), toConfirm, "recipientConfirmed",
    undefined, ana.seed));
ben.take(ana);
const settled = splitz.obligation_of(ben.facts(), billId, ben.entries, []);
check("once confirmed, the debt is gone",
      settled.settlements.length === 0 && settled.awaiting.length === 0,
      `settlements=${settled.settlements.length} awaiting=${settled.awaiting.length}`);

console.log("a refusal crosses as a §12 code");
check("a scan that is nothing is refused by its code",
      typeof splitz.read_scanned("not a bill").refused_code === "string",
      `${splitz.read_scanned("not a bill").refused_code}`);

console.log(failures === 0
  ? `CONSUMER RESULT: javascript drives a whole bill with no callbacks, ${failures} failures`
  : `CONSUMER RESULT: ${failures} check(s) failed`);
process.exit(failures === 0 ? 0 : 1);
