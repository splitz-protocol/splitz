// A wallet, written against the generated Node package and nothing else.
//
// `splitz_ffi.js` is imported before `load()` runs: it installs the runtime
// hook that registers the callback vtables, and `load()` fires that hook. ESM
// evaluates imports in order, so these two lines are the order that matters —
// calling `load()` first leaves Rust with no vtable and it aborts the process.
import { SplitzSession, SplitzErrorProtocol } from "./splitz_ffi.js";
import { load } from "./splitz_ffi-ffi.js";

load(process.argv[2]);

let failures = 0;
const check = (name, ok, saw) => {
  console.log(`  ${ok ? "PASS" : "FAIL"}  ${name} — ${saw}`);
  if (!ok) failures += 1;
};

/// One device, implementing all seven of SPEC.md §15's interfaces.
/// §15.3 and §15.4 name their operations identically — `read`, `write`,
/// `delete` — so one object cannot implement both. Two, as a wallet has two:
/// a keychain and a store.
class Keychain {
  constructor() { this.values = new Map(); }
  read(k) { return this.values.get(k); }
  write(k, v) { this.values.set(k, v); }
  delete(k) { this.values.delete(k); }
}

class Store {
  constructor() { this.values = new Map(); }
  read(k) { return this.values.get(k); }
  write(k, v) { this.values.set(k, v); }
  delete(k) { this.values.delete(k); }
  keys(prefix) { return [...this.values.keys()].filter((k) => k.startsWith(prefix)); }
  /// Nothing to sweep: a map cannot be half written.
  sweep_unfinished_writes() { return 0; }
}

class Wallet {
  constructor(who, address, relay) {
    this.who = who;
    this.address = address;
    this.relay = relay;
    this.secrets = new Keychain();
    this.storage = new Store();
    this.minute = 0;
    this.counter = 0;
  }

  // §15.1 — who this device speaks as, its clock and its randomness.
  account_id() { return this.who; }
  viewing_key() { return `uview-${this.who}`; }
  now() {
    // A §9.3 instant: UTC, exactly three fractional digits, fixed width, so a
    // log sorts as text on every device (§10.2).
    this.minute += 1;
    const total = 19 * 60 + 30 + this.minute;
    const hh = String(Math.floor(total / 60)).padStart(2, "0");
    const mm = String(total % 60).padStart(2, "0");
    return `2026-10-28T${hh}:${mm}:00.000Z`;
  }
  random_bytes(n) {
    // §9.4 derives a bill's id from a nonce. A shipped wallet uses the
    // platform's own entropy: two bills opened in one second by one person
    // are one bill if this can be guessed.
    this.counter += 1;
    return Uint8Array.from({ length: n }, (_, i) => (this.counter + i) & 0xff);
  }

  // §15.5 — the relay is shared between the two devices.
  push(channel, blobs) { this.relay.push(channel, blobs); }
  fetch(channel) { return this.relay.fetch(channel); }

  // §15.2 — one call takes the whole request.
  send(uri) {
    this.sent = uri;
    return { phase: "Succeeded", txid: `tx-${this.who}-1`, status_message: undefined, error: undefined };
  }
  pay_to_address() { return this.address; }

  // §15.6 — minor units per ZEC, as an integer.
  minor_units_per_zec(currency) { return currency === "EUR" ? 300000 : undefined; }

  // §15.7 — a build that arranges no swaps says so rather than doing nothing.
  tradable_assets() { return []; }
  quote() { throw new Error("this build arranges no swaps"); }
  status_of() { throw new Error("this build arranges no swaps"); }
}

/// A relay holding ciphertext, shared by both devices.
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
const ana = new Wallet("ana", "u1ana", relay);
const ben = new Wallet("ben", "u1ben", relay);
const hers = new SplitzSession(ana, ana.secrets, ana.storage, ana, ana, ana, ana);
const his = new SplitzSession(ben, ben.secrets, ben.storage, ben, ben, ben, ben);

console.log("the seam carries a callback in both directions");
check("the session read an identity out of JavaScript's keychain",
      ana.secrets.values.size > 0, `keys=${[...ana.secrets.values.keys()]}`);
check("the identity key is 43 unpadded base64url characters",
      hers.identity_key().length === 43, hers.identity_key());

console.log("ana opens a bill, splits it and prices it");
const billId = hers.create_bill("Dinner", "EUR", "equal");
hers.join_bill(billId, "Ana", "u1ana");
hers.add_expense(billId, "x1", "ana", 9000,
                 '{"type":"equal","among":["ana","ben"]}', "dinner");
hers.set_rate(billId, "EUR", 300000, "a fixed feed");
check("the bill is held", JSON.stringify(hers.bill_ids()) === JSON.stringify([billId]), billId);

console.log("ben takes it from a scanned payload and joins");
const payload = hers.shareable_bill(billId);
check("the whole bill fits in one code", typeof payload === "string", `${payload?.length} characters`);
check("the scan opened the same bill", his.accept_scan(payload) === billId, billId);
his.join_bill(billId, "Ben", "u1ben");

const folded = his.fold(billId);
check("both people are on the bill", folded.bill.participants.length === 2,
      folded.bill.participants.map((p) => p.id).join(", "));
check("nothing was set aside", folded.set_aside.length === 0, JSON.stringify(folded.set_aside));
check("the expense is nine thousand minor units",
      Number(folded.bill.expenses[0].amount) === 9000, `${folded.bill.expenses[0].amount}`);

console.log("ben owes half of it, and settles");
const owed = his.obligation(billId, []);
check("ben has an obligation", owed !== undefined, owed?.request?.uri ?? "none");
check("it is four and a half thousand to ana",
      owed.settlements[0].to === "ana" && Number(owed.settlements[0].amount) === 4500,
      `${owed.settlements[0].to} ${owed.settlements[0].amount}`);
check("the request is a ZIP 321 URI naming ana's address",
      owed.request.uri.startsWith("zcash:u1ana"), owed.request.uri);

const settled = his.settle(billId, []);
check("the wallet was handed the whole request in one call",
      ben.sent === owed.request.uri, ben.sent ?? "nothing");
check("one payment was recorded", settled.sent && Number(settled.recorded) === 1,
      `sent=${settled.sent} recorded=${settled.recorded} txid=${settled.txid}`);

console.log("a record is a claim until the payee confirms");
his.sync(billId);
hers.sync(billId);
const seen = hers.fold(billId);
check("ana sees the payment", seen.bill.payments.length === 1, JSON.stringify(seen.bill.payments.map((p) => p.id)));
check("and it is not confirmed", seen.bill.confirmed_payments.length === 0,
      JSON.stringify(seen.bill.confirmed_payments));
check("so ben is asked for nothing twice", his.obligation(billId, []).settlements.length === 0,
      JSON.stringify(his.obligation(billId, []).settlements));

hers.confirm_payment(billId, seen.bill.payments[0].id, "recipientConfirmed", undefined);
hers.sync(billId);
his.sync(billId);
const after = his.obligation(billId, []);
check("once confirmed, the debt is gone",
      after.settlements.length === 0 && after.awaiting.length === 0,
      JSON.stringify({ settlements: after.settlements.length, awaiting: after.awaiting.length }));

console.log("a refusal crosses as a §12 code");
try {
  hers.accept_scan("not a bill");
  check("a scan that is nothing is refused", false, "it was not");
} catch (e) {
  check("a scan that is nothing is refused by its code",
        e instanceof SplitzErrorProtocol && typeof e.code === "string",
        e.code ?? String(e));
}

console.log(failures === 0
  ? `CONSUMER RESULT: the binding carries a whole bill, ${failures} failures`
  : `CONSUMER RESULT: ${failures} check(s) failed`);
process.exit(failures === 0 ? 0 : 1);
