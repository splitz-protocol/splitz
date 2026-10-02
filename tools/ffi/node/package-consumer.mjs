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
  constructor(seedByte) {
    this.entries = [];
    this.minute = 0;
    // The Ed25519 seed this account signs with, as §9.4 writes a key: 32
    // bytes, unpadded base64url. A shipped wallet keeps this in the platform
    // keychain.
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

const ana = new Device(1);
const ben = new Device(90);
const anaKey = ana.key;
const benKey = ben.key;

console.log(`the package resolved its own native library on import`);
check("the surface is callable with no library path given",
      anaKey.length === 43, anaKey);
check("the relay client is exported beside the binding",
      typeof splitz.SplitzRelay === "function", typeof splitz.SplitzRelay);
let refusedOrigin;
try {
  new splitz.SplitzRelay("https://relay.example?t=1");
} catch (e) {
  refusedOrigin = e;
}
check("and decides through the package's own binding",
      refusedOrigin instanceof splitz.SplitzErrorHost && refusedOrigin.transient === false,
      `${refusedOrigin?.detail}`);

console.log("ana opens a bill, and both join it");
// The key is minted first: the create entry commits to it (§9.4).
const billKey = splitz.new_bill_key({ bytes: crypto.getRandomValues(new Uint8Array(32)) });
const create = splitz.create_bill_entry(ana.facts(), "Dinner", "EUR", "equal",
    anaKey, billKey, ana.seed);
ana.add(create);
const billId = JSON.parse(create).id;
ana.add(splitz.join_bill_entry(ana.facts(), billId, "Ana", "u1ana", anaKey, [], ana.seed));
ben.take(ana);
ben.add(splitz.join_bill_entry(ben.facts(), billId, "Ben", "u1ben", benKey, [], ben.seed));
ana.take(ben);

console.log("ana adds an expense they share, and prices it");
const dinner = splitz.add_expense_entry(ana.facts(), billId, "x1", ana.me, 9000,
    JSON.stringify({ type: "equal", among: [ana.me, ben.me] }), "dinner", ana.seed);
ana.add(dinner);
// §7 snapshots one rate onto the bill, so two devices do not price one dinner
// two ways. 300000 minor units per ZEC is €3000.00.
ana.add(splitz.set_rate_entry(ana.facts(), billId, "EUR", 300000, "a fixed feed", ana.seed));

const folded = splitz.fold_entries(ana.facts(), billId, ana.entries);
check("the bill is the one ana opened", folded.bill.id === billId, folded.bill.id);
check("both people are on it", folded.bill.participants.length === 2,
      folded.bill.participants.map((p) => p.id).join(", "));
check("nothing was set aside", folded.set_aside.length === 0,
      JSON.stringify(folded.set_aside));

console.log("ana plans taking ben off the bill (§10.8)");
const unpaid = splitz.plan_removal(ana.facts(), billId, ana.entries, ben.me, ana.me);
const onlyAna = JSON.stringify({ type: "equal", among: [ana.me] });
check("her expense is offered, split without him, and nothing blocks it",
      unpaid.blockers.length === 0 && unpaid.edits.length === 1 &&
        unpaid.edits[0].entry_id === JSON.parse(dinner).id &&
        JSON.stringify(JSON.parse(unpaid.edits[0].split_json)) === onlyAna,
      JSON.stringify(unpaid.edits.map((e) => e.split_json)));
check("and the plan still stands while the bill has not moved",
      splitz.same_removal_plan(unpaid,
        splitz.plan_removal(ana.facts(), billId, ana.entries, ben.me, ana.me)) ===
        splitz.RemovalPlanStanding.Stands,
      "stands");
const without = splitz.split_without(
    JSON.stringify({ type: "equal", among: [ana.me, ben.me] }), ben.me);
check("a split without him crosses as JSON",
      typeof without === "string" && JSON.stringify(JSON.parse(without)) === onlyAna, `${without}`);
check("and one only a person can redivide is answered with none",
      splitz.split_without(JSON.stringify({ type: "exact", amounts: { [ana.me]: 1, [ben.me]: 1 } }),
        ben.me) === undefined,
      "none");
let notSplit;
try {
  splitz.split_without("{", ben.me);
} catch (e) {
  notSplit = e;
}
check("text that is not a split is refused", notSplit instanceof splitz.SplitzErrorHost,
      `${notSplit?.detail}`);

console.log("ben owes half of it");
ben.take(ana);
const owed = splitz.obligation_of(ben.facts(), billId, ben.entries);
check("ben has an obligation", owed !== undefined, owed?.request?.uri ?? "none");
check("it is four and a half thousand to ana",
      owed.settlements[0].to === ana.me && Number(owed.settlements[0].amount) === 4500,
      `${owed.settlements[0].to} ${owed.settlements[0].amount}`);
check("the request is a ZIP 321 URI naming ana's address",
      owed.request.uri.startsWith("zcash:u1ana"), owed.request.uri);
check("nothing was withheld from the request",
      Number(owed.request.withheld_minor_units) === 0,
      `${owed.request.withheld_minor_units}`);

console.log("a send the wallet wrote down is not cleared on a person's word (§14.3)");
const txid = "ab".repeat(32);
const note = splitz.pending_send_note(billId, owed, ben.now());
check("nobody may say it never left while the wallet is still sending",
      splitz.pending_send_unsent_refusal(billId, note, true, [])?.tag === "StillSending",
      JSON.stringify(splitz.pending_send_unsent_refusal(billId, note, true, [])));
const builtSince = splitz.pending_send_unsent_refusal(billId, note, false,
    [{ txid, created: ben.now() }]);
check("nor once the wallet built a transaction after the note was written",
      builtSince?.tag === "BuiltSince" && builtSince.txid === txid, JSON.stringify(builtSince));
check("one built before it does not hold the note",
      splitz.pending_send_unsent_refusal(billId, note, false,
        [{ txid: "cd".repeat(32), created: "2026-10-28T19:30:00.000Z" }]) === undefined,
      "none");

console.log("the wallet sends, then records what §14.3 allows");
const records = splitz.payment_entries_for_send(ben.facts(), billId, owed, "tx-ben-1", ben.seed);
check("one record, for what the request carried", records.length === 1,
      `${records.length} record(s)`);
for (const record of records) ben.add(record);

ana.take(ben);
const afterPayment = splitz.fold_entries(ana.facts(), billId, ana.entries);
check("ana sees the payment", afterPayment.bill.payments.length === 1,
      JSON.stringify(afterPayment.bill.payments.map((p) => p.id)));
check("and it is not confirmed", afterPayment.bill.confirmed_payments.length === 0,
      JSON.stringify(afterPayment.bill.confirmed_payments));
const paid = afterPayment.bill.payments[0];
const withdrawal = (me, state) =>
  splitz.own_payment_withdrawal_refusal(paid.from, paid.method, paid.reference, me, state);
check("ben may not withdraw his record while its transaction is mined",
      withdrawal(ben.me, splitz.TransactionState.Mined) === splitz.OwnPaymentWithdrawal.Mined,
      `${withdrawal(ben.me, splitz.TransactionState.Mined)}`);
check("and may once it expired unmined",
      withdrawal(ben.me, splitz.TransactionState.Expired) === undefined, "none");
check("ana's word on ben's record is not this rule's",
      withdrawal(ana.me, splitz.TransactionState.Mined) === undefined, "none");
const paidPlan = splitz.plan_removal(ana.facts(), billId, ana.entries, ben.me, ana.me);
check("once he has paid, taking ben off is blocked by the payment",
      paidPlan.blockers.length === 1 &&
        paidPlan.blockers[0].block === splitz.RemovalBlock.Payment && paidPlan.blockers[0].from_them,
      JSON.stringify(paidPlan.blockers.map((b) => b.block)));
check("so the plan ana saw before no longer stands",
      splitz.same_removal_plan(unpaid, paidPlan) === splitz.RemovalPlanStanding.Changed, "changed");check("so ben is asked for nothing twice",
      splitz.obligation_of(ben.facts(), billId, ben.entries).settlements.length === 0,
      JSON.stringify(splitz.obligation_of(ben.facts(), billId, ben.entries).settlements));

// A payee confirms a payment they can see, by the id the bill carries. One
// transaction paying several people writes one record each, so the id is not
// the transaction's — the transaction is in `reference`.
const toConfirm = afterPayment.bill.payments[0].id;
ana.add(splitz.confirm_payment_entry(ana.facts(), billId, toConfirm, "recipientConfirmed",
    undefined, afterPayment.payment_digests.get(toConfirm), ana.seed));
ben.take(ana);
const settled = splitz.obligation_of(ben.facts(), billId, ben.entries);
check("once confirmed, the debt is gone",
      settled.settlements.length === 0 && settled.awaiting.length === 0,
      `settlements=${settled.settlements.length} awaiting=${settled.awaiting.length}`);

console.log("a refusal still crosses as a §12 code");
check("a scan that is nothing is refused by its code",
      typeof splitz.read_scanned("not a bill").refused_code === "string",
      `${splitz.read_scanned("not a bill").refused_code}`);

console.log("two price sources agree, or give no price");
check("two sources within the tolerance agree on the higher",
      Number(splitz.agreed_price(138819, 138905, 200)) === 138905,
      `${splitz.agreed_price(138819, 138905, 200)}`);
check("and two that are not give no price",
      splitz.agreed_price(138819, 152701, 200) === undefined,
      `${splitz.agreed_price(138819, 152701, 200)}`);

console.log("a payout a person declares goes first, and replaces its own kind (§9.1)");
const listed = (payouts) =>
  payouts.map((p) => [p.kind, p.address, p.asset, p.chain].map((f) => f ?? "-").join(":")).join(" ");
const ranked = splitz.ranked_payouts(
    { id: "p", name: "P", pay_to: "zOld", identity_key: undefined, payouts: [] },
    { kind: "swap", address: "0xa", asset: "USDC", chain: undefined });
check("a pay-to-only record keeps its ZEC behind the new swap",
      listed(ranked) === "swap:0xa:USDC:- zec:zOld:-:-", listed(ranked));
const replaced = splitz.ranked_payouts(
    { id: "p", name: "P", pay_to: undefined, identity_key: undefined, payouts: [
      { kind: "cash", address: undefined, asset: undefined, chain: undefined },
      { kind: "zec", address: "zA", asset: undefined, chain: undefined },
      { kind: "swap", address: "0xb", asset: "USDT", chain: undefined },
    ] },
    { kind: "zec", address: "zB", asset: undefined, chain: undefined });
check("a new ZEC payout replaces the old one, the rest keep their order",
      listed(replaced) === "zec:zB:-:- cash:-:-:- swap:0xb:USDT:-", listed(replaced));

console.log("a swap deposit is checked against the bill before it is sent (§15.7)");
const onBase = { kind: "swap", address: "0xbenbase", asset: "USDC", chain: "base" };
const onArb = { kind: "swap", address: "0xbenarb", asset: "USDC", chain: "arb" };
const taxiCreate = splitz.create_bill_entry(ana.facts(), "Taxi", "EUR", "equal", anaKey, undefined, ana.seed);
const taxiId = JSON.parse(taxiCreate).id;
const taxi = [
  taxiCreate,
  splitz.join_bill_entry(ana.facts(), taxiId, "Ana", "u1ana", anaKey, [], ana.seed),
  splitz.join_bill_entry(ben.facts(), taxiId, "Ben", undefined, benKey, [onBase, onArb], ben.seed),
  splitz.add_expense_entry(ben.facts(), taxiId, "t1", ben.me, 8000,
    JSON.stringify({ type: "equal", among: [ana.me, ben.me] }), undefined, ben.seed),
  splitz.set_rate_entry(ana.facts(), taxiId, "EUR", 51234, undefined, ana.seed),
];
const quote = (recipient, chain) => ({
  deposit_address: "t1deposit", recipient, deposit_memo: undefined,
  amount_in_zatoshi: 7807316, amount_out: "39990000", min_amount_out: undefined,
  asset: { asset_id: `nep141:${chain}-usdc`, symbol: "USDC", chain, decimals: 6 },
  deadline: "2026-10-29T23:00:00.000Z", reference: "intent-1",
});
const swapRefusal = (q, chosen) =>
  splitz.swap_send_refusal(ana.facts(), taxiId, taxi, q, ben.me, 4000, chosen);
check("a deposit to ben's first payout, for what ana owes, may go",
      swapRefusal(quote("0xbenbase", "base"), undefined) === undefined, "none");
check("one asked for his second payout may go too",
      swapRefusal(quote("0xbenarb", "arb"), onArb) === undefined, "none");
const wrongRecipient = swapRefusal(quote("0xbenbase", "base"), onArb);
check("one whose recipient is not the payout chosen is refused",
      wrongRecipient?.tag === "RecipientChanged", JSON.stringify(wrongRecipient));
check("the payout chosen is found by type, address, asset and chain",
      splitz.declared_payout_index([onBase, onArb], onArb) === 1 &&
        splitz.declared_payout_index([onBase, onArb], { ...onArb, chain: undefined }) === undefined,
      `${splitz.declared_payout_index([onBase, onArb], onArb)}`);
const swapRecord = splitz.record_payment_entry(ana.facts(), taxiId, {
  payment_id: "intent-1", to: ben.me, amount: 4000, method: "swap", reference: "intent-1",
  zatoshi: 7807316, paid_at_rate: undefined, note: undefined,
}, ana.seed);
taxi.push(swapRecord);
const held = swapRefusal(quote("0xbenbase", "base"), undefined);
check("once a payment covers the debt, a second deposit is held for ben to confirm",
      held?.tag === "Held" && JSON.stringify(held.paid_to) === JSON.stringify([ben.me]),
      JSON.stringify(held));
const failed = splitz.failed_swap_withdrawals(ana.facts(), taxiId, taxi, "intent-1");
check("a swap that failed names ana's record of it to withdraw",
      failed.length === 1 && failed[0] === JSON.parse(swapRecord).id, JSON.stringify(failed));
check("and nothing to ben, who did not write it",
      splitz.failed_swap_withdrawals(ben.facts(), taxiId, taxi, "intent-1").length === 0, "none");

console.log(failures === 0
  ? `PACKAGE CONSUMER RESULT: an installed npm package drives a whole bill, ${failures} failures`
  : `PACKAGE CONSUMER RESULT: ${failures} check(s) failed`);
process.exit(failures === 0 ? 0 : 1);
