//! Every answer this layer gives, as data in and data out.
//!
//! A wallet reads its own store, keeps its own secrets, talks to its own
//! relay and signs its own transactions. It hands the entries it holds to one
//! of these and gets the next thing to do. Nothing here opens a socket, reads
//! a clock or keeps state between calls.

use serde_json::Value;
use splitz_core::host::{
    add_expense, amend_entry, confirm_payment, create_bill, invite_for, join_bill, obligation_for,
    read_scan, record_payment, set_rate, shareable_bill, sign_entry, void_entry, BillLog, Scanned,
};
use splitz_core::{merge_logs, order_entries};
use splitz_host::{activity_of, Sealing, Signer};

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

/// Signs `entry` with `seed` and returns it as the JSON §9.3 canonicalises.
fn signed(facts: &HostFacts, seed: &[u8], entry: Value) -> Result<String> {
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
    Ok(sign_entry(&host, &entry)?.to_string())
}

/// Builds an entry through a host that knows only the facts it was given.
fn build(
    facts: &HostFacts,
    seed: &str,
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
    signed(facts, &seed, entry)
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
    build(&facts, &seed, |host| {
        create_bill(host, &name, &currency, &split_mode, &creator_key)
    })
}

#[uniffi::export]
pub fn join_bill_entry(
    facts: HostFacts,
    name: Option<String>,
    pay_to: Option<String>,
    identity_key: Option<String>,
    seed: String,
) -> Result<String> {
    build(&facts, &seed, |host| {
        join_bill(
            host,
            name.as_deref(),
            pay_to.as_deref(),
            identity_key.as_deref(),
            None,
        )
    })
}

#[uniffi::export]
pub fn add_expense_entry(
    facts: HostFacts,
    expense_id: String,
    paid_by: String,
    amount: i64,
    split_json: String,
    description: Option<String>,
    seed: String,
) -> Result<String> {
    let split = parse(&split_json, "a split")?;
    build(&facts, &seed, |host| {
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
    pub note: Option<String>,
}

/// Records a claim that a debt was discharged. **A record is a claim**: §10.5
/// moves the balance only when the payee confirms.
#[uniffi::export]
pub fn record_payment_entry(
    facts: HostFacts,
    payment: PaymentDraft,
    seed: String,
) -> Result<String> {
    build(&facts, &seed, |host| {
        record_payment(
            host,
            &payment.payment_id,
            &payment.to,
            payment.amount,
            &payment.method,
            payment.reference.as_deref(),
            payment.zatoshi,
            None,
            payment.note.as_deref(),
        )
    })
}

/// Confirms a payment to this device. **Only the payee confirms** — a payer
/// who could confirm their own would settle a debt by asserting twice that
/// they paid it.
#[uniffi::export]
pub fn confirm_payment_entry(
    facts: HostFacts,
    payment_id: String,
    method: String,
    reference: Option<String>,
    seed: String,
) -> Result<String> {
    build(&facts, &seed, |host| {
        confirm_payment(host, &payment_id, &method, reference.as_deref())
    })
}

/// Snapshots a rate onto the bill (§7), so every device prices from one figure
/// rather than from whatever its own feed said.
#[uniffi::export]
pub fn set_rate_entry(
    facts: HostFacts,
    currency: String,
    minor_units_per_zec: i64,
    source: Option<String>,
    seed: String,
) -> Result<String> {
    build(&facts, &seed, |host| {
        set_rate(host, &currency, minor_units_per_zec, source.as_deref())
    })
}

#[uniffi::export]
pub fn void_entry_for(facts: HostFacts, target_id: String, seed: String) -> Result<String> {
    build(&facts, &seed, |host| void_entry(host, &target_id))
}

/// Replaces an entry this device wrote, wholesale (§10.3).
///
/// `member` names the payload's own member — `expense`, `participant`,
/// `payment` — and must be the target's own kind: an amendment carrying
/// another kind's payload deletes what it claims to correct.
#[uniffi::export]
pub fn amend_entry_for(
    facts: HostFacts,
    target_id: String,
    member: String,
    payload_json: String,
    seed: String,
) -> Result<String> {
    let payload = parse(&payload_json, "an amendment payload")?;
    build(&facts, &seed, |host| {
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

/// The bill as `entries` stands, with what the fold refused and who §10.7
/// leaves contested.
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
    let verified = Signer.prepare(entries.iter());
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
    let verified = Signer.prepare(parsed.iter());
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
/// and nothing invents a price to avoid showing that. `pay_anyway` names the
/// contested participants the payer has been shown and chosen to pay (§10.7).
///
/// `bill_id` names the bill the entries belong to, so a create for another
/// bill pushed into its channel cannot make it unopenable (§10.3).
#[uniffi::export]
pub fn obligation_of(
    facts: HostFacts,
    bill_id: String,
    entries: Vec<String>,
    pay_anyway: Vec<String>,
) -> Result<Option<ffi::PayerObligation>> {
    let parsed = parse_entries(&entries)?;
    let verified = Signer.prepare(parsed.iter());
    let verify = |entry: &Value, key: &str| verified.verify(entry, key);
    let host = FactHost {
        facts: &facts,
        sign: None,
        verify: Some(&verify),
    };
    let folded = BillLog::with_entries(&host, parsed)
        .for_bill(bill_id)
        .fold()?;
    let chosen = pay_anyway.into_iter().collect();
    Ok(obligation_for(&host, &folded, &chosen)?.map(|o| convert::obligation(&o)))
}

// --- keys a wallet keeps ----------------------------------------------------

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

/// Every entry this device holds, signed where it is this device's own and
/// sealed under the bill key — the blobs to push.
///
/// A signature is deterministic and a blob is keyed by its content, so pushing
/// the whole log every time is safe: a relay stores each entry once however
/// often it is sent.
#[uniffi::export]
pub fn blobs_to_push(
    entries: Vec<String>,
    bill_key: String,
    seed: String,
    author_id: String,
) -> Result<Vec<String>> {
    let entries = parse_entries(&entries)?;
    let seed = seed_bytes(&seed)?;
    let facts = HostFacts {
        me: author_id.clone(),
        pay_to: None,
        now: String::new(),
        nonce: Vec::new(),
    };
    let sign = |message: &[u8]| {
        Signer
            .sign(&seed, message)
            .expect("seed_bytes checked the length")
    };
    let host = FactHost {
        facts: &facts,
        sign: Some(&sign),
        verify: None,
    };
    let mut blobs = Vec::with_capacity(entries.len());
    for entry in &entries {
        let mine = entry.get("author").and_then(Value::as_str) == Some(author_id.as_str())
            && entry.get("sig").is_none();
        let to_seal = if mine {
            sign_entry(&host, entry)?
        } else {
            entry.clone()
        };
        blobs.push(Sealing.seal(&to_seal, &bill_key)?);
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
) -> Result<String> {
    let folded = folded_bill(&facts, &bill_id, &entries)?;
    Ok(invite_for(&folded.bill, &bill_key, name.as_deref(), None)?)
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
            refused_code: Some(code.to_owned()),
        },
        Scanned::Invite(invite) => ScanOutcome {
            bill_id: Some(invite.bill_id),
            bill_key: Some(invite.key),
            entries: Vec::new(),
            refused_code: None,
        },
        Scanned::Bill(scan) => ScanOutcome {
            bill_id: scan.invite.as_ref().map(|i| i.bill_id.clone()),
            bill_key: scan.invite.as_ref().map(|i| i.key.clone()),
            entries: scan.entries.iter().map(Value::to_string).collect(),
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
    let verified = Signer.prepare(parsed.iter());
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
/// debt was settled that the payee must then contest.
#[uniffi::export]
pub fn payment_entries_for_send(
    facts: HostFacts,
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
    let unpayable: std::collections::BTreeSet<&str> = obligation
        .request
        .unpayable
        .iter()
        .map(|u| u.id.as_str())
        .collect();
    let mut owed: std::collections::BTreeMap<&str, i64> = std::collections::BTreeMap::new();
    for settlement in &obligation.settlements {
        if unpayable.contains(settlement.to.as_str()) {
            continue;
        }
        *owed.entry(settlement.to.as_str()).or_insert(0) += settlement.amount;
    }
    let mut records = Vec::with_capacity(owed.len());
    for (to, amount) in owed {
        let payment_id = splitz_core::host::payment_id_for_send(&txid, to);
        records.push(build(&facts, &seed, |host| {
            record_payment(
                host,
                &payment_id,
                to,
                amount,
                "shieldedZec",
                Some(&txid),
                None,
                None,
                None,
            )
        })?);
    }
    Ok(records)
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
