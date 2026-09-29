//! Every answer this layer gives, as data in and data out.
//!
//! A wallet reads its own store, keeps its own secrets, talks to its own
//! relay and signs its own transactions. It hands the entries it holds to one
//! of these and gets the next thing to do. Nothing here opens a socket, reads
//! a clock or keeps state between calls.

use serde_json::Value;
use splitz_core::host::{
    add_expense, amend_entry, confirm_payment, create_bill, invite_for, join_bill, read_scan,
    record_payment, set_rate, shareable_bill, sign_entry, void_entry, BillLog, Scanned,
};
use splitz_core::money::checked_add;
use splitz_core::{code, decode_rate, merge_logs, order_entries};
use splitz_host::{activity_of, PendingSend, Sealing, SendEnded, Signer, Unrecordable};
use std::collections::{BTreeMap, HashMap};

use crate::convert;
use crate::error::{Result, SplitzError};
use crate::host_facts::{FactHost, HostFacts};
use crate::records as ffi;

/// Reads a log a wallet handed over, in the order §10.2 puts it.
fn parse_entries(entries_json: &[String]) -> Result<Vec<Value>> {
    let mut entries = Vec::with_capacity(entries_json.len());
    for text in entries_json {
        entries.push(parse(text, "an entry")?);
    }
    order_entries(&mut entries);
    Ok(entries)
}

fn parse(text: &str, what: &str) -> Result<Value> {
    serde_json::from_str(text).map_err(|e| SplitzError::Host {
        detail: format!("{what} is not JSON: {e}"),
        transient: false,
    })
}

/// The signing seed a wallet keeps, as §9.4 writes a key: unpadded base64url.
///
/// Text rather than bytes, because that is what a keychain holds and what
/// every other key in this API already is.
///
/// Checked for length here, where it enters: every signing closure below
/// relies on it, and a seed of the wrong length reaching one would otherwise
/// fail inside a callback that has no way to return an error.
fn seed_bytes(seed: &str) -> Result<Vec<u8>> {
    let bytes = splitz_host::base64url_decode(seed).ok_or_else(|| SplitzError::Host {
        detail: "an identity seed is base64url".to_owned(),
        transient: false,
    })?;
    if bytes.len() != splitz_host::SEED_BYTES {
        return Err(SplitzError::Host {
            detail: format!(
                "an identity seed is {} bytes, not {}",
                splitz_host::SEED_BYTES,
                bytes.len()
            ),
            transient: false,
        });
    }
    Ok(bytes)
}

/// Signs `entry` with `seed` on the bill `bill_id` (§10.6) and returns it as
/// the JSON §9.3 canonicalises.
fn signed(facts: &HostFacts, seed: &[u8], entry: Value, bill_id: &str) -> Result<String> {
    let sign = |message: &[u8]| {
        Signer
            .sign(seed, message)
            .expect("seed_bytes checked the length")
    };
    let host = FactHost {
        facts,
        sign: Some(&sign),
        verify: None,
    };
    Ok(sign_entry(&host, &entry, bill_id)?.to_string())
}

/// Builds an entry through a host that knows only the facts it was given, and
/// signs it on `bill_id` — or, for the entry that opens a bill, on the id it
/// derives.
fn build(
    facts: &HostFacts,
    seed: &str,
    bill_id: Option<&str>,
    make: impl Fn(&FactHost<'_>) -> splitz_core::Result<Value>,
) -> Result<String> {
    let seed = seed_bytes(seed)?;
    let sign = |message: &[u8]| {
        Signer
            .sign(&seed, message)
            .expect("seed_bytes checked the length")
    };
    let host = FactHost {
        facts,
        sign: Some(&sign),
        verify: None,
    };
    let entry = make(&host)?;
    let bill = match bill_id {
        Some(id) => id.to_owned(),
        None => entry
            .get("id")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_owned(),
    };
    signed(facts, &seed, entry, &bill)
}

// --- entries a device writes -----------------------------------------------

/// Opens a bill. Its §9.4 id is the digest of this entry, so read it back
/// from the entry rather than deriving it a second way.
#[uniffi::export]
pub fn create_bill_entry(
    facts: HostFacts,
    name: String,
    currency: String,
    split_mode: String,
    creator_key: String,
    seed: String,
) -> Result<String> {
    // §9.4's nonce is 16 bytes nobody can predict. Fewer is refused rather
    // than padded: two bills opened with one short nonce would be one bill.
    if facts.nonce.len() < 16 {
        return Err(SplitzError::Host {
            detail: format!(
                "a bill's nonce is 16 random bytes, got {}",
                facts.nonce.len()
            ),
            transient: false,
        });
    }
    build(&facts, &seed, None, |host| {
        create_bill(host, &name, &currency, &split_mode, &creator_key)
    })
}

#[uniffi::export]
pub fn join_bill_entry(
    facts: HostFacts,
    bill_id: String,
    name: Option<String>,
    pay_to: Option<String>,
    identity_key: Option<String>,
    payouts: Vec<ffi::Payout>,
    seed: String,
) -> Result<String> {
    // §9.1: how this participant is paid, most preferred first. None declared
    // leaves `payTo` to speak for them.
    let payouts = (!payouts.is_empty()).then(|| payouts.iter().map(payout_json).collect());
    build(&facts, &seed, Some(&bill_id), |host| {
        join_bill(
            host,
            name.as_deref(),
            pay_to.as_deref(),
            identity_key.as_deref(),
            payouts.clone(),
        )
    })
}

/// One §9.1 payout as the join carries it.
fn payout_json(p: &ffi::Payout) -> Value {
    let mut out = serde_json::Map::new();
    out.insert("type".to_owned(), Value::from(p.kind.clone()));
    for (member, value) in [
        ("address", &p.address),
        ("asset", &p.asset),
        ("chain", &p.chain),
    ] {
        if let Some(value) = value {
            out.insert(member.to_owned(), Value::from(value.clone()));
        }
    }
    Value::Object(out)
}

/// A §7 rate as a record carries it in `paidAtRate`.
fn rate_json(r: &ffi::ExchangeRate) -> Value {
    let mut out = serde_json::Map::new();
    out.insert("currency".to_owned(), Value::from(r.currency.clone()));
    out.insert(
        "minorUnitsPerZec".to_owned(),
        Value::from(r.minor_units_per_zec),
    );
    out.insert("at".to_owned(), Value::from(r.at.clone()));
    if let Some(source) = &r.source {
        out.insert("source".to_owned(), Value::from(source.clone()));
    }
    Value::Object(out)
}

// A binding exports flat parameters, and each one is a member the entry needs.
#[allow(clippy::too_many_arguments)]
#[uniffi::export]
pub fn add_expense_entry(
    facts: HostFacts,
    bill_id: String,
    expense_id: String,
    paid_by: String,
    amount: i64,
    split_json: String,
    description: Option<String>,
    seed: String,
) -> Result<String> {
    let split = parse(&split_json, "a split")?;
    build(&facts, &seed, Some(&bill_id), |host| {
        add_expense(
            host,
            &expense_id,
            &paid_by,
            amount,
            split.clone(),
            description.as_deref(),
        )
    })
}

/// A payment somebody says they made.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct PaymentDraft {
    /// The transaction id for a `shieldedZec` payment; the caller's own id for
    /// cash, which MUST be unique on the bill — two cash payments sharing an
    /// id are one payment to every reader that folds the log.
    pub payment_id: String,
    pub to: String,
    /// Minor units of the bill's currency (§2.1).
    pub amount: i64,
    /// `shieldedZec`, `swap` or `cash` (§9.2).
    pub method: String,
    /// A swap's own identifier. **Not a Zcash txid.**
    pub reference: Option<String>,
    /// What left the payer's wallet. Advisory: the fiat `amount` settles the
    /// debt and this takes no part in §5 or §6.
    pub zatoshi: Option<i64>,
    /// The rate the payment was priced at (§9.2), so the payee confirms
    /// against a figure they can compare with what arrived.
    pub paid_at_rate: Option<ffi::ExchangeRate>,
    pub note: Option<String>,
}

/// Records a claim that a debt was discharged. **A record is a claim**: §10.5
/// moves the balance only when the payee confirms.
#[uniffi::export]
pub fn record_payment_entry(
    facts: HostFacts,
    bill_id: String,
    payment: PaymentDraft,
    seed: String,
) -> Result<String> {
    build(&facts, &seed, Some(&bill_id), |host| {
        record_payment(
            host,
            &payment.payment_id,
            &payment.to,
            payment.amount,
            &payment.method,
            payment.reference.as_deref(),
            payment.zatoshi,
            payment.paid_at_rate.as_ref().map(rate_json),
            payment.note.as_deref(),
        )
    })
}

/// Confirms a payment to this device. **Only the payee confirms** — a payer
/// who could confirm their own would settle a debt by asserting twice that
/// they paid it.
#[uniffi::export]
///
/// `record` is the digest of the record being confirmed, from the folded
/// bill's `payment_digests` (§10.5): the confirmation stands only while the
/// record under `payment_id` still says what it said then.
pub fn confirm_payment_entry(
    facts: HostFacts,
    bill_id: String,
    payment_id: String,
    method: String,
    reference: Option<String>,
    record: String,
    seed: String,
) -> Result<String> {
    build(&facts, &seed, Some(&bill_id), |host| {
        confirm_payment(host, &payment_id, &method, reference.as_deref(), &record)
    })
}

/// Snapshots a rate onto the bill (§7), so every device prices from one figure
/// rather than from whatever its own feed said.
#[uniffi::export]
pub fn set_rate_entry(
    facts: HostFacts,
    bill_id: String,
    currency: String,
    minor_units_per_zec: i64,
    source: Option<String>,
    seed: String,
) -> Result<String> {
    build(&facts, &seed, Some(&bill_id), |host| {
        set_rate(host, &currency, minor_units_per_zec, source.as_deref())
    })
}

#[uniffi::export]
pub fn void_entry_for(
    facts: HostFacts,
    bill_id: String,
    target_id: String,
    seed: String,
) -> Result<String> {
    build(&facts, &seed, Some(&bill_id), |host| {
        void_entry(host, &target_id)
    })
}

/// Replaces an entry this device wrote, wholesale (§10.3).
///
/// `member` names the payload's own member — `expense`, `participant`,
/// `payment` — and must be the target's own kind: an amendment carrying
/// another kind's payload deletes what it claims to correct.
#[uniffi::export]
pub fn amend_entry_for(
    facts: HostFacts,
    bill_id: String,
    target_id: String,
    member: String,
    payload_json: String,
    seed: String,
) -> Result<String> {
    let payload = parse(&payload_json, "an amendment payload")?;
    build(&facts, &seed, Some(&bill_id), |host| {
        amend_entry(host, &target_id, &member, payload.clone())
    })
}

// --- what a log says --------------------------------------------------------

/// A merged log, and what §10.1 refused at ingress.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct MergeOutcome {
    pub entries: Vec<String>,
    /// A caller that ignores this has dropped somebody's entry without
    /// telling them.
    pub refused: Vec<ffi::SetAside>,
}

/// Merges two logs by the set union §10.2 fixes: idempotent, commutative, and
/// deciding a collision by content rather than by which copy arrived first.
#[uniffi::export]
pub fn merge_entries(held: Vec<String>, incoming: Vec<String>) -> Result<MergeOutcome> {
    let held = parse_entries(&held)?;
    let incoming = parse_entries(&incoming)?;
    let merged = merge_logs(&[held, incoming])?;
    Ok(MergeOutcome {
        entries: merged.merged.iter().map(Value::to_string).collect(),
        refused: merged.refused.iter().map(convert::set_aside).collect(),
    })
}

/// The bill as `entries` stands, with what the fold refused and which keys
/// §10.7 binds.
///
/// `bill_id` names the bill the entries belong to, so a create for another
/// bill pushed into its channel cannot make it unopenable (§10.3).
#[uniffi::export]
pub fn fold_entries(
    facts: HostFacts,
    bill_id: String,
    entries: Vec<String>,
) -> Result<ffi::FoldedBill> {
    let entries = parse_entries(&entries)?;
    let verified = Signer.prepare(entries.iter(), &bill_id);
    let verify = |entry: &Value, key: &str| verified.verify(entry, key);
    let host = FactHost {
        facts: &facts,
        sign: None,
        verify: Some(&verify),
    };
    let folded = BillLog::with_entries(&host, entries)
        .for_bill(bill_id)
        .fold()?;
    let unanswered = verified.unanswered();
    if !unanswered.is_empty() {
        // A pair nobody answered reads as an invalid signature, which is a
        // much quieter claim than "nobody asked".
        return Err(SplitzError::Host {
            detail: format!(
                "the fold asked about {} (entry, key) pair(s) that were never verified",
                unanswered.len()
            ),
            transient: false,
        });
    }
    Ok(convert::folded(&folded))
}

/// The log read as a history, newest first.
///
/// `bill_id` names the bill the entries belong to, so a create for another
/// bill pushed into its channel cannot make it unopenable (§10.3).
#[uniffi::export]
pub fn history_of(
    facts: HostFacts,
    bill_id: String,
    entries: Vec<String>,
) -> Result<Vec<ffi::BillEvent>> {
    let parsed = parse_entries(&entries)?;
    let verified = Signer.prepare(parsed.iter(), &bill_id);
    let verify = |entry: &Value, key: &str| verified.verify(entry, key);
    let host = FactHost {
        facts: &facts,
        sign: None,
        verify: Some(&verify),
    };
    let folded = BillLog::with_entries(&host, parsed.clone())
        .for_bill(bill_id)
        .fold()?;
    Ok(
        activity_of(&parsed, &folded.bill, &folded.set_aside, &folded.withdrawn)
            .iter()
            .map(convert::event)
            .collect(),
    )
}

/// What this device owes, and the §8 request that carries it.
///
/// `None` when the bill carries no rate: an unpriced bill is an ordinary bill
/// and nothing invents a price to avoid showing that.
///
/// `bill_id` names the bill the entries belong to, so a create for another
/// bill pushed into its channel cannot make it unopenable (§10.3).
#[uniffi::export]
pub fn obligation_of(
    facts: HostFacts,
    bill_id: String,
    entries: Vec<String>,
) -> Result<Option<ffi::PayerObligation>> {
    obligation_via(facts, bill_id, entries, HashMap::new())
}

/// `obligation_of`, with the payer's choice of payout for the recipients `via`
/// names (§14.8).
///
/// `via` maps a participant id to the index of one of their declared payouts;
/// that payout is the one the request is rendered from, for this payment
/// alone, and who owes what does not move. Refused with `unknown_participant`
/// for an id not on the bill and `payout_not_declared` for an index that is
/// not one of that participant's payouts.
#[uniffi::export]
pub fn obligation_via(
    facts: HostFacts,
    bill_id: String,
    entries: Vec<String>,
    via: HashMap<String, i64>,
) -> Result<Option<ffi::PayerObligation>> {
    let via: BTreeMap<String, i64> = via.into_iter().collect();
    let parsed = parse_entries(&entries)?;
    let verified = Signer.prepare(parsed.iter(), &bill_id);
    let verify = |entry: &Value, key: &str| verified.verify(entry, key);
    let host = FactHost {
        facts: &facts,
        sign: None,
        verify: Some(&verify),
    };
    let folded = BillLog::with_entries(&host, parsed)
        .for_bill(bill_id)
        .fold()?;
    Ok(splitz_core::host::obligation_via(&host, &folded, &via)?.map(|o| convert::obligation(&o)))
}

/// Why the payments a wallet read from `uri` are not the ones it asks for, in
/// words for the payer, or `None` when they are (§14.6): [`check_proposal`]'s
/// answer as the sentence to show. A wallet builds nothing unless this is
/// `None`, and a refusal here is a failed send — nothing was built, so nothing
/// can land (§14.3).
#[uniffi::export]
pub fn proposal_problem(uri: String, outputs: Vec<ffi::ProposedOutput>) -> Option<String> {
    let outputs: Vec<splitz_core::host::ProposedOutput> = outputs
        .into_iter()
        .map(|o| splitz_core::host::ProposedOutput {
            address: o.address,
            zatoshi: o.zatoshi,
        })
        .collect();
    splitz_host::proposal_problem(&uri, &outputs)
}

/// The body of a swap provider's answer with HTTP `status`, refused when the
/// status says no: a 4xx or 5xx body is never read as a quote. The refusal
/// carries the provider's own `message`; a 5xx is transient.
#[uniffi::export]
pub fn swap_answer(status: u16, body: String) -> Result<String> {
    Ok(splitz_host::swap_answer(status, body.as_bytes())?)
}

/// Compares the payments a wallet is about to sign with the request `uri` it
/// was handed (§14.6).
///
/// `outputs` are what the wallet's own ZIP 321 reader made of `uri`, change
/// left out. Each requested payment is matched to one output with the same
/// address and zatoshi, in any order. Sign only when the answer's two lists
/// are both empty. `uri` must be a request this protocol wrote; anything else
/// is refused with `zip321_not_canonical`.
#[uniffi::export]
pub fn check_proposal(
    uri: String,
    outputs: Vec<ffi::ProposedOutput>,
) -> Result<ffi::ProposalCheck> {
    let outputs: Vec<splitz_core::host::ProposedOutput> = outputs
        .into_iter()
        .map(|o| splitz_core::host::ProposedOutput {
            address: o.address,
            zatoshi: o.zatoshi,
        })
        .collect();
    let check = splitz_core::host::check_proposal(&uri, &outputs)?;
    Ok(ffi::ProposalCheck {
        missing: check
            .missing
            .into_iter()
            .map(|p| ffi::ProposedOutput {
                address: p.address,
                zatoshi: p.zatoshi,
            })
            .collect(),
        unexpected: check
            .unexpected
            .into_iter()
            .map(|o| ffi::ProposedOutput {
                address: o.address,
                zatoshi: o.zatoshi,
            })
            .collect(),
    })
}

/// What `address` is — its network, kind, receivers and whether a memo
/// reaches it — or `address_invalid` (§8.6). A wallet checks the network
/// against its own before it pays or publishes an address.
#[uniffi::export]
pub fn parse_address(address: String) -> Result<ffi::ParsedAddress> {
    let parsed = splitz_core::parse_address(&address)?;
    Ok(ffi::ParsedAddress {
        network: parsed.network.as_str().to_owned(),
        kind: parsed.kind.as_str().to_owned(),
        receivers: parsed.receivers,
        can_receive_memo: parsed.can_receive_memo,
    })
}

/// A plain-language sentence for a §12 code, to show a person in place of the
/// code; `None` for a code this library does not define.
#[uniffi::export]
pub fn describe_code(code: String) -> Option<String> {
    splitz_core::describe_code(&code).map(str::to_owned)
}

/// Where this device stands with each person, per currency, summed over
/// every bill it holds. Each bill is folded as [`obligation_of`] folds one.
#[uniffi::export]
pub fn totals_of(facts: HostFacts, bills: Vec<ffi::HeldBill>) -> Result<ffi::Totals> {
    let folded = fold_held(&facts, bills)?;
    let totals = splitz_core::host::totals_across(&folded, &facts.me);
    Ok(ffi::Totals {
        standings: totals
            .standings
            .into_iter()
            .map(|s| ffi::Standing {
                with_id: s.with_id,
                currency: s.currency,
                owed_to_me: s.owed_to_me,
                owed_by_me: s.owed_by_me,
                sent_awaiting: s.sent_awaiting,
                received_awaiting: s.received_awaiting,
                bill_ids: s.bill_ids,
            })
            .collect(),
        uncounted: totals.uncounted.into_iter().collect(),
    })
}

/// Folds each held bill, verified and held to the id it is named by.
fn fold_held(
    facts: &HostFacts,
    bills: Vec<ffi::HeldBill>,
) -> Result<Vec<splitz_core::host::FoldedBill>> {
    let mut folded = Vec::with_capacity(bills.len());
    for held in bills {
        let parsed = parse_entries(&held.entries)?;
        let verified = Signer.prepare(parsed.iter(), &held.bill_id);
        let verify = |entry: &Value, key: &str| verified.verify(entry, key);
        let host = FactHost {
            facts,
            sign: None,
            verify: Some(&verify),
        };
        folded.push(
            BillLog::with_entries(&host, parsed)
                .for_bill(held.bill_id)
                .fold()?,
        );
    }
    Ok(folded)
}

/// Payments to this device whose transaction its wallet received (§14.7),
/// across every bill it holds at once, so one transaction is evidence once.
///
/// Each bill is folded as [`obligation_of`] folds one: verified, and held to
/// the id it is named by. `received` is each transaction this account was
/// paid in, with the zatoshi it brought.
#[uniffi::export]
pub fn arrivals_of(
    facts: HostFacts,
    bills: Vec<ffi::HeldBill>,
    received: Vec<ffi::IncomingTransaction>,
) -> Result<ffi::Arrivals> {
    let folded = fold_held(&facts, bills)?;
    let received: Vec<splitz_core::host::IncomingTransaction> = received
        .into_iter()
        .map(|t| splitz_core::host::IncomingTransaction {
            txid: t.txid,
            zatoshi: t.zatoshi,
        })
        .collect();
    let found = splitz_core::host::arrivals_for(&folded, &facts.me, &received);
    let each = |list: Vec<splitz_core::host::Arrival>| -> Vec<ffi::Arrival> {
        list.into_iter()
            .map(|a| ffi::Arrival {
                bill_id: a.bill_id,
                payment: convert::payment(&a.payment),
                record: a.record,
                txid: a.txid,
            })
            .collect()
    };
    Ok(ffi::Arrivals {
        arrived: each(found.arrived),
        short: each(found.short),
        unstated: each(found.unstated),
    })
}

/// The CoinGecko `/simple/price` request for one ZEC in `currency`, under the
/// API root `origin` a wallet chose (such as
/// `https://api.coingecko.com/api/v3`). `None` when `currency` is not one the
/// ISO 4217 register gives an exponent (§2.1): nothing is asked, and the bill
/// stays unpriced.
#[uniffi::export]
pub fn zec_price_request(origin: String, currency: String) -> Option<String> {
    if !splitz_core::is_currency(&currency) || splitz_host::currency_exponent(&currency).is_none() {
        return None;
    }
    Some(splitz_host::coingecko_price_url(&origin, &currency))
}

/// Minor units of `currency` one ZEC costs, read from the answer to
/// [`zec_price_request`] — exactly, rounding halves up. `None` when the answer
/// does not price it; refused when the answer is not a price answer at all.
#[uniffi::export]
pub fn zec_price_from_response(body: String, currency: String) -> Result<Option<i64>> {
    Ok(splitz_host::price_from_coingecko(&body, &currency)?)
}

/// The Binance request for ZEC's USD price, the ZECUSDC ticker, under the
/// host `origin` a wallet chose (such as `https://data-api.binance.vision`).
///
/// Binance prices USD alone, reading USDC as USD. A wallet asks it first for
/// USD and [`coinbase_price_request`] for every other currency, or for USD
/// when Binance cannot answer.
#[uniffi::export]
pub fn binance_price_request(origin: String) -> String {
    splitz_host::binance_price_url(&origin)
}

/// Minor units of `currency` one ZEC costs, read from the answer to
/// [`binance_price_request`]: `None` for any currency but USD; refused when
/// the answer is not the ZECUSDC ticker or its price is not a decimal string.
#[uniffi::export]
pub fn zec_price_from_binance(body: String, currency: String) -> Result<Option<i64>> {
    Ok(splitz_host::price_from_binance(&body, &currency)?)
}

/// The Coinbase request for ZEC's rates against every currency, under the
/// host `origin` a wallet chose (such as `https://api.coinbase.com`). One
/// answer prices every currency it lists.
#[uniffi::export]
pub fn coinbase_price_request(origin: String) -> String {
    splitz_host::coinbase_price_url(&origin)
}

/// Minor units of `currency` one ZEC costs, read from the answer to
/// [`coinbase_price_request`] exactly, rounding halves up. `None` when the
/// answer does not price it; refused when it is not a ZEC rates answer or the
/// rate is not a decimal string.
#[uniffi::export]
pub fn zec_price_from_coinbase(body: String, currency: String) -> Result<Option<i64>> {
    Ok(splitz_host::price_from_coinbase(&body, &currency)?)
}

// --- keys a wallet keeps ----------------------------------------------------

/// The participant id `key` speaks as (§10.7): the id a wallet that
/// publishes this identity key writes every entry under. A key binds only the
/// id it derives, so a wallet speaking under any other id is unbound.
#[uniffi::export]
pub fn participant_id_for_key(key: String) -> Result<String> {
    splitz_core::participant_id(&key).ok_or_else(|| SplitzError::Host {
        detail: "an identity key is 32 bytes, canonical unpadded base64url".to_owned(),
        transient: false,
    })
}

/// The public half other participants pin under §10.7, from the seed a wallet
/// holds.
#[uniffi::export]
pub fn identity_key_from_seed(seed: String) -> Result<String> {
    let bytes = seed_bytes(&seed)?;
    Signer
        .public_key_from_seed(&bytes)
        .ok_or_else(|| SplitzError::Host {
            detail: "an identity seed is 32 bytes".to_owned(),
            transient: false,
        })
}

/// The seed an account signs with, derived from its spending secret.
///
/// `secret` is bytes the wallet derives from what only its owner holds — for
/// a software wallet, the mnemonic and passphrase — so the same mnemonic
/// yields the same identity on a reinstalled device. It MUST NOT be anything
/// the wallet shows or shares, such as a viewing key: whoever holds it holds
/// the identity. A wallet with no such secret mints a random seed instead,
/// which signs correctly and cannot be recovered.
#[uniffi::export]
pub fn identity_seed_from_secret(secret: Vec<u8>) -> Result<String> {
    if secret.is_empty() {
        return Err(SplitzError::Host {
            detail: "an empty secret derives nothing; mint a random seed".to_owned(),
            transient: false,
        });
    }
    Ok(splitz_host::base64url_encode(
        &splitz_host::identity_seed_from(&secret),
    ))
}

/// Why `key` is not one the cipher can use, or `None` when it is.
///
/// §11.1 checks only that an invite's `k` is non-empty base64url — not that it
/// is the right length — so a key that is the right alphabet and the wrong
/// length reaches a wallet untouched, and storing it would move the failure
/// into whatever loop next tries to decrypt.
///
/// A token rather than a sentence: §1 says the wording is the wallet's.
#[uniffi::export]
pub fn bill_key_problem(key: String) -> Option<String> {
    if key.is_empty() {
        return Some("empty".to_owned());
    }
    match splitz_host::base64url_decode(&key) {
        None => Some("not_base64url".to_owned()),
        Some(bytes) if bytes.len() != splitz_host::KEY_LENGTH_BYTES => {
            Some("wrong_length".to_owned())
        }
        Some(_) => None,
    }
}

/// A new bill key from `entropy`, whose bytes MUST be exactly 32 from the
/// platform's cryptographically secure generator: base64url, no padding, the
/// form an invite carries and [`bill_key_problem`] accepts.
///
/// The binding takes the bytes rather than drawing them because it has no
/// source of randomness of its own. Refused for any other length.
#[uniffi::export]
pub fn new_bill_key(entropy: ffi::RandomBytes) -> Result<String> {
    let random_bytes = entropy.bytes;
    if random_bytes.len() != splitz_host::KEY_LENGTH_BYTES {
        return Err(SplitzError::Host {
            detail: format!(
                "a bill key is {} random bytes, got {}",
                splitz_host::KEY_LENGTH_BYTES,
                random_bytes.len()
            ),
            transient: false,
        });
    }
    Ok(splitz_host::base64url_encode(&random_bytes))
}

// --- what crosses a transport ----------------------------------------------

/// The channel a bill syncs under: the bill id's SHA-256, hex.
///
/// A digest rather than the id itself, because the id is live in every invite
/// and every scanned code. Every participant computes the same channel, and an
/// observer of relay traffic alone cannot run it back to the id.
#[uniffi::export]
pub fn channel_for_bill(bill_id: String) -> String {
    splitz_host::channel_for_bill(&bill_id)
}

// --- the relay's wire, for a client written in the wallet's language --------
//
// A wallet makes the two HTTP requests of §15.5 itself, over its own network
// route; these decide what goes on the wire and what an answer means, so every
// language's client refuses and retries alike. Every refusal is
// `SplitzError::Host`, whose `transient` says whether retrying could succeed.

/// The URL both relay routes address for `channel`: `<origin>/c/<channel>`.
/// An origin carrying a query or a fragment is refused, not transient.
#[uniffi::export]
pub fn relay_channel_url(origin: String, channel: String) -> Result<String> {
    Ok(splitz_host::HttpSplitsRelay::channel_url(
        &origin, &channel,
    )?)
}

/// The JSON body of `POST <origin>/c/<channel>`, or `None` when `blobs` is
/// empty and no request is made. A blob over 65536 characters is refused, not
/// transient, before anything is sent.
#[uniffi::export]
pub fn relay_push_body(blobs: Vec<String>) -> Result<Option<String>> {
    Ok(splitz_host::HttpSplitsRelay::push_body(&blobs)?)
}

/// Reads the body the relay answered a push with, whatever the HTTP status.
/// Anything but `{"ok":true}` is refused, transient.
#[uniffi::export]
pub fn relay_push_answer(body: String) -> Result<()> {
    Ok(splitz_host::HttpSplitsRelay::push_answer(&body)?)
}

/// The blobs in the body the relay answered a fetch with, whatever the HTTP
/// status. An answer that is not `{"blobs":[…]}` is refused, transient; a
/// non-string inside the list is dropped.
#[uniffi::export]
pub fn relay_fetch_answer(body: String) -> Result<Vec<String>> {
    Ok(splitz_host::HttpSplitsRelay::fetch_answer(&body)?)
}

/// Every entry this device holds, sealed under the bill key as it is held —
/// the blobs to push.
///
/// **Nothing is signed here.** An entry is signed when it is written, by the
/// builder that writes it. The log also holds what peers pushed, and an
/// unsigned entry a peer wrote in this device's name would otherwise be signed
/// with this device's key on the next push. A blob is keyed by its content,
/// so pushing the whole log every time is safe: a relay stores each entry
/// once however often it is sent.
#[uniffi::export]
pub fn blobs_to_push(entries: Vec<String>, bill_key: String) -> Result<Vec<String>> {
    let entries = parse_entries(&entries)?;
    let mut blobs = Vec::with_capacity(entries.len());
    for entry in &entries {
        blobs.push(Sealing.seal(entry, &bill_key)?);
    }
    Ok(blobs)
}

/// What a channel's blobs held, and how many would not open.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct OpenedBlobs {
    pub entries: Vec<String>,
    /// Counted rather than ignored. A channel where every blob is unopenable
    /// is a key that is wrong, and that looks identical to a quiet relay
    /// unless somebody counts.
    pub unopenable: u32,
}

/// Opens what a relay handed back.
///
/// A foreign or altered blob is skipped rather than failing the pull, so one
/// bad blob cannot strand a bill. Authorship is **not** judged here: an entry
/// admitted by what a device happened to hold when it arrived would make the
/// stored log depend on network order, and §10.7 decides authorship over the
/// whole log at fold time instead.
#[uniffi::export]
pub fn open_blobs(blobs: Vec<String>, bill_key: String) -> OpenedBlobs {
    let mut entries = Vec::new();
    let mut unopenable = 0;
    for blob in &blobs {
        match Sealing.open(blob, &bill_key) {
            Ok(entry) => entries.push(entry.to_string()),
            Err(_) => unopenable += 1,
        }
    }
    OpenedBlobs {
        entries,
        unopenable,
    }
}

// --- sharing a bill without a relay ----------------------------------------

/// The invite URI for a bill (§11.1).
///
/// `bill_id` names the bill the entries belong to, so a create for another
/// bill pushed into its channel cannot make it unopenable (§10.3).
#[uniffi::export]
pub fn invite_for_bill(
    facts: HostFacts,
    bill_id: String,
    entries: Vec<String>,
    bill_key: String,
    name: Option<String>,
    expiry: Option<i64>,
) -> Result<String> {
    let folded = folded_bill(&facts, &bill_id, &entries)?;
    Ok(invite_for(
        &folded.bill,
        &bill_key,
        name.as_deref(),
        expiry,
    )?)
}

/// The whole bill as one scanned payload (§11.2), or `None` when it will not
/// fit in one. A caller shown `None` shares by relay instead.
///
/// `bill_id` names the bill the entries belong to, so a create for another
/// bill pushed into its channel cannot make it unopenable (§10.3).
#[uniffi::export]
pub fn shareable_bill_payload(
    facts: HostFacts,
    bill_id: String,
    entries: Vec<String>,
    bill_key: String,
) -> Result<Option<String>> {
    let parsed = parse_entries(&entries)?;
    let host = FactHost {
        facts: &facts,
        sign: None,
        verify: None,
    };
    let log = BillLog::with_entries(&host, parsed).for_bill(bill_id);
    let folded = log.fold()?;
    Ok(shareable_bill(&log, &bill_key, &folded.bill))
}

/// The entries a peer holding `they_have` lacks, as one scanned payload
/// (§14.5): `missing` 0 when it lacks none, `too_big_code` when they will not
/// fit one square and a relay is needed. A delta carries no key; its reader
/// already holds one.
///
/// `they_have` is the entry ids the peer reports holding.
#[uniffi::export]
pub fn delta_for_peer(
    facts: HostFacts,
    bill_id: String,
    entries: Vec<String>,
    they_have: Vec<String>,
) -> Result<ffi::Delta> {
    let parsed = parse_entries(&entries)?;
    let host = FactHost {
        facts: &facts,
        sign: None,
        verify: None,
    };
    let log = BillLog::with_entries(&host, parsed).for_bill(bill_id);
    let have: std::collections::BTreeSet<String> = they_have.into_iter().collect();
    Ok(match splitz_core::host::delta_for(&log, &have) {
        splitz_core::Delta::NothingMissing => ffi::Delta {
            missing: 0,
            uri: None,
            too_big_code: None,
        },
        splitz_core::Delta::Square { uri, entry_count } => ffi::Delta {
            missing: entry_count as u64,
            uri: Some(uri),
            too_big_code: None,
        },
        splitz_core::Delta::TooBig { entry_count, code } => ffi::Delta {
            missing: entry_count as u64,
            uri: None,
            too_big_code: Some(code.to_owned()),
        },
    })
}

/// `invite` as an https link under `base` (§11.1), such as
/// `https://example.org/join`: the invite whole in the fragment, which a
/// browser never sends to `base`'s host. [`read_scanned`] reads it back.
#[uniffi::export]
pub fn render_invite_link(invite: String, base: String) -> Result<String> {
    let parsed = splitz_core::parse_invite(&invite)?;
    Ok(splitz_core::render_invite_link(&parsed, &base)?)
}

/// `invite`'s expiry, and whether it falls before `now_unix_seconds` (§11.1).
/// An invite with no expiry never expires. Which clock, and whether an
/// expired invite is refused or only shown, are the wallet's.
#[uniffi::export]
pub fn invite_expiry(invite: String, now_unix_seconds: i64) -> Result<ffi::InviteExpiry> {
    let parsed = splitz_core::parse_invite(&invite)?;
    Ok(ffi::InviteExpiry {
        expiry: parsed.expiry,
        expired: splitz_core::is_invite_expired(&parsed, now_unix_seconds),
    })
}

/// What a scanned code turned out to be.
#[derive(uniffi::Record, Debug, Clone, PartialEq, Eq)]
pub struct ScanOutcome {
    /// The bill the code names, when it names one.
    pub bill_id: Option<String>,
    /// The key it carried, when it carried one. A wallet stores this in its
    /// keychain.
    pub bill_key: Option<String>,
    /// Entries it carried, for a payload rather than a bare invite.
    pub entries: Vec<String>,
    /// The invite's expiry, a Unix time in seconds, when it states one
    /// (§11.1). A freshness hint for the wallet to show and honour; this
    /// protocol does not compare it with a clock.
    pub expiry: Option<i64>,
    /// The §12 code, when it is neither. §1 leaves the wording to the wallet.
    pub refused_code: Option<String>,
}

/// Reads a scanned invite or payload (§11.1, §11.2).
#[uniffi::export]
pub fn read_scanned(text: String) -> ScanOutcome {
    match read_scan(&text) {
        Scanned::Refused(code) => ScanOutcome {
            bill_id: None,
            bill_key: None,
            entries: Vec::new(),
            expiry: None,
            refused_code: Some(code.to_owned()),
        },
        Scanned::Invite(invite) => ScanOutcome {
            bill_id: Some(invite.bill_id),
            bill_key: Some(invite.key),
            entries: Vec::new(),
            expiry: invite.expiry,
            refused_code: None,
        },
        Scanned::Bill(scan) => ScanOutcome {
            bill_id: scan.invite.as_ref().map(|i| i.bill_id.clone()),
            bill_key: scan.invite.as_ref().map(|i| i.key.clone()),
            entries: scan.entries.iter().map(Value::to_string).collect(),
            expiry: scan.invite.as_ref().and_then(|i| i.expiry),
            refused_code: None,
        },
    }
}

/// Folds `entries` with every signature checked.
fn folded_bill(
    facts: &HostFacts,
    bill_id: &str,
    entries: &[String],
) -> Result<splitz_core::host::FoldedBill> {
    let parsed = parse_entries(entries)?;
    let verified = Signer.prepare(parsed.iter(), bill_id);
    let verify = |entry: &Value, key: &str| verified.verify(entry, key);
    let host = FactHost {
        facts,
        sign: None,
        verify: Some(&verify),
    };
    Ok(BillLog::with_entries(&host, parsed)
        .for_bill(bill_id)
        .fold()?)
}

// --- what to record once the wallet has sent --------------------------------

/// The payment entries to append after a send **succeeded** (§14.3).
///
/// One record per recipient the request carried, each for what that recipient
/// was owed. The wallet sends; this says what may then be written down.
///
/// **Only for a send that reached the network.** A transaction that was built
/// and not broadcast may still land: recording it settles a debt nothing on
/// chain settled, and retrying it pays the debt twice. For that outcome a
/// wallet records nothing and says so.
///
/// **What the request carried, not what the payer owes.** A recipient §8.5
/// left out is not paid by this transaction, and recording one would claim a
/// debt was settled that the payee must then dispute.
#[uniffi::export]
pub fn payment_entries_for_send(
    facts: HostFacts,
    bill_id: String,
    obligation: ffi::PayerObligation,
    txid: String,
    seed: String,
) -> Result<Vec<String>> {
    if txid.is_empty() {
        return Err(SplitzError::Host {
            detail: "a send that succeeded carries a transaction id".to_owned(),
            transient: false,
        });
    }
    let (owed, sent) = carried(&obligation)?;
    let paid_at_rate = rate_json(&obligation.rate);
    let mut records = Vec::with_capacity(owed.len());
    for (to, amount) in owed {
        let payment_id = splitz_core::host::payment_id_for_send(&txid, &to);
        records.push(build(&facts, &seed, Some(&bill_id), |host| {
            record_payment(
                host,
                &payment_id,
                &to,
                amount,
                "shieldedZec",
                Some(&txid),
                sent.get(&to).copied(),
                Some(paid_at_rate.clone()),
                None,
            )
        })?);
    }
    Ok(records)
}

/// What `obligation`'s request pays each recipient: in the bill's minor units,
/// the debts it carries — a recipient §8.5 left out is not paid by it — and
/// in zatoshi, what its outputs send.
fn carried(
    obligation: &ffi::PayerObligation,
) -> Result<(BTreeMap<String, i64>, BTreeMap<String, i64>)> {
    let unpayable: std::collections::BTreeSet<&str> = obligation
        .request
        .unpayable
        .iter()
        .map(|u| u.id.as_str())
        .collect();
    // Summed with §2.2's checked arithmetic: the obligation is the caller's
    // record, and a sum that wrapped would record a payment nobody made.
    let overflow = |_| SplitzError::Host {
        detail: "the amounts this obligation carries overflow 64 bits".to_owned(),
        transient: false,
    };
    let mut owed: BTreeMap<String, i64> = BTreeMap::new();
    for settlement in &obligation.settlements {
        if unpayable.contains(settlement.to.as_str()) {
            continue;
        }
        let held = owed.entry(settlement.to.clone()).or_insert(0);
        *held = checked_add(*held, settlement.amount, code::AMOUNT_OVERFLOW).map_err(overflow)?;
    }
    // Each record states what it sent in ZEC and the rate it was priced at
    // (§9.2), from the request the send carried.
    let mut sent: BTreeMap<String, i64> = BTreeMap::new();
    for payment in &obligation.request.payments {
        let held = sent.entry(payment.to.clone()).or_insert(0);
        *held = checked_add(*held, payment.zatoshi, code::AMOUNT_OVERFLOW).map_err(overflow)?;
    }
    Ok((owed, sent))
}

// --- the pay-twice guard (§14.3) --------------------------------------------
//
// A wallet on this binding keeps one string per bill in storage that outlives
// the process, and these carry every rule about it: `PendingSends` in the host
// crate, with the storage left to the wallet.
//
// 1. `pending_send_blocks` before offering a send. `Some` means a send from
//    this bill is written down and not resolved: start no other.
// 2. `pending_send_note`, stored **before** the wallet is called, and in the
//    same step that checked (1) — a second send started between the check and
//    the write is the one this exists to stop.
// 3. `pending_send_after` once the wallet answers: store what it returns, or
//    delete the note on `None`.
// 4. `pending_send_records` when a person says a kept send landed; merge the
//    records into the bill, then delete the note.

/// Why a pending send's records cannot be written, as a foreign caller reads
/// it. A refusal the protocol made keeps its §12 code.
fn unrecordable(e: Unrecordable) -> SplitzError {
    let detail = match e {
        Unrecordable::NotATransactionId => "a transaction id is 64 hexadecimal digits",
        Unrecordable::DetailsLost => {
            "the pending send's note would not read or carries nothing to record; \
             record what was paid by hand, then delete the note"
        }
        Unrecordable::IsASwap => {
            "the pending send was a swap's deposit, recorded by the provider's \
             reference and not by a transaction id"
        }
        Unrecordable::Refused(e) => return e.into(),
    };
    SplitzError::Host {
        detail: detail.to_owned(),
        transient: false,
    }
}

/// The send `note` holds for `bill_id`, or `None` when there is no note.
///
/// **A note that does not read still blocks**, answered with `damaged` set:
/// text that is not JSON, is not what `pending_send_note` writes, or names
/// another bill. Taking it for no note would let the send it stands for go
/// out a second time.
#[uniffi::export]
pub fn pending_send_blocks(bill_id: String, note: Option<String>) -> Option<ffi::PendingSendHeld> {
    let held = PendingSend::held(&bill_id, &note?);
    Some(ffi::PendingSendHeld {
        damaged: held.is_damaged(),
        uri: held.uri,
        at: held.at,
        txid: held.txid,
    })
}

/// The note to store for sending `obligation`'s request from `bill_id`,
/// written before the wallet is called. `at` is the wallet's §9.3 instant.
///
/// It carries what a record of the send needs — what the request carries to
/// each recipient in minor units and in zatoshi, and its rate — so the records
/// can be written after a restart, from the note alone. Refused when the
/// obligation has no request or carries nobody.
#[uniffi::export]
pub fn pending_send_note(
    bill_id: String,
    obligation: ffi::PayerObligation,
    at: String,
) -> Result<String> {
    let Some(uri) = obligation.request.uri.clone() else {
        return Err(SplitzError::Host {
            detail: "there is nothing to send".to_owned(),
            transient: false,
        });
    };
    let (carried, sent) = carried(&obligation)?;
    if carried.is_empty() {
        return Err(SplitzError::Host {
            detail: "there is nothing this request can carry".to_owned(),
            transient: false,
        });
    }
    let send = PendingSend {
        bill_id,
        uri,
        carried,
        at,
        sent,
        rate: Some(decode_rate(&rate_json(&obligation.rate))?),
        swap: None,
        zatoshi: None,
        txid: None,
    };
    Ok(send.to_json().to_string())
}

/// What `note` becomes once the wallet has answered: the note to store, or
/// `None` to delete it (§14.3).
///
/// - `ReachedNetwork` and `recorded` — the records are on the bill: `None`.
/// - `ReachedNetwork` and not `recorded`: kept, with `txid`, so the records
///   can be written once the bill is back.
/// - `Refused` — nothing was spent: `None`, and the debt can be sent again.
/// - `Unresolved`: kept, with `txid` when the wallet named the transaction it
///   built. The wallet does not retry it; a person says which way it went.
///
/// A note that does not read is kept exactly as it was.
#[uniffi::export]
pub fn pending_send_after(
    bill_id: String,
    note: String,
    how: ffi::SendEnded,
    txid: Option<String>,
    recorded: bool,
) -> Option<String> {
    let held = PendingSend::held(&bill_id, &note);
    let how = match how {
        ffi::SendEnded::ReachedNetwork => SendEnded::ReachedNetwork,
        ffi::SendEnded::Refused => SendEnded::Refused,
        ffi::SendEnded::Unresolved => SendEnded::Unresolved,
    };
    let kept = held.after(how, txid.as_deref(), recorded)?;
    Some(if kept == held {
        note
    } else {
        kept.to_json().to_string()
    })
}

/// The signed payment records for the send `note` holds having gone out as
/// the transaction `txid`, for a wallet to merge into the bill before it
/// deletes the note.
///
/// One record per recipient the request carried, each under the payment id
/// `payment_entries_for_send` gives that recipient for the transaction written
/// in lower case; a recipient the bill already holds a record for under it is
/// left out, so one payment is never on the bill twice. `txid` is trimmed
/// and lower-cased. Refused when `txid` is not a transaction id, when the
/// note does not read, and when it was a swap's deposit.
#[uniffi::export]
pub fn pending_send_records(
    facts: HostFacts,
    bill_id: String,
    entries: Vec<String>,
    note: String,
    txid: String,
    seed: String,
) -> Result<Vec<String>> {
    let send = PendingSend::held(&bill_id, &note);
    let seed = seed_bytes(&seed)?;
    let parsed = parse_entries(&entries)?;
    let verified = Signer.prepare(parsed.iter(), &bill_id);
    let verify = |entry: &Value, key: &str| verified.verify(entry, key);
    let sign = |message: &[u8]| {
        Signer
            .sign(&seed, message)
            .expect("seed_bytes checked the length")
    };
    let host = FactHost {
        facts: &facts,
        sign: Some(&sign),
        verify: Some(&verify),
    };
    let mut log = BillLog::with_entries(&host, parsed).for_bill(bill_id);
    let records = send.records(&host, &mut log, &txid).map_err(unrecordable)?;
    Ok(records.iter().map(Value::to_string).collect())
}

// --- what the payer is shown before sending (§14.2) -------------------------

/// The reason codes §8.5 gives a recipient a request cannot carry.
const UNPAYABLE_REASONS: [&str; 4] = ["no_address", "bad_address", "payout_not_zec", "unpriceable"];

/// `zatoshi` as §8.1 writes decimal ZEC: no trailing zeros, `.` as the
/// decimal point, no grouping. The text a review screen shows for an output's
/// amount. Refused for nothing or less, and for more than the supply.
#[uniffi::export]
pub fn render_amount(zatoshi: i64) -> Result<String> {
    Ok(splitz_core::render_amount(zatoshi)?)
}

/// The rate figure a review screen shows: `rate`'s minor units per ZEC in the
/// currency's major units, `.` as the decimal point and every fractional
/// digit its ISO 4217 exponent gives (`51234` EUR is `512.34`). A currency the
/// register gives no exponent is shown as its minor units unchanged.
#[uniffi::export]
pub fn rate_figure(rate: ffi::ExchangeRate) -> String {
    splitz_host::rate_figure(&exchange_rate(&rate))
}

/// Every §14.2 fact for sending `obligation` from the bill `entries` hold that
/// `visible_text` — the strings the wallet's review screen shows — does not
/// show. An empty answer is the only passing one.
///
/// `obligation` is the one `obligation_of` gave for this bill; its outputs'
/// addresses are read from its request. The bill is folded as
/// `fold_entries` folds it, and a participant is looked for by the name it
/// gives them. `reason_words` maps each §8.5 reason code to the screen's own
/// words for it; a code with no entry is a finding. `via` is the choice the
/// obligation was made with (`obligation_via`, empty for `obligation_of`),
/// and `lower_words` the screen's words for a recipient paid by a payout
/// other than their first (§14.8); empty words are a finding when one is.
///
/// The strings are joined with a line break and each fact is looked for as a
/// case-sensitive substring: an amount as [`render_amount`] writes it with no
/// digit touching it, the rate as [`rate_figure`] writes it, an address whole
/// or by a prefix of at least 10 characters that stops on a character that is
/// not an ASCII letter or digit. Findings come in the order of §14.2's list.
///
/// Refused when the entries do not read or fold, and when `obligation` is not
/// one this binding wrote: a request that does not read (`zip321_not_canonical`),
/// outputs that are not the ones its request carries, or a reason §8.5 does
/// not give.
#[allow(clippy::too_many_arguments)]
#[uniffi::export]
pub fn check_payer_review(
    facts: HostFacts,
    bill_id: String,
    entries: Vec<String>,
    obligation: ffi::PayerObligation,
    visible_text: Vec<String>,
    reason_words: HashMap<String, String>,
    via: HashMap<String, i64>,
    lower_words: String,
) -> Result<Vec<ffi::ReviewFinding>> {
    let obligation = payer_obligation(&obligation)?;
    let folded = folded_bill(&facts, &bill_id, &entries)?;
    let reason_words: BTreeMap<String, String> = reason_words.into_iter().collect();
    let via: BTreeMap<String, i64> = via.into_iter().collect();
    let found = splitz_host::check_payer_review(
        &obligation,
        &folded,
        &visible_text,
        &reason_words,
        &via,
        &lower_words,
    )?;
    Ok(found.into_iter().map(review_finding).collect())
}

/// Every §14.2 fact for confirming `payment` that `visible_text` — the
/// strings the wallet's confirm screen shows — does not show. An empty answer
/// is the only passing one.
///
/// `payment` is a record from [`fold_entries`]. Each of its ZEC, rate and
/// reference that it carries must be shown, written and matched as
/// [`check_payer_review`] writes and matches them. A `shieldedZec` or `swap`
/// record lacking one must show `absent_words`, the wallet's words for a
/// missing figure; a `cash` record needs nothing shown.
#[uniffi::export]
pub fn check_payee_review(
    payment: ffi::PaymentRecord,
    visible_text: Vec<String>,
    absent_words: String,
) -> Result<Vec<ffi::ReviewFinding>> {
    let record = splitz_core::PaymentRecord {
        id: payment.id,
        from: payment.from,
        to: payment.to,
        amount: payment.amount,
        currency: payment.currency,
        method: payment.method,
        at: payment.at,
        zatoshi: payment.zatoshi,
        paid_at_rate: payment.paid_at_rate.as_ref().map(exchange_rate),
        reference: payment.reference,
        note: payment.note,
    };
    let found = splitz_host::check_payee_review(&record, &visible_text, &absent_words)?;
    Ok(found.into_iter().map(review_finding).collect())
}

/// The payments this device may confirm, newest first (§10.5): those naming
/// `facts.me` as payee and not yet confirmed. Only the payee confirms; each is
/// shown with what [`check_payee_review`] holds a confirm screen to.
///
/// `bill_id` names the bill the entries belong to, as for [`fold_entries`].
#[uniffi::export]
pub fn awaiting_my_confirmation(
    facts: HostFacts,
    bill_id: String,
    entries: Vec<String>,
) -> Result<Vec<ffi::PaymentRecord>> {
    let folded = folded_bill(&facts, &bill_id, &entries)?;
    Ok(
        splitz_host::awaiting_confirmation_by(&folded.bill, &facts.me)
            .iter()
            .map(convert::payment)
            .collect(),
    )
}

fn review_finding(f: splitz_host::ReviewFinding) -> ffi::ReviewFinding {
    ffi::ReviewFinding {
        rule: match f.rule {
            splitz_host::ReviewRule::Unpayable => ffi::ReviewRule::Unpayable,
            splitz_host::ReviewRule::ReplacedAddress => ffi::ReviewRule::ReplacedAddress,
            splitz_host::ReviewRule::Awaiting => ffi::ReviewRule::Awaiting,
            splitz_host::ReviewRule::LowerPreference => ffi::ReviewRule::LowerPreference,
            splitz_host::ReviewRule::Rate => ffi::ReviewRule::Rate,
            splitz_host::ReviewRule::Output => ffi::ReviewRule::Output,
            splitz_host::ReviewRule::PayeeZec => ffi::ReviewRule::PayeeZec,
            splitz_host::ReviewRule::PayeeRate => ffi::ReviewRule::PayeeRate,
            splitz_host::ReviewRule::PayeeReference => ffi::ReviewRule::PayeeReference,
        },
        fact: f.fact,
        expected: f.expected,
    }
}

fn exchange_rate(r: &ffi::ExchangeRate) -> splitz_core::ExchangeRate {
    splitz_core::ExchangeRate {
        currency: r.currency.clone(),
        minor_units_per_zec: r.minor_units_per_zec,
        at: r.at.clone(),
        source: r.source.clone(),
    }
}

/// The library's obligation for one `obligation_of` handed out. Each output's
/// address is read from the request, which must carry exactly the outputs the
/// record lists, in its order and for its zatoshi.
fn payer_obligation(o: &ffi::PayerObligation) -> Result<splitz_core::host::PayerObligation> {
    let not_ours = |detail: &str| SplitzError::Host {
        detail: format!("this obligation is not one the binding wrote: {detail}"),
        transient: false,
    };
    let payments = match &o.request.uri {
        Some(uri) => splitz_core::read_request(uri)?,
        None => Vec::new(),
    };
    if payments.len() != o.request.payments.len()
        || payments
            .iter()
            .zip(&o.request.payments)
            .any(|(read, listed)| read.zatoshi != listed.zatoshi)
    {
        return Err(not_ours("its outputs are not the ones its request carries"));
    }
    let mut unpayable = Vec::with_capacity(o.request.unpayable.len());
    for u in &o.request.unpayable {
        let Some(reason) = UNPAYABLE_REASONS.iter().find(|r| **r == u.reason) else {
            return Err(not_ours(&format!(
                "{:?} is not a reason §8.5 gives",
                u.reason
            )));
        };
        unpayable.push(splitz_core::Unpayable {
            id: u.id.clone(),
            reason,
            minor_units: u.minor_units,
        });
    }
    Ok(splitz_core::host::PayerObligation {
        settlements: o
            .settlements
            .iter()
            .map(|s| splitz_core::Settlement {
                from: s.from.clone(),
                to: s.to.clone(),
                amount: s.amount,
                covers: s
                    .covers
                    .iter()
                    .map(|d| splitz_core::DirectDebt {
                        from: d.from.clone(),
                        to: d.to.clone(),
                        amount: d.amount,
                    })
                    .collect(),
            })
            .collect(),
        awaiting: o
            .awaiting
            .iter()
            .map(|a| splitz_core::Awaiting {
                to: a.to.clone(),
                owed: a.owed,
                paid: a.paid,
                paid_to: a.paid_to.clone(),
            })
            .collect(),
        request: splitz_core::Obligation {
            uri: o.request.uri.clone(),
            payments,
            recipients: o.request.payments.iter().map(|p| p.to.clone()).collect(),
            unpayable,
            carried_minor_units: o.request.carried_minor_units,
            withheld_minor_units: o.request.withheld_minor_units,
        },
        rate: exchange_rate(&o.rate),
    })
}

// --- a debt owed in another asset (§9.2, §15.7) ----------------------------
//
// The provider is reached over HTTP, and HTTP is the wallet's. What the
// request looks like and what the answer means are not.

/// The body a quote request carries, for the wallet to POST to `/v0/quote`.
#[uniffi::export]
pub fn swap_quote_request(
    zec_asset_id: String,
    asset: ffi::TradableAsset,
    amount_in_zatoshi: i64,
    recipient: String,
    refund_to: String,
    deadline: String,
    referral: Option<String>,
) -> Result<String> {
    Ok(splitz_host::quote_request_body(
        &zec_asset_id,
        &convert::asset_back(&asset),
        amount_in_zatoshi,
        &recipient,
        &refund_to,
        &deadline,
        referral.as_deref(),
    )?)
}

/// Every asset a provider's `/v0/tokens` answer says it will deliver.
///
/// Read before quoting, so a payout naming an asset the provider does not
/// carry is refused before a person is asked to send anything.
#[uniffi::export]
pub fn swap_assets_from_tokens(body: String) -> Result<Vec<ffi::TradableAsset>> {
    Ok(splitz_host::assets_from_tokens(&body)?
        .iter()
        .map(convert::asset)
        .collect())
}

/// The quote a provider's `/v0/quote` answer states, once it is shown to
/// answer `request` — the body `swap_quote_request` produced and the wallet
/// posted. A quote for another recipient, asset or amount is refused.
#[uniffi::export]
pub fn swap_quote_from_response(
    body: String,
    request: String,
    asset: ffi::TradableAsset,
    amount_in_zatoshi: i64,
    asked_deadline: String,
) -> Result<ffi::SwapQuote> {
    Ok(convert::quote(&splitz_host::quote_from_response(
        &body,
        &request,
        &convert::asset_back(&asset),
        amount_in_zatoshi,
        &asked_deadline,
    )?))
}

/// What a provider's `/v0/status` answer means.
///
/// **Not a confirmation.** §10.5 says only the recipient settles a debt, and
/// the transaction this reports is on the destination chain. A status word
/// nobody here has defined reads as still processing, never as delivered.
#[uniffi::export]
pub fn swap_status_from_response(body: String) -> Result<ffi::SwapStatus> {
    Ok(convert::status(&splitz_host::status_from_response(&body)?))
}
