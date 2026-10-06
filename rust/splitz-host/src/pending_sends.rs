//! Sends this device started and has not seen resolved (SPEC.md §14.3).
//!
//! A send can reach the network, be refused before anything is built, or be
//! built and signed and not handed to the network — and the third may still
//! land. So may one the app died during. In either case the bill holds no
//! record of it, and without a note kept outside the bill the same debt is
//! offered again and the same money goes out twice.
//!
//! The note is written **before** the wallet is called, kept in this device's
//! [`BillStorage`] under its own prefix, and blocks every further send from
//! that bill until somebody says which way it went.

use std::collections::{BTreeMap, BTreeSet};
use std::sync::Mutex;

use serde_json::{json, Map, Value};
use splitz_core::host::{payment_id_for_send, record_send, BillHost, BillLog};
use splitz_core::{decode_rate, rate_to_json, ExchangeRate, SplitError};

use crate::error::{HostError, Result};
use crate::swap_watch::SwapWatch;
use crate::transport::component_encode;
use crate::wallet::BillStorage;

/// Which of §14.3's three outcomes a send ended in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SendEnded {
    /// The transaction reached the network.
    ReachedNetwork,
    /// Nothing was built, or nothing was spent.
    Refused,
    /// Built and signed and not known to have reached the network — or the
    /// send raised, so which way it went is unknown. It may still land.
    Unresolved,
}

impl SendEnded {
    /// True when a send that ended this way takes its note with it: it was
    /// refused, or it reached the network and `recorded` says its records are
    /// on the bill. Every other ending keeps the note.
    pub fn clears_note(self, recorded: bool) -> bool {
        self == SendEnded::Refused || (self == SendEnded::ReachedNetwork && recorded)
    }
}

/// One send from one bill, written down before the wallet was called.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PendingSend {
    pub bill_id: String,
    /// The ZIP 321 request handed to the wallet. Empty only when
    /// [`PendingSend::damaged`].
    pub uri: String,
    /// What the request pays each recipient, in the bill's minor units: what
    /// a record of this send carries once its transaction is known.
    pub carried: BTreeMap<String, i64>,
    /// When the send was started, as §9.3 renders an instant.
    pub at: String,
    /// What the request sends each recipient, in zatoshi: the ZEC a record of
    /// this send states (§9.2).
    pub sent: BTreeMap<String, i64>,
    /// The rate the request was priced at: what a record states as
    /// `paidAtRate`.
    pub rate: Option<ExchangeRate>,
    /// The swap this send was the deposit for, or `None` for a payment
    /// request. A swap is recorded by the provider's reference, not a
    /// transaction id.
    pub swap: Option<SwapWatch>,
    /// What a swap's deposit sent, in zatoshi.
    pub zatoshi: Option<i64>,
    /// The transaction, when the wallet named one: the one an unresolved send
    /// built, or one that reached the network while its records could not be
    /// written.
    pub txid: Option<String>,
}

impl PendingSend {
    /// What [`PendingSends::of`] answers for a note that is there and will
    /// not read. It blocks exactly as a readable one does, and carries nothing
    /// to record from.
    pub fn damaged(bill_id: &str) -> Self {
        Self {
            bill_id: bill_id.to_owned(),
            uri: String::new(),
            carried: BTreeMap::new(),
            at: String::new(),
            sent: BTreeMap::new(),
            rate: None,
            swap: None,
            zatoshi: None,
            txid: None,
        }
    }

    /// True for a note that would not read. Nothing can be recorded from it;
    /// a person records what they paid by hand, then resolves it.
    pub fn is_damaged(&self) -> bool {
        self.uri.is_empty()
    }

    /// Whether `other` is the note of this same send: one
    /// [`PendingSends::begin`] wrote once, however its transaction was filled
    /// in since. A damaged note is nobody's.
    pub fn is_same_send(&self, other: &PendingSend) -> bool {
        !self.is_damaged() && !other.is_damaged() && self.at == other.at && self.uri == other.uri
    }

    /// The note stored for `bill_id` as `raw` holds it.
    ///
    /// **A note that will not read still blocks.** Treating it as absent
    /// would let the send it stands for go out a second time, so text that is
    /// not JSON, is not what [`PendingSend::to_json`] writes, or names another
    /// bill is answered as [`PendingSend::damaged`].
    pub fn held(bill_id: &str, raw: &str) -> Self {
        serde_json::from_str::<Value>(raw)
            .ok()
            .and_then(|v| Self::from_json(&v))
            .filter(|s| s.bill_id == bill_id)
            .unwrap_or_else(|| Self::damaged(bill_id))
    }

    /// What this note becomes once the wallet has answered (§14.3): `None`
    /// when it goes, else the note to keep.
    ///
    /// - `ReachedNetwork` and `recorded`: the bill holds the records, so the
    ///   note goes.
    /// - `ReachedNetwork` and not `recorded` — the bill was forgotten while
    ///   the money went out: the note stays, with `txid`, so the records can
    ///   be written once the bill is back.
    /// - `Refused`: nothing was spent, so the note goes and the debt can be
    ///   sent again.
    /// - `Unresolved`: the note stays, with `txid` when the wallet named the
    ///   transaction it built.
    ///
    /// A damaged note is kept unchanged: it has nothing to carry a `txid` on.
    pub fn after(&self, how: SendEnded, txid: Option<&str>, recorded: bool) -> Option<Self> {
        if how.clears_note(recorded) {
            return None;
        }
        match txid {
            Some(id) if !self.is_damaged() => Some(self.sent_as(Some(id))),
            _ => Some(self.clone()),
        }
    }

    /// The payment records for this send having gone out as the transaction
    /// `txid`, signed and appended to `log`. Recipients the bill already
    /// holds a record for under this transaction are left out: a second
    /// record under one payment id is a duplicate the fold sets aside.
    ///
    /// `txid` is trimmed and lower-cased.
    pub fn records(
        &self,
        host: &dyn BillHost,
        log: &mut BillLog<'_>,
        txid: &str,
    ) -> std::result::Result<Vec<Value>, Unrecordable> {
        let id = txid.trim().to_ascii_lowercase();
        let hex = id.len() == 64 && id.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'));
        if !hex {
            return Err(Unrecordable::NotATransactionId);
        }
        if self.is_damaged() || self.carried.is_empty() {
            return Err(Unrecordable::DetailsLost);
        }
        // A deposit sent alone is recorded by its provider's reference, not a
        // transaction id. One sent beside a request (§14.10) leaves the
        // request's half to record here, and the swap to its own record.
        let swap_to = self.swap.as_ref().map(|s| s.to.as_str());
        if swap_to.is_some() && self.sent.is_empty() {
            return Err(Unrecordable::IsASwap);
        }
        let recorded: BTreeSet<String> = log
            .fold()
            .map_err(Unrecordable::Refused)?
            .bill
            .payments
            .into_iter()
            .map(|p| p.id)
            .collect();
        let carried: BTreeMap<String, i64> = self
            .carried
            .iter()
            .filter(|(to, _)| {
                Some(to.as_str()) != swap_to
                    && !recorded.contains(&payment_id_for_send(host.me(), &id, to))
            })
            .map(|(to, amount)| (to.clone(), *amount))
            .collect();
        record_send(host, log, &carried, &id, &self.sent, self.rate.as_ref())
            .map_err(Unrecordable::Refused)
    }

    /// This send, with the transaction `id` it went out as.
    pub fn sent_as(&self, id: Option<&str>) -> Self {
        Self {
            txid: id.map(str::to_owned).or_else(|| self.txid.clone()),
            ..self.clone()
        }
    }

    pub fn to_json(&self) -> Value {
        let mut out = json!({
            "billId": self.bill_id,
            "uri": self.uri,
            "carried": self.carried,
            "at": self.at,
        });
        if !self.sent.is_empty() {
            out["sent"] = json!(self.sent);
        }
        if let Some(rate) = &self.rate {
            out["rate"] = rate_to_json(rate);
        }
        if let Some(swap) = &self.swap {
            out["swap"] = swap.to_json();
        }
        if let Some(zatoshi) = self.zatoshi {
            out["zatoshi"] = json!(zatoshi);
        }
        if let Some(txid) = &self.txid {
            out["txid"] = json!(txid);
        }
        out
    }

    /// `None` for anything [`PendingSend::to_json`] did not write.
    pub fn from_json(raw: &Value) -> Option<Self> {
        let text = |key: &str| raw.get(key).and_then(Value::as_str).map(str::to_owned);
        let uri = text("uri").filter(|u| !u.is_empty())?;
        let ints = |value: Option<&Value>| -> Option<BTreeMap<String, i64>> {
            let map: &Map<String, Value> = value?.as_object()?;
            map.iter()
                .map(|(k, v)| Some((k.clone(), v.as_i64()?)))
                .collect()
        };
        let empty = Value::Object(Map::new());
        let rate = match raw.get("rate") {
            None | Some(Value::Null) => None,
            Some(r) => Some(decode_rate(r).ok()?),
        };
        let swap = match raw.get("swap") {
            None | Some(Value::Null) => None,
            Some(s) => Some(SwapWatch::from_json(s)?),
        };
        let zatoshi = match raw.get("zatoshi") {
            None | Some(Value::Null) => None,
            Some(z) => Some(z.as_i64()?),
        };
        let txid = match raw.get("txid") {
            None | Some(Value::Null) => None,
            Some(t) => Some(t.as_str()?.to_owned()),
        };
        Some(Self {
            bill_id: text("billId")?,
            uri,
            carried: ints(raw.get("carried"))?,
            at: text("at")?,
            sent: ints(Some(raw.get("sent").unwrap_or(&empty)))?,
            rate,
            swap,
            zatoshi,
            txid,
        })
    }
}

/// Why [`PendingSends::records_for`] cannot record a send.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Unrecordable {
    /// The text is not 64 hexadecimal digits, which is how a Zcash
    /// transaction id is written.
    NotATransactionId,
    /// The note would not read, or carries nothing to record.
    DetailsLost,
    /// The send was a swap's deposit, which is recorded by the provider's
    /// reference and not by a transaction id.
    IsASwap,
    /// The records could not be written.
    Refused(SplitError),
}

/// Namespaced away from bills so a sweep of one never reaches the other.
const PREFIX: &str = "pendingsend/";

/// A transaction the wallet built itself, as [`unsent_claim_refusal`] reads
/// it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OwnTransaction {
    /// The transaction's id, as the wallet reports it.
    pub txid: String,
    /// When the wallet created it: a §9.3 instant. Fixed width, so it orders
    /// against a note's `at` by its text.
    pub created: String,
    /// What it sent out of the account, in zatoshi: the balance it took less
    /// its fee. `None` when the wallet cannot say, which makes it one that
    /// may be any send.
    pub sent: Option<i64>,
}

/// What `send` sends out of the account in all, in zatoshi: its outputs'
/// ZEC, or a swap deposit's. `None` when the note does not say.
fn send_total(send: &PendingSend) -> Option<i64> {
    if send.sent.is_empty() && send.zatoshi.is_none() {
        return None;
    }
    // Its outputs' ZEC, a swap deposit's, or both when one transaction carried
    // a request and a deposit (§14.10).
    send.sent
        .values()
        .chain(send.zatoshi.iter())
        .try_fold(
            0i64,
            |total, &z| {
                if z < 0 {
                    None
                } else {
                    total.checked_add(z)
                }
            },
        )
}

/// Why a person may not say a send left nothing in the wallet (§14.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum UnsentClaimRefusal {
    /// The wallet is still sending a transaction, and it may be this one.
    StillSending,
    /// The wallet built `txid` at or after the note was written.
    BuiltSince { txid: String },
}

/// Whether `send`'s note may be removed on a person's word that nothing left
/// the wallet, given whether the wallet is `still_sending` any transaction and
/// the transactions it built itself, `own` (§14.3). `None` when it may go.
///
/// A send killed after its broadcast and mined before the app came back is
/// no longer waiting, and its note may carry no transaction id; a
/// transaction the wallet built at or after the note was written may be it,
/// and clearing the note would let the debt go out again. Compared to the
/// second: a wallet stamps its transactions in whole seconds, and the note is
/// written before the wallet is called. A note that will not read names no
/// instant, so only `still_sending` holds it.
///
/// Only a transaction that may be this send holds it: one that sent out what
/// the note's request sends in all. A later payment of something else from
/// the same wallet is not this send, and holding the note on it would leave
/// the bill unpayable for good, with recording that transaction as this
/// payment the only way out. A transaction or a note that does not say what
/// it sent may be any send, and holds it.
pub fn unsent_claim_refusal(
    send: &PendingSend,
    still_sending: bool,
    own: &[OwnTransaction],
) -> Option<UnsentClaimRefusal> {
    if still_sending {
        return Some(UnsentClaimRefusal::StillSending);
    }
    if send.is_damaged() {
        return None;
    }
    let began = send.at.get(..19)?;
    let total = send_total(send);
    own.iter()
        .find(|t| {
            t.created.get(..19).is_some_and(|c| c >= began)
                && (t.sent.is_none() || total.is_none() || t.sent == total)
        })
        .map(|t| UnsentClaimRefusal::BuiltSince {
            txid: t.txid.clone(),
        })
}

/// Why a person may not clear a note that names its transaction (§14.3).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NamedSendRefusal {
    /// The wallet still holds the transaction and may broadcast it.
    Waiting,
    /// The wallet shows it went through: it is recorded, not cleared.
    Mined,
    /// The wallet's history could not be read, so nothing says the
    /// transaction can no longer land.
    Unread,
}

/// Whether `send`'s note may be removed on a person's word that nothing left
/// the wallet, when the note names the transaction the send built: `state` is
/// where the wallet's history shows that transaction. `None` when it may go,
/// and for a note naming no transaction, which [`unsent_claim_refusal`]
/// decides.
///
/// A transaction neither mined nor expired may still be broadcast, and one
/// mined went through; either way clearing the note lets the debt go out a
/// second time. Expired, or absent from a history that was read, it can no
/// longer land. A history that could not be read says neither, and the note
/// stays: taking a failed read for "absent" sends the debt twice.
pub fn named_send_refusal(
    send: &PendingSend,
    state: Option<TransactionState>,
) -> Option<NamedSendRefusal> {
    send.txid.as_ref()?;
    match state {
        Some(TransactionState::Waiting) => Some(NamedSendRefusal::Waiting),
        Some(TransactionState::Mined) => Some(NamedSendRefusal::Mined),
        Some(TransactionState::Unread) => Some(NamedSendRefusal::Unread),
        Some(TransactionState::Expired) | None => None,
    }
}

/// Where a transaction the wallet built stands in its history. `None` where
/// one is asked for means the history was read and does not hold it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionState {
    /// In a block: it went through.
    Mined,
    /// Not mined and not expired: the wallet may still broadcast it.
    Waiting,
    /// Expired unmined: it can no longer go through.
    Expired,
    /// The history could not be read: it may be in any state above.
    Unread,
}

/// Why this device may not withdraw its own record of a shielded payment.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnPaymentWithdrawal {
    /// The wallet shows the transaction the record names went through.
    Mined,
    /// The wallet still holds that transaction and may send it.
    Waiting,
    /// The wallet's history could not be read, so nothing says the
    /// transaction can no longer reach the payee.
    Unread,
}

/// Whether this device, `me`, may withdraw its own record of `payment`, given
/// where its wallet shows the transaction the record names, `state`: `None`
/// when it may (§14.4).
///
/// Withdrawn, the debt is offered again while the first payment has reached,
/// or may yet reach, the payee, and it is paid twice. Only a `shieldedZec`
/// record the payer wrote names a transaction this wallet can look up; a cash
/// or swap record, or one somebody else wrote, is decided by §10.8 alone, and
/// `state` `None` (a history that was read does not hold it) or expired leaves
/// it free; an unread history leaves it held.
pub fn own_payment_withdrawal_refusal(
    payment: &splitz_core::PaymentRecord,
    me: &str,
    state: Option<TransactionState>,
) -> Option<OwnPaymentWithdrawal> {
    if payment.from != me || payment.method != "shieldedZec" || payment.reference.is_none() {
        return None;
    }
    match state {
        Some(TransactionState::Mined) => Some(OwnPaymentWithdrawal::Mined),
        Some(TransactionState::Waiting) => Some(OwnPaymentWithdrawal::Waiting),
        Some(TransactionState::Unread) => Some(OwnPaymentWithdrawal::Unread),
        Some(TransactionState::Expired) | None => None,
    }
}

/// The unresolved send for each bill, at most one per bill.
///
/// **One instance per storage.** The check that a send is not already under
/// way in this process is held by the instance.
pub struct PendingSends<'a> {
    storage: &'a dyn BillStorage,
    /// The bills this instance began a send from and has not ended, released
    /// from the registry when it is dropped: the registry names a storage by
    /// its address, which a later storage may reuse.
    began: Mutex<BTreeSet<String>>,
}

impl Drop for PendingSends<'_> {
    fn drop(&mut self) {
        let began = std::mem::take(&mut *self.began.lock().unwrap());
        let mut registry = under_way().lock().unwrap();
        for bill_id in began {
            registry.remove(&self.slot(&bill_id));
        }
    }
}

/// Bills with a send between `begin` and `end`, per storage and for the whole
/// process: two instances over one storage are one wallet sending, and each
/// on its own would let the other clear the note and send the debt again.
fn under_way() -> &'static Mutex<BTreeSet<(usize, String)>> {
    static UNDER_WAY: std::sync::OnceLock<Mutex<BTreeSet<(usize, String)>>> =
        std::sync::OnceLock::new();
    UNDER_WAY.get_or_init(|| Mutex::new(BTreeSet::new()))
}

impl<'a> PendingSends<'a> {
    pub fn new(storage: &'a dyn BillStorage) -> Self {
        Self {
            storage,
            began: Mutex::new(BTreeSet::new()),
        }
    }

    /// This instance's storage, as the registry names it.
    fn slot(&self, bill_id: &str) -> (usize, String) {
        (
            self.storage as *const dyn BillStorage as *const () as usize,
            bill_id.to_owned(),
        )
    }

    /// Whether a send from `bill_id` is between `begin` and `end` in this
    /// process.
    pub fn under_way(&self, bill_id: &str) -> bool {
        under_way().lock().unwrap().contains(&self.slot(bill_id))
    }

    fn key(bill_id: &str) -> String {
        format!("{PREFIX}{}", component_encode(bill_id))
    }

    /// The unresolved send for `bill_id`, or `None`.
    ///
    /// **A note that will not read still blocks.** Treating it as absent
    /// would let the send it stands for go out a second time, so it is
    /// answered as [`PendingSend::damaged`].
    pub fn of(&self, bill_id: &str) -> Result<Option<PendingSend>> {
        match self.storage.read(&Self::key(bill_id)) {
            Ok(Some(raw)) => Ok(Some(PendingSend::held(bill_id, &raw))),
            Ok(None) => Ok(None),
            Err(HostError::Unreadable(_)) => Ok(Some(PendingSend::damaged(bill_id))),
            Err(e) => Err(e),
        }
    }

    /// Writes `send` down before the wallet is called.
    ///
    /// Refused with [`HostError::SendInFlight`] when a send from the same
    /// bill is written down and not resolved, or is between `begin` and
    /// [`PendingSends::end`] in this process. Every call that returns `Ok`
    /// MUST be followed by `end`, whatever the wallet did — including when it
    /// failed.
    pub fn begin(&self, send: &PendingSend) -> Result<()> {
        if send.is_damaged() {
            return Err(HostError::Malformed(
                "a pending send carries its request".to_owned(),
            ));
        }
        if !under_way().lock().unwrap().insert(self.slot(&send.bill_id)) {
            return Err(HostError::SendInFlight {
                bill_id: send.bill_id.clone(),
                pending: None,
            });
        }
        let written = match self.of(&send.bill_id) {
            Ok(Some(held)) => Err(HostError::SendInFlight {
                bill_id: send.bill_id.clone(),
                pending: Some(Box::new(held)),
            }),
            Ok(None) => self
                .storage
                .write(&Self::key(&send.bill_id), &send.to_json().to_string()),
            Err(e) => Err(e),
        };
        if written.is_err() {
            under_way()
                .lock()
                .unwrap()
                .remove(&self.slot(&send.bill_id));
        } else {
            self.began.lock().unwrap().insert(send.bill_id.clone());
        }
        written
    }

    /// Settles what the note says once the wallet has answered (§14.3), by
    /// [`PendingSend::after`]'s rules.
    ///
    /// `wrote` is the note `begin` was given. When it is, only that note is
    /// changed: one a later send wrote is neither deleted nor rewritten, and
    /// when the note is gone an unresolved send with a `txid` writes it back,
    /// so a transaction the wallet built always has a note naming it.
    pub fn end(
        &self,
        bill_id: &str,
        how: SendEnded,
        txid: Option<&str>,
        recorded: bool,
        wrote: Option<&PendingSend>,
    ) -> Result<()> {
        let outcome = (|| {
            let held = self.of(bill_id)?;
            if let (Some(held), Some(wrote)) = (&held, wrote) {
                if !held.is_same_send(wrote) {
                    return Ok(());
                }
            }
            if how.clears_note(recorded) {
                return match held {
                    Some(_) => self.storage.delete(&Self::key(bill_id)),
                    None => Ok(()),
                };
            }
            if txid.is_none() {
                return Ok(());
            }
            let Some(held) = held.or_else(|| {
                (how == SendEnded::Unresolved)
                    .then(|| wrote.cloned())
                    .flatten()
            }) else {
                return Ok(());
            };
            match held.after(how, txid, recorded) {
                Some(kept) if self.of(bill_id)?.as_ref() != Some(&kept) => self
                    .storage
                    .write(&Self::key(bill_id), &kept.to_json().to_string()),
                _ => Ok(()),
            }
        })();
        under_way().lock().unwrap().remove(&self.slot(bill_id));
        self.began.lock().unwrap().remove(bill_id);
        outcome
    }

    /// Removes the note for `bill_id`: its records are on the bill, or a
    /// person has said nothing left the wallet.
    ///
    /// Refused with [`HostError::SendInFlight`] while a send from `bill_id` is
    /// under way: until the wallet answers, nobody knows that nothing left it.
    /// When `seen` is given, only that note is removed, never one a later
    /// send wrote.
    pub fn resolve(&self, bill_id: &str, seen: Option<&PendingSend>) -> Result<()> {
        if self.under_way(bill_id) {
            return Err(HostError::SendInFlight {
                bill_id: bill_id.to_owned(),
                pending: None,
            });
        }
        if let Some(seen) = seen {
            if let Some(held) = self.of(bill_id)? {
                // A note that will not read names no send, so it is nobody's
                // to keep: a person who records what they paid by hand
                // clears it.
                if !held.is_damaged() && !held.is_same_send(seen) {
                    return Err(HostError::SendInFlight {
                        bill_id: bill_id.to_owned(),
                        pending: Some(Box::new(held)),
                    });
                }
            }
        }
        self.storage.delete(&Self::key(bill_id))
    }

    /// The payment records for `send` having gone out as the transaction
    /// `txid`, by [`PendingSend::records`] — to merge into the bill before
    /// [`PendingSends::resolve`].
    pub fn records_for(
        &self,
        host: &dyn BillHost,
        log: &mut BillLog<'_>,
        send: &PendingSend,
        txid: &str,
    ) -> std::result::Result<Vec<Value>, Unrecordable> {
        send.records(host, log, txid)
    }
}
