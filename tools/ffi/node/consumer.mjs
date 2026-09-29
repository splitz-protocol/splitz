// A JavaScript wallet over the generated Node package.
//
// It holds its own log, its own clock and its own randomness, and hands the
// library the facts it owns. Nothing calls back into JavaScript: §15's seven
// interfaces would be seven sets of callbacks across a foreign boundary, and
// this is the language with no static types to catch one going wrong.
import * as splitz from "./splitz_ffi.js";
import { load } from "./splitz_ffi-ffi.js";
import { SplitzRelay } from "./splitz_relay.js";

// The library, a running relay's origin, and an origin nothing answers.
const [library, origin, downOrigin] = process.argv.slice(2);
load(library);

let failures = 0;
const check = (name, ok, saw) => {
  console.log(`  ${ok ? "PASS" : "FAIL"}  ${name} — ${saw}`);
  if (!ok) failures += 1;
};

/// One device: its log, its clock, its randomness.
class Device {
  constructor(seedByte) {
    this.entries = [];
    this.minute = 0;
    // The Ed25519 seed this account signs with, as §9.4 writes a key. A
    // shipped wallet keeps this in the platform keychain.
    this.seed = Buffer.from(
      Array.from({ length: 32 }, (_, i) => (seedByte + i) & 0xff),
    ).toString("base64url");
    this.seedByte = seedByte;
    // The key this account publishes, and the participant id it derives
    // (§10.7): a wallet that publishes a key writes every entry under the id
    // that key derives, or the key binds nothing.
    this.key = splitz.identity_key_from_seed(this.seed);
    this.me = splitz.participant_id_for_key(this.key);
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
    return { me: this.me, now: this.now(), nonce: this.nonce() };
  }

  add(entry) {
    this.entries = splitz.merge_entries(this.entries, [entry]).entries;
  }

  take(other) {
    this.entries = splitz.merge_entries(this.entries, other.entries).entries;
  }
}

/// What `run` threw as a host refusal, or undefined when it threw nothing.
const refusal = async (run) => {
  try {
    await run();
    return undefined;
  } catch (e) {
    if (e instanceof splitz.SplitzErrorHost) return e;
    throw e;
  }
};

const ana = new Device(1);
const ben = new Device(90);
const anaKey = ana.key;
const benKey = ben.key;

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
ana.add(splitz.join_bill_entry(ana.facts(), billId, "Ana", "u1ana", anaKey, [], ana.seed));

console.log("ana shares it, and ben takes it from the code");
const billKey = "-_" + "A".repeat(41);
const payload = splitz.shareable_bill_payload(ana.facts(), billId, ana.entries, billKey);
check("the whole bill fits in one code", typeof payload === "string",
      `${payload?.length} characters`);
const scanned = splitz.read_scanned(payload);
check("the scan names the same bill", scanned.bill_id === billId, `${scanned.bill_id}`);
ben.entries = splitz.merge_entries(ben.entries, scanned.entries).entries;
ben.add(splitz.join_bill_entry(ben.facts(), billId, "Ben", "u1ben", benKey, [], ben.seed));

console.log("the two logs move through a relay that holds only ciphertext");
const channel = splitz.channel_for_bill(billId);
check("the channel is the bill id's hash, never the id",
      channel !== billId && channel.length === 64, channel.slice(0, 16) + "…");
// Two clients, as two devices hold them: what one pushes the other fetches.
const benRelay = new SplitzRelay(origin);
const anaRelay = new SplitzRelay(origin);
// Pushed as they are held: each was signed when it was written.
const pushed = splitz.blobs_to_push(ben.entries, billKey);
await benRelay.push(channel, pushed);
await benRelay.push(channel, pushed);
const fetched = await anaRelay.fetch(channel);
check("another client fetches every blob pushed, once, though it was pushed twice",
      JSON.stringify([...fetched].sort()) === JSON.stringify([...pushed].sort()),
      `${fetched.length} of ${pushed.length}`);
const opened = splitz.open_blobs(fetched, billKey);
check("every blob opened", Number(opened.unopenable) === 0, `unopenable=${opened.unopenable}`);
// Merged by entry id: what a blob opens to is the entry, not necessarily the
// same text, so a round trip adds no entry to the log it came from.
const roundTrip = splitz.merge_entries(ben.entries, opened.entries);
check("to the entries ben holds",
      opened.entries.length === ben.entries.length &&
        roundTrip.entries.length === ben.entries.length && roundTrip.refused.length === 0,
      `${opened.entries.length} opened, ${roundTrip.entries.length} after merging into ${ben.entries.length}`);
ana.entries = splitz.merge_entries(ana.entries, opened.entries).entries;

console.log("a relay that fails says whether retrying could succeed");
const down = await refusal(() => new SplitzRelay(downOrigin).fetch(channel));
check("a relay that is down raises, transient", down?.transient === true, `${down?.detail}`);
const notChannel = await refusal(() => anaRelay.push("not-a-channel", ["x"]));
check("a 4xx the relay answers raises, transient, as every client classifies it",
      notChannel?.transient === true && notChannel.detail.includes("refused"),
      `${notChannel?.detail}`);
const oversize = await refusal(() => anaRelay.push(channel, ["x".repeat(64 * 1024 + 1)]));
check("a blob over the cap raises before it is sent, not transient",
      oversize?.transient === false, `${oversize?.detail}`);
const queried = await refusal(() => new SplitzRelay(`${origin}?t=1`));
check("an origin carrying a query raises, not transient",
      queried?.transient === false, `${queried?.detail}`);
const noFetch = await refusal(() => new SplitzRelay(origin, { fetch: 42 }));
check("a runtime with no fetch is named, not transient",
      noFetch?.transient === false, `${noFetch?.detail}`);

console.log("ana adds an expense they share, and prices it");
ana.add(splitz.add_expense_entry(ana.facts(), billId, "x1", ana.me, 9000,
    JSON.stringify({ type: "equal", among: [ana.me, ben.me] }), "dinner", ana.seed));
ana.add(splitz.set_rate_entry(ana.facts(), billId, "EUR", 300000, "a fixed feed", ana.seed));

const folded = splitz.fold_entries(ana.facts(), billId, ana.entries);
check("both people are on the bill", folded.bill.participants.length === 2,
      folded.bill.participants.map((p) => p.id).join(", "));
check("nothing was set aside", folded.set_aside.length === 0, JSON.stringify(folded.set_aside));

console.log("ben owes half of it");
ben.take(ana);
const owed = splitz.obligation_of(ben.facts(), billId, ben.entries);
check("ben has an obligation", owed !== undefined, owed?.request?.uri ?? "none");
check("it is four and a half thousand to ana",
      owed.settlements[0].to === ana.me && Number(owed.settlements[0].amount) === 4500,
      `${owed.settlements[0].to} ${owed.settlements[0].amount}`);
check("the request is a ZIP 321 URI naming ana's address",
      owed.request.uri.startsWith("zcash:u1ana"), owed.request.uri);

console.log("the wallet writes the send down before it sends (§14.3)");
// One string per bill, kept where it outlives the process. The send is the
// wallet's; these say what the note becomes.
const txid = "ab".repeat(32);
check("with no note, nothing blocks a send",
      splitz.pending_send_blocks(billId, undefined) === undefined, "none");
let note = splitz.pending_send_note(billId, owed, ben.now());
check("with the note stored, a second send from this bill is blocked",
      splitz.pending_send_blocks(billId, note)?.damaged === false,
      JSON.stringify(splitz.pending_send_blocks(billId, note)?.uri));
check("a refused send takes its note with it",
      splitz.pending_send_after(billId, note, splitz.SendEnded.Refused, undefined, false) === undefined,
      "cleared");
check("so does one that reached the network once its records are on the bill",
      splitz.pending_send_after(billId, note, splitz.SendEnded.ReachedNetwork, txid, true) === undefined,
      "cleared");
note = splitz.pending_send_after(billId, note, splitz.SendEnded.Unresolved, txid, false);
check("one built and not broadcast keeps its note, naming the transaction",
      splitz.pending_send_blocks(billId, note)?.txid === txid,
      `${splitz.pending_send_blocks(billId, note)?.txid}`);
check("and the next send is still blocked",
      splitz.pending_send_blocks(billId, note) !== undefined, "blocked");
check("a note that does not read blocks as well",
      splitz.pending_send_blocks(billId, "{not json")?.damaged === true, "damaged");
const lost = await refusal(() =>
  splitz.pending_send_records(ben.facts(), billId, ben.entries, "{not json", txid, ben.seed));
check("and nothing is recorded from it", lost !== undefined, `${lost?.detail}`);

console.log("a person says the send landed; its records come from the note alone");
const records = splitz.pending_send_records(ben.facts(), billId, ben.entries, note, txid, ben.seed);
check("one record, for what the request carried", records.length === 1,
      `${records.length} record(s)`);
const paymentId = `"${txid}:${ana.me}"`;
check("under the payment id a send that succeeded records",
      records[0].includes(paymentId) &&
        splitz.payment_entries_for_send(ben.facts(), billId, owed, txid, ben.seed)[0]
          .includes(paymentId),
      `${txid}:${ana.me.slice(0, 8)}…`);
for (const record of records) ben.add(record);
check("asked again, nothing is recorded twice",
      splitz.pending_send_records(ben.facts(), billId, ben.entries, note, txid, ben.seed)
        .length === 0,
      "none");
note = undefined; // deleted, now the records are on the bill
check("with the note deleted, the bill can be sent from again",
      splitz.pending_send_blocks(billId, note) === undefined, "none");

ana.take(ben);
const afterPayment = splitz.fold_entries(ana.facts(), billId, ana.entries);
check("ana sees the payment", afterPayment.bill.payments.length === 1,
      JSON.stringify(afterPayment.bill.payments.map((p) => p.id)));
check("and it is not confirmed", afterPayment.bill.confirmed_payments.length === 0,
      JSON.stringify(afterPayment.bill.confirmed_payments));
check("so ben is asked for nothing twice",
      splitz.obligation_of(ben.facts(), billId, ben.entries).settlements.length === 0,
      JSON.stringify(splitz.obligation_of(ben.facts(), billId, ben.entries).settlements));
const totals = splitz.totals_of(ben.facts(), [{ bill_id: billId, entries: ben.entries }]);
const withAna = totals.standings[0];
check("across bills, ben still owes ana, with the payment on its way",
      totals.standings.length === 1 && withAna.with_id === ana.me &&
        Number(withAna.owed_by_me) === 4500 && Number(withAna.sent_awaiting) === 4500 &&
        totals.uncounted.size === 0,
      `${withAna?.with_id} owed=${withAna?.owed_by_me} sent=${withAna?.sent_awaiting}`);

console.log("before signing, the wallet holds what it read against the request");
const anaAddress = folded.bill.participants.find((p) => p.id === ana.me).pay_to;
const sent = owed.request.payments[0].zatoshi;
const same = splitz.check_proposal(owed.request.uri, [{ address: anaAddress, zatoshi: sent }]);
check("what the request asks is what would be signed",
      same.missing.length === 0 && same.unexpected.length === 0,
      `missing=${same.missing.length} unexpected=${same.unexpected.length}`);
const dropped = splitz.check_proposal(owed.request.uri, []);
check("a reader that dropped the payment is caught",
      dropped.missing.length === 1 && dropped.missing[0].address === anaAddress,
      `missing=${dropped.missing.map((m) => m.address)}`);

console.log("ana's wallet saw the transaction arrive");
const arrivals = splitz.arrivals_of(ana.facts(), [{ bill_id: billId, entries: ana.entries }],
    [{ txid, zatoshi: sent }]);
const arrival = arrivals.arrived[0];
check("the payment is proposed for confirmation",
      arrivals.arrived.length === 1 && arrival.payment.id === afterPayment.bill.payments[0].id,
      `arrived=${arrivals.arrived.length} short=${arrivals.short.length}`);

// A payee confirms a payment they can see, by the id the bill carries. One
// transaction paying several people writes one record each, so the id is not
// the transaction's — the transaction is in `reference`.
ana.add(splitz.confirm_payment_entry(ana.facts(), billId, arrival.payment.id, "walletReceived",
    arrival.txid, arrival.record, ana.seed));
ben.take(ana);
const settled = splitz.obligation_of(ben.facts(), billId, ben.entries);
check("once confirmed, the debt is gone",
      settled.settlements.length === 0 && settled.awaiting.length === 0,
      `settlements=${settled.settlements.length} awaiting=${settled.awaiting.length}`);

console.log("a refusal crosses as a §12 code");
check("a scan that is nothing is refused by its code",
      typeof splitz.read_scanned("not a bill").refused_code === "string",
      `${splitz.read_scanned("not a bill").refused_code}`);
check("and a code has a sentence a person can read",
      typeof splitz.describe_code(splitz.read_scanned("not a bill").refused_code) === "string" &&
        splitz.describe_code("self_payment") === "You can't pay yourself." &&
        splitz.describe_code("no_such_code") === undefined,
      `${splitz.describe_code("self_payment")}`);

console.log("an address is decoded before it is paid or published");
const unified = splitz.parse_address("u1ay3aawlldjrmxqnjf5medr5ma6p3acnet464ht8lmwplq5cd3ugytcmlf96rrmtgwldc75x94qn4n8pgen36y8tywlq6yjk7lkf3fa8wzjrav8z2xpxqnrnmjxh8tmz6jhfh425t7f3vy6p4pd3zmqayq49efl2c4xydc0gszg660q9p");
check("a unified address, on main, that takes a memo",
      unified.network === "main" && unified.kind === "unified" &&
        JSON.stringify([...unified.receivers]) === "[2,3]" && unified.can_receive_memo === true,
      JSON.stringify({ ...unified, receivers: [...unified.receivers] }));
const transparent = splitz.parse_address("t1Hsc1LR8yKnbbe3twRp88p6vFfC5t7DLbs");
check("a transparent one takes none",
      transparent.kind === "p2pkh" && transparent.can_receive_memo === false, transparent.kind);
let refusedAddress;
try { splitz.parse_address("u1ana"); } catch (e) { refusedAddress = e.code; }
check("anything else is refused by its code", refusedAddress === "address_invalid",
      `${refusedAddress}`);

console.log("a price, asked and read without a callback");
const priceOrigin = "https://api.coingecko.com/api/v3";
check("the request names one currency",
      splitz.zec_price_request(priceOrigin, "EUR") ===
        "https://api.coingecko.com/api/v3/simple/price?ids=zcash&vs_currencies=eur",
      `${splitz.zec_price_request(priceOrigin, "EUR")}`);
check("a code with no exponent is not asked for",
      splitz.zec_price_request(priceOrigin, "XAU") === undefined, "XAU");
const eur = splitz.zec_price_from_response('{"zcash":{"eur":1222.41}}', "EUR");
check("the answer reads as minor units, exactly", Number(eur) === 122241, `${eur}`);

console.log(failures === 0
  ? `CONSUMER RESULT: javascript drives a whole bill with no callbacks, ${failures} failures`
  : `CONSUMER RESULT: ${failures} check(s) failed`);
process.exit(failures === 0 ? 0 : 1);
