//! Answers `tools/differential/host_generate.py`'s operations.
//!
//! One JSON object per line in, one per line out. The Dart host answers the
//! same list, and the runner diffs them: nothing in `vectors/` can cover a
//! curve operation, because a vector cannot carry a private key.

use std::io::{self, BufRead, Write};

use serde_json::{json, Value};
use splitz_core::host::{
    add_expense, create_bill, join_bill, obligation_for, set_rate, settle, BillLog, SendResult,
};
use splitz_core::{decode_bill, signing_message, SetAside};
use splitz_host::{
    activity_of, awaiting_confirmation_by, base64url_decode, base64url_encode, component_encode,
    is_well_formed_key, query_encode, BillEvent, BillStorage, BillStore, DraftItem, HostError,
    HttpTransport, InMemoryBillStorage, InMemorySecretStore, OneClickSwaps, Sealing, Signer,
    SplitDraft, SplitKind, SplitsKeys, SwapProvider, SwapQuote, SwapWatch, SystemRandomness,
    TradableAsset, WalletAccount,
};

/// A provider that answers with one scripted body and records its URLs.
struct ScriptedProvider {
    body: String,
    urls: std::cell::RefCell<Vec<String>>,
    /// How the answer echoes the request: see [`echoed`].
    echo: Option<String>,
}

impl HttpTransport for ScriptedProvider {
    fn post(&self, url: &str, sent: &str) -> Result<String, String> {
        self.urls.borrow_mut().push(url.to_owned());
        Ok(echoed(&self.body, sent, self.echo.as_deref()))
    }

    fn get(&self, url: &str) -> Result<String, String> {
        self.urls.borrow_mut().push(url.to_owned());
        Ok(self.body.clone())
    }
}

/// `body`, answering the request `sent` as a provider does when `echo` says
/// to: `asked` echoes it as `quoteRequest` and states its amount as the
/// quote's `amountIn`; `other` echoes it for another recipient.
fn echoed(body: &str, sent: &str, echo: Option<&str>) -> String {
    let Ok(Value::Object(mut answer)) = serde_json::from_str::<Value>(body) else {
        return body.to_owned();
    };
    let Some(echo) = echo else {
        return body.to_owned();
    };
    let mut request: Value = serde_json::from_str(sent).unwrap_or(Value::Null);
    if echo == "other" {
        request["recipient"] = Value::from("0xsomebodyelse");
    }
    let amount = request.get("amount").cloned().unwrap_or(Value::Null);
    answer.insert("quoteRequest".to_owned(), request);
    if let Some(Value::Object(quote)) = answer.get_mut("quote") {
        quote.entry("amountIn").or_insert(amount);
    }
    Value::Object(answer).to_string()
}

/// The swap refusals, by which one rather than by its wording.
fn swap_tag(e: &HostError) -> String {
    let HostError::Swap { message, transient } = e else {
        return "other".to_owned();
    };
    let which = if message.starts_with("A swap sends more than nothing") {
        "nothing"
    } else if message.starts_with("A swap states both") {
        "no_refund"
    } else if message.starts_with("The provider omitted") {
        "omitted"
    } else if message.starts_with("Malformed") {
        "malformed"
    } else if message.starts_with("A quote response carries the request") {
        "no_echo"
    } else if message.starts_with("The provider quoted a different") {
        "mismatch"
    } else if message.starts_with("A quote response carries") {
        "no_quote"
    } else if message.contains("could not be reached") {
        "unreachable"
    } else if message.contains("is not JSON") {
        "not_json"
    } else {
        "unclassified"
    };
    format!("{which}/{transient}")
}

fn usdc_on_base() -> TradableAsset {
    TradableAsset {
        asset_id: "nep141:base-usdc".to_owned(),
        symbol: "USDC".to_owned(),
        chain: "base".to_owned(),
        decimals: 6,
    }
}

fn quote_json(q: &SwapQuote) -> Value {
    json!({
        "depositAddress": q.deposit_address,
        "depositMemo": q.deposit_memo,
        "amountInZatoshi": q.amount_in_zatoshi,
        "amountOut": q.amount_out,
        "minAmountOut": q.min_amount_out,
        "deadline": q.deadline,
        "reference": q.reference,
        "paymentReference": q.payment_reference(),
    })
}

/// Rebuilds a split form from one operation's description of it.
fn draft_from(op: &Value) -> SplitDraft {
    let kind = match op["kind"].as_str().unwrap_or("equal") {
        "exact" => SplitKind::Exact,
        "percentage" => SplitKind::Percentage,
        "shares" => SplitKind::Shares,
        "itemized" => SplitKind::Itemized,
        _ => SplitKind::Equal,
    };
    let strings = |key: &str| -> Vec<String> {
        op[key]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default()
    };
    let weights = |key: &str| -> std::collections::BTreeMap<String, i64> {
        op[key]
            .as_object()
            .map(|o| {
                o.iter()
                    .filter_map(|(k, v)| v.as_i64().map(|n| (k.clone(), n)))
                    .collect()
            })
            .unwrap_or_default()
    };
    let mut draft = SplitDraft::new(kind);
    draft.among = strings("among").into_iter().collect();
    draft.amounts = weights("amounts");
    draft.basis_points = weights("basisPoints");
    draft.share_counts = weights("shareCounts");
    draft.extra_minor_units = op["extra"].as_i64().unwrap_or(0);
    draft.items = op["items"]
        .as_array()
        .map(|items| {
            items
                .iter()
                .map(|item| DraftItem {
                    description: item["description"].as_str().unwrap_or("").to_owned(),
                    minor_units: item["minorUnits"].as_i64().unwrap_or(0),
                    shared_by: item["sharedBy"]
                        .as_array()
                        .map(|a| {
                            a.iter()
                                .filter_map(|v| v.as_str().map(str::to_owned))
                                .collect()
                        })
                        .unwrap_or_default(),
                })
                .collect()
        })
        .unwrap_or_default();
    for id in strings("toggle") {
        draft.toggle(&id);
    }
    draft
}

/// One history line, as JSON, so the two implementations are compared line for
/// line rather than by a summary either could get wrong the same way.
fn event_json(e: &BillEvent) -> Value {
    json!({
        "entryId": e.entry_id,
        "kind": format!("{:?}", e.kind),
        "author": e.author,
        "at": e.at,
        "subject": e.subject,
        "amount": e.amount_minor_units,
        "description": e.description,
        "method": e.method,
        "reference": e.reference,
        "withdrawn": e.withdrawn,
        "refusedCode": e.refused_code,
        "confirmed": e.confirmed,
        "applied": e.applied(),
    })
}

/// The two implementations word their refusals differently; what has to match
/// is *which* refusal. Both drivers map to this vocabulary.
fn tag(e: &HostError) -> &'static str {
    let HostError::Sealing(why) = e else {
        return "other";
    };
    for (prefix, name) in [
        ("Empty blob", "empty"),
        ("Blob is format v", "version"),
        ("Blob is too short", "short"),
        ("Malformed base64url", "malformed_b64"),
        ("Key is ", "key_length"),
        ("Could not open", "auth"),
        ("Opened blob is not UTF-8", "not_utf8"),
        ("Opened blob is not JSON", "not_json"),
        ("Opened blob is not an entry", "not_entry"),
        ("An entry is not sealable", "not_sealable"),
    ] {
        if why.starts_with(prefix) {
            return name;
        }
    }
    "unclassified"
}

/// Flips a bit of a blob: 1 in the ciphertext, 2 in the version byte.
fn tamper_blob(blob: &str, how: i64) -> String {
    let Some(mut bytes) = base64url_decode(blob) else {
        return blob.to_owned();
    };
    match how {
        1 if bytes.len() > 3 => {
            let last = bytes.len() - 3;
            bytes[last] ^= 0x01;
        }
        2 if !bytes.is_empty() => bytes[0] = bytes[0].wrapping_add(1),
        _ => {}
    }
    base64url_encode(&bytes)
}

// --- a whole bill, and one participant settling it ---------------------------

/// A wallet for the settle operation: a fixed instant, a fixed transaction id,
/// and no signing. The instant is supplied per entry so both implementations
/// write the same `at` without either deriving one.
struct DiffHost {
    me: String,
    at: String,
    txid: String,
}

impl splitz_core::host::BillHost for DiffHost {
    fn me(&self) -> &str {
        &self.me
    }
    fn now(&self) -> String {
        self.at.clone()
    }
    fn random_bytes(&self, byte_count: usize) -> Vec<u8> {
        (0..byte_count).map(|i| i as u8).collect()
    }
    fn broadcast(&self, _uri: &str) -> splitz_core::host::Sent {
        if self.txid.is_empty() {
            splitz_core::host::Sent::failed(Some("no transaction id".to_owned()))
        } else {
            splitz_core::host::Sent::sent(self.txid.clone())
        }
    }
}

fn settle_records(op: &Value) -> Value {
    type Person = (String, Option<String>, Option<Vec<Value>>);
    let people: Vec<Person> = op["people"]
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            (
                p["id"].as_str().unwrap().to_owned(),
                p["payTo"].as_str().map(str::to_owned),
                p["payouts"].as_array().cloned(),
            )
        })
        .collect();
    let instants: Vec<String> = op["instants"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i.as_str().unwrap().to_owned())
        .collect();
    let me = op["me"].as_str().unwrap();
    let txid = op["txid"].as_str().unwrap_or("");

    let host_at = |step: usize, who: &str| DiffHost {
        me: who.to_owned(),
        at: instants[step].clone(),
        txid: txid.to_owned(),
    };

    let mut step = 0usize;
    let (first_id, _, _) = people[0].clone();
    let mut entries = Vec::new();
    let creator = host_at(step, &first_id);
    step += 1;
    match create_bill(
        &creator,
        "Dinner",
        "EUR",
        "equal",
        op["creatorKey"].as_str().unwrap(),
    ) {
        Ok(entry) => entries.push(entry),
        Err(e) => return json!({ "folded": false, "error": e.code }),
    }
    for (id, pay_to, payouts) in &people {
        let h = host_at(step, id);
        step += 1;
        match join_bill(
            &h,
            Some(id.as_str()),
            pay_to.as_deref(),
            None,
            payouts.clone(),
        ) {
            Ok(entry) => entries.push(entry),
            Err(e) => return json!({ "folded": false, "error": e.code }),
        }
    }
    for expense in op["expenses"].as_array().unwrap() {
        let paid_by = expense["paidBy"].as_str().unwrap();
        let h = host_at(step, paid_by);
        step += 1;
        match add_expense(
            &h,
            expense["id"].as_str().unwrap(),
            paid_by,
            expense["amount"].as_i64().unwrap(),
            json!({ "type": "equal", "among": expense["among"] }),
            None,
        ) {
            Ok(entry) => entries.push(entry),
            Err(e) => return json!({ "folded": false, "error": e.code }),
        }
    }
    if let Some(rate) = op["rate"].as_i64() {
        let h = host_at(step, &first_id);
        step += 1;
        match set_rate(&h, "EUR", rate, None) {
            Ok(entry) => entries.push(entry),
            Err(e) => return json!({ "folded": false, "error": e.code }),
        }
    }

    let host = host_at(step, me);
    let mut log = BillLog::new(&host);
    let refused = match log.add(entries) {
        Ok(refused) => refused,
        Err(e) => return json!({ "folded": false, "error": e.code }),
    };
    let folded = match log.fold() {
        Ok(folded) => folded,
        Err(e) => return json!({ "folded": false, "error": e.code }),
    };
    let refused_codes: Vec<&str> = refused.iter().map(|r| r.code).collect();
    let set_aside_codes: Vec<&str> = folded.set_aside.iter().map(|s| s.code).collect();
    let owed = match obligation_for(&host, &folded) {
        Ok(Some(owed)) => owed,
        Ok(None) => {
            return json!({
                "folded": true,
                "refused": refused_codes,
                "setAside": set_aside_codes,
                "priced": false,
            })
        }
        Err(e) => return json!({ "folded": false, "error": e.code }),
    };
    let settled = match settle(&host, &mut log, &owed) {
        Ok(settled) => settled,
        Err(e) => return json!({ "folded": false, "error": e.code }),
    };
    json!({
        "folded": true,
        "refused": refused_codes,
        "setAside": set_aside_codes,
        "priced": true,
        "settlements": owed.settlements.iter()
            .map(|s| json!({ "to": s.to, "amount": s.amount }))
            .collect::<Vec<_>>(),
        "unpayable": owed.unpayable().iter()
            .map(|u| json!({ "id": u.id, "reason": u.reason }))
            .collect::<Vec<_>>(),
        "awaiting": owed.awaiting.iter()
            .map(|a| json!({ "to": a.to, "owed": a.owed, "paid": a.paid }))
            .collect::<Vec<_>>(),
        "uri": owed.uri(),
        "result": match settled.result {
            SendResult::Sent => "sent",
            SendResult::Pending => "pending",
            SendResult::Failed => "failed",
        },
        "detail": settled.detail,
        "txid": settled.txid,
        // The whole payload, `at` included: the clock is fixed, so an instant
        // that differs is a real divergence rather than a race.
        "records": settled.records.iter().map(|r| r["payment"].clone()).collect::<Vec<_>>(),
    })
}

fn answer(op: &Value) -> Value {
    let name = op.get("op").and_then(Value::as_str).unwrap_or("");
    match name {
        "settle_records" => settle_records(op),
        "public_key" => {
            let seed = base64url_decode(op["seed"].as_str().unwrap()).unwrap_or_default();
            json!(Signer.public_key_from_seed(&seed))
        }
        "sign_entry" => {
            let seed = base64url_decode(op["seed"].as_str().unwrap()).unwrap_or_default();
            match signing_message(&op["entry"], op["bill"].as_str().unwrap_or("")) {
                Err(e) => json!({ "error": e.code }),
                Ok(message) => json!({
                    "message": message,
                    "sig": Signer.sign(&seed, message.as_bytes()),
                }),
            }
        }
        "verify" => {
            let sign_with = base64url_decode(op["signWith"].as_str().unwrap()).unwrap_or_default();
            let bill = op["bill"].as_str().unwrap_or("");
            let verify_on = op["verifyOn"].as_str().unwrap_or(bill);
            let Ok(message) = signing_message(&op["entry"], bill) else {
                return json!({ "signable": false });
            };
            let Some(mut sig) = Signer.sign(&sign_with, message.as_bytes()) else {
                return json!({ "signable": false });
            };
            if op["tamper"].as_bool().unwrap_or(false) {
                sig = tamper(&sig);
            }
            let mut entry = op["entry"].clone();
            entry["sig"] = Value::from(sig);
            let true_key = Signer.public_key_from_seed(&sign_with).unwrap_or_default();
            json!({
                "signable": true,
                "againstGivenKey": Signer.verify_entry(&entry, op["key"].as_str().unwrap(), verify_on),
                "againstTrueKey": Signer.verify_entry(&entry, &true_key, verify_on),
            })
        }
        "identity_seed" => {
            let secret = base64url_decode(op["secret"].as_str().unwrap_or("")).unwrap_or_default();
            if secret.is_empty() {
                // Random, on both sides, so there is nothing to compare.
                json!(Value::Null)
            } else {
                // Through the keychain path a wallet reaches, not a restated
                // formula: two sides each computing their own digest would
                // agree by construction.
                let store = InMemorySecretStore::default();
                let random = SystemRandomness;
                let account = WalletAccount {
                    id: "differential".to_owned(),
                    identity_secret: Some(secret),
                };
                json!(base64url_encode(
                    &SplitsKeys::new(&store, &random)
                        .ensure_identity_seed(&account)
                        .unwrap()
                ))
            }
        }
        "seal_open" => {
            let key = op["key"].as_str().unwrap();
            let blob = match Sealing.seal(&op["entry"], key) {
                Err(e) => return json!({ "sealed": false, "why": tag(&e) }),
                Ok(blob) => blob,
            };
            let presented = tamper_blob(&blob, op["tamper"].as_i64().unwrap_or(0));
            let open_with = op["openWith"].as_str().unwrap_or(key);
            match Sealing.open(&presented, open_with) {
                Ok(entry) => json!({ "sealed": true, "blob": blob, "opened": true,
                                     "entry": entry }),
                Err(e) => json!({ "sealed": true, "blob": blob, "opened": false,
                                  "why": tag(&e) }),
            }
        }
        "open_raw" => match Sealing.open(op["blob"].as_str().unwrap(), op["key"].as_str().unwrap())
        {
            Ok(entry) => json!({ "opened": true, "entry": entry }),
            Err(e) => json!({ "opened": false, "why": tag(&e) }),
        },
        "swap_encode" => {
            let text = op["text"].as_str().unwrap_or("");
            json!({ "query": query_encode(text), "component": component_encode(text) })
        }
        "swap_status" => {
            let provider = ScriptedProvider {
                body: op["body"].to_string(),
                urls: std::cell::RefCell::new(Vec::new()),
                echo: None,
            };
            let deadline = || String::new();
            let swaps = OneClickSwaps::new(
                "https://swap.example",
                "nep141:zec",
                &provider,
                &deadline,
                None,
            )
            .unwrap();
            let mut quote = SwapQuote {
                deposit_address: "u1provider".to_owned(),
                recipient: None,
                deposit_memo: op["memo"].as_str().map(str::to_owned),
                amount_in_zatoshi: 1,
                amount_out: "1".to_owned(),
                min_amount_out: None,
                asset: usdc_on_base(),
                deadline: "2026-01-01T00:00:00.000Z".to_owned(),
                reference: None,
            };
            if quote.deposit_memo.as_deref() == Some("") {
                quote.deposit_memo = Some(String::new());
            }
            match swaps.status_of(&quote) {
                Err(e) => json!({ "ok": false, "why": swap_tag(&e) }),
                Ok(status) => json!({
                    "ok": true,
                    "state": format!("{:?}", status.state),
                    "hash": status.destination_tx_hash,
                    "detail": status.detail,
                    "url": provider.urls.borrow().last(),
                }),
            }
        }
        "swap_quote" => {
            let provider = ScriptedProvider {
                body: op["body"].to_string(),
                urls: std::cell::RefCell::new(Vec::new()),
                echo: op["echo"].as_str().map(str::to_owned),
            };
            let stated = op["deadline"].as_str().unwrap_or("").to_owned();
            let deadline = move || stated.clone();
            let swaps = OneClickSwaps::new(
                "https://swap.example",
                "nep141:zec",
                &provider,
                &deadline,
                None,
            )
            .unwrap();
            match swaps.quote(
                &usdc_on_base(),
                op["amount"].as_i64().unwrap_or(0),
                op["recipient"].as_str().unwrap_or(""),
                op["refundTo"].as_str().unwrap_or(""),
            ) {
                Err(e) => json!({ "ok": false, "why": swap_tag(&e) }),
                Ok(q) => json!({ "ok": true, "quote": quote_json(&q) }),
            }
        }
        "swap_watch" => match SwapWatch::from_json(&op["json"]) {
            None => json!({ "parsed": false }),
            Some(watch) => json!({
                "parsed": true,
                "json": watch.to_json(),
                "quote": quote_json(&watch.as_quote()),
            }),
        },
        "split_draft" => {
            let draft = draft_from(op);
            let total = op["total"].as_i64().unwrap_or(0);
            json!({
                "wireType": draft.kind.wire_type(),
                "split": draft.to_split(),
                "allocation": draft.allocation(total),
                "refusalCode": draft.refusal_code(total),
                "participants": draft.participants().into_iter().collect::<Vec<_>>(),
            })
        }
        "activity" => {
            let Ok(bill) = decode_bill(&op["bill"]) else {
                return json!({ "decoded": false });
            };
            let entries: Vec<Value> = op["entries"].as_array().cloned().unwrap_or_default();
            let set_aside: Vec<SetAside> = op["setAside"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .map(|s| SetAside {
                    id: s["id"].as_str().unwrap_or_default().to_owned(),
                    // The library's codes are constants; a driver reading one
                    // from a file has to hand it a `'static` and this process
                    // is one operation list long.
                    code: Box::leak(
                        s["code"]
                            .as_str()
                            .unwrap_or_default()
                            .to_owned()
                            .into_boxed_str(),
                    ),
                })
                .collect();
            let withdrawn: Vec<String> = op["withdrawn"]
                .as_array()
                .cloned()
                .unwrap_or_default()
                .iter()
                .filter_map(|v| v.as_str().map(str::to_owned))
                .collect();
            let history = activity_of(&entries, &bill, &set_aside, &withdrawn);
            let awaiting = awaiting_confirmation_by(&bill, op["me"].as_str().unwrap_or(""));
            json!({
                "decoded": true,
                "history": history.iter().map(event_json).collect::<Vec<_>>(),
                "awaiting": awaiting.iter().map(|p| json!({
                    "id": p.id, "from": p.from, "to": p.to,
                    "amount": p.amount, "at": p.at,
                })).collect::<Vec<_>>(),
            })
        }
        "store_read" => {
            let storage = InMemoryBillStorage::default();
            storage
                .write("splitz_bill_b1", op["stored"].as_str().unwrap())
                .unwrap();
            let entries = BillStore::new(&storage).read("b1").unwrap();
            json!({ "count": entries.len(), "entries": entries })
        }
        "store_merge" => {
            let storage = InMemoryBillStorage::default();
            let store = BillStore::new(&storage);
            let held: Vec<Value> = op["held"].as_array().cloned().unwrap_or_default();
            let incoming: Vec<Value> = op["incoming"].as_array().cloned().unwrap_or_default();
            match store
                .merge("b1", held)
                .and_then(|_| store.merge("b1", incoming))
            {
                Err(_) => json!({ "merged": false }),
                Ok(merged) => json!({
                    "merged": true,
                    "ids": merged.entries.iter()
                        .map(|e| e["id"].clone()).collect::<Vec<_>>(),
                    "refused": merged.refused.len(),
                    "readBack": store.read("b1").unwrap().len(),
                }),
            }
        }
        "well_formed_key" => json!(is_well_formed_key(op["key"].as_str().unwrap())),
        "b64_round_trip" => match base64url_decode(op["text"].as_str().unwrap()) {
            None => json!(Value::Null),
            Some(raw) => json!({ "bytes": raw.len(), "reencoded": base64url_encode(&raw) }),
        },
        other => json!({ "error": format!("unknown op {other}") }),
    }
}

/// Changes one character of a signature, so it is well formed and wrong.
fn tamper(sig: &str) -> String {
    let mut chars: Vec<char> = sig.chars().collect();
    let first = chars[0];
    chars[0] = if first == 'A' { 'B' } else { 'A' };
    chars.into_iter().collect()
}

fn main() -> io::Result<()> {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let op: Value = serde_json::from_str(&line).expect("an operation is one JSON object");
        writeln!(out, "{}", serde_json::to_string(&answer(&op)).unwrap())?;
    }
    Ok(())
}
