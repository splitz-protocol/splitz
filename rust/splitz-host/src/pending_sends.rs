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
        if self.swap.is_some() {
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
            .filter(|(to, _)| !recorded.contains(&payment_id_for_send(&id, to)))
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

/// The unresolved send for each bill, at most one per bill.
///
/// **One instance per storage.** The check that a send is not already under
/// way in this process is held by the instance.
pub struct PendingSends<'a> {
    storage: &'a dyn BillStorage,
    under_way: Mutex<BTreeSet<String>>,
}

impl<'a> PendingSends<'a> {
    pub fn new(storage: &'a dyn BillStorage) -> Self {
        Self {
            storage,
            under_way: Mutex::new(BTreeSet::new()),
        }
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
        if !self.under_way.lock().unwrap().insert(send.bill_id.clone()) {
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
            self.under_way.lock().unwrap().remove(&send.bill_id);
        }
        written
    }

    /// Settles what the note says once the wallet has answered (§14.3), by
    /// [`PendingSend::after`]'s rules.
    pub fn end(
        &self,
        bill_id: &str,
        how: SendEnded,
        txid: Option<&str>,
        recorded: bool,
    ) -> Result<()> {
        let outcome = (|| {
            if how.clears_note(recorded) {
                return self.storage.delete(&Self::key(bill_id));
            }
            if txid.is_none() {
                return Ok(());
            }
            let Some(held) = self.of(bill_id)? else {
                return Ok(());
            };
            match held.after(how, txid, recorded) {
                Some(kept) if kept != held => self
                    .storage
                    .write(&Self::key(bill_id), &kept.to_json().to_string()),
                _ => Ok(()),
            }
        })();
        self.under_way.lock().unwrap().remove(bill_id);
        outcome
    }

    /// Removes the note for `bill_id`: its records are on the bill, or a
    /// person has said nothing left the wallet.
    pub fn resolve(&self, bill_id: &str) -> Result<()> {
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
