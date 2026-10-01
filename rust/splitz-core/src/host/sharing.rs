//! Getting a bill from one phone to another.
//!
//! Two scans and no network: an invite carries the bill's id and the key its
//! contents are encrypted under, and a payload carries the log itself so a
//! joiner holds a bill rather than a name to go looking for.
//!
//! This layer carries the key; it does not encrypt. §11.3 puts the cipher in
//! the wallet, and nothing here pretends otherwise.

use serde_json::{Map, Value};
use std::collections::BTreeSet;

use crate::error::{code, Result};
use crate::invite::{
    decode_payload, delta_for as protocol_delta_for, encode_payload, parse_invite, render_invite,
    strip_scan_padding, Delta, Invite, BILL_PREFIX, DELTA_PREFIX, PAYLOAD_VERSION,
};
use crate::log::SetAside;
use crate::model::Bill;

use super::bill_log::BillLog;

/// A bill and, on the full form, the invite that opens it.
///
/// Its own type rather than a variant's members so that
/// [`accept_scan`] can take only this: an invite carries no entries and a
/// refusal carries nothing at all, and a signature that admits either would
/// need a refusal for a caller mistake the type system can decide.
#[derive(Debug, Clone, PartialEq)]
pub struct ScannedBill {
    pub entries: Vec<Value>,
    /// Present on a `splitz1:` payload, absent on a `splitzd1:` delta — a
    /// delta is for a reader that already holds the key (§11.2).
    pub invite: Option<Invite>,
}

/// What a scan produced.
#[derive(Debug, Clone, PartialEq)]
pub enum Scanned {
    /// An invite: a bill's id and its key, and nothing else. The log still has
    /// to arrive from somewhere.
    Invite(Invite),
    /// A payload carrying a log.
    Bill(ScannedBill),
    /// What a scan could not be read as, named by a §12 code. A wallet's
    /// message is derived from this, never written beside it.
    Refused(&'static str),
}

/// Reads whatever a camera or a clipboard produced.
///
/// One entry point because a person points a camera at a square and does not
/// know which kind it is. Tried in order: a payload carries more, so it is
/// tried first.
pub fn read_scan(text: &str) -> Scanned {
    match decode_payload(text) {
        Ok(payload) => {
            let invite = payload.invite.as_ref().and_then(reparse_invite);
            // §9.4: a key handed over with a bill it was not made for opens a
            // version of the bill only its holder sees.
            if let Some(invite) = &invite {
                let mismatched = payload
                    .log
                    .iter()
                    .any(|e| crate::invite::create_refuses_key(e, &invite.bill_id, &invite.key));
                if mismatched {
                    return Scanned::Refused(code::INVITE_KEY_MISMATCH);
                }
            }
            Scanned::Bill(ScannedBill {
                entries: payload.log,
                invite,
            })
        }
        Err(e) => {
            // Text claiming to be a payload is answered as one. Falling
            // through to the invite parser would hand a person "not an invite"
            // for a bill QR that is merely damaged, which names the wrong
            // thing to fix.
            let trimmed = strip_scan_padding(text);
            if trimmed.starts_with(BILL_PREFIX) || trimmed.starts_with(DELTA_PREFIX) {
                return Scanned::Refused(e.code);
            }
            // Not a payload at all. It may still be an invite.
            match parse_invite(text) {
                Ok(invite) => Scanned::Invite(invite),
                Err(e) => Scanned::Refused(e.code),
            }
        }
    }
}

/// The invite a payload carried, when it carried one this reader can use.
///
/// §11.2 carries the invite verbatim and validates nothing inside it, so every
/// member here is whatever a peer wrote — a number, a list, absent. Each is
/// tested before use, never assumed: this is reached by pointing a camera at a
/// square somebody else made.
///
/// §11.1 is what decides whether it is an invite, so it goes through the real
/// parser rather than being read member by member here.
fn reparse_invite(raw: &Value) -> Option<Invite> {
    let bill_id = raw.get("b")?.as_str()?;
    let key = raw.get("k")?.as_str()?;
    let invite = Invite {
        bill_id: bill_id.to_owned(),
        key: key.to_owned(),
        name: String::new(),
        expiry: None,
    };
    parse_invite(&render_invite(&invite).ok()?).ok()
}

/// The invite for a bill this device holds.
///
/// The key is the wallet's: §11.1 says the invite carries it and nothing here
/// mints one. `expiry` is seconds since the Unix epoch, as §11.1 writes one.
pub fn invite_for(
    bill: &Bill,
    key: &str,
    name: Option<&str>,
    expiry: Option<i64>,
) -> Result<String> {
    render_invite(&Invite {
        bill_id: bill.id.clone(),
        key: key.to_owned(),
        name: name.unwrap_or("").to_owned(),
        expiry,
    })
}

/// One square carrying the whole bill, or `None` when it has outgrown a scan.
///
/// §11.2 caps a payload, and a bill with several people carrying payout
/// addresses reaches that cap quickly — two people and one expense, or three
/// people and none. A bill past it needs a relay, and this returns `None`
/// rather than a code so a caller has a state to show instead of an error to
/// report.
pub fn shareable_bill(log: &BillLog<'_>, key: &str, bill: &Bill) -> Option<String> {
    let mut invite = Map::new();
    invite.insert("v".to_owned(), Value::from(PAYLOAD_VERSION));
    invite.insert("b".to_owned(), Value::from(bill.id.clone()));
    invite.insert("k".to_owned(), Value::from(key));

    let mut body = Map::new();
    body.insert("v".to_owned(), Value::from(PAYLOAD_VERSION));
    body.insert("invite".to_owned(), Value::Object(invite));
    body.insert("log".to_owned(), Value::Array(log.entries()));

    encode_payload(BILL_PREFIX, &Value::Object(body)).ok()
}

/// What `they_have` is missing, as one square when it fits (§14.5).
///
/// Delegates to the protocol: which entries a peer lacks and whether they fit
/// §11.2's cap is a function of the log, so a second implementation here is a
/// second place for the rule to drift.
pub fn delta_for(log: &BillLog<'_>, they_have: &BTreeSet<String>) -> Delta {
    protocol_delta_for(&log.entries(), they_have)
}

/// Folds a scanned bill into this device's own log, reporting what it refused.
pub fn accept_scan(log: &mut BillLog<'_>, scan: ScannedBill) -> Result<Vec<SetAside>> {
    log.add(scan.entries)
}
