//! Runs the language-neutral corpus in ../vectors against this crate.
//!
//! A case carries either `expect` or `error`. Refusing for the wrong reason is
//! a failure, so the code is compared and not merely the fact of a refusal.

use serde_json::{json, Value};
use splitz::error::Result;
use std::fs;

/// The corpus lives at the repository root, one level above this package, so a
/// published crate cannot carry it. `SPLITZ_VECTORS` points at a checkout.
fn vector_dir() -> String {
    std::env::var("SPLITZ_VECTORS").unwrap_or_else(|_| "../vectors".to_owned())
}

/// Panics when the corpus is not on disk.
///
/// Returning early instead would report `ok` for a suite that asserted
/// nothing, and `cargo test` captures stdout on a pass, so the notice would
/// never be read. Cargo has no skip state, so absence has to be a failure to
/// be visible at all. The package does not ship `tests/`, so this cannot fire
/// for a consumer of the published crate.
fn require_corpus() {
    let dir = vector_dir();
    assert!(
        std::path::Path::new(&dir).is_dir(),
        "no corpus at {dir}. Point SPLITZ_VECTORS at a checkout of \
         https://github.com/KamaIOps/Splitz-protocol to run the conformance \
         suite."
    );
}

fn load(name: &str) -> Value {
    let path = format!("{}/{name}", vector_dir());
    let text = fs::read_to_string(&path).unwrap_or_else(|e| panic!("cannot read {path}: {e}"));
    serde_json::from_str(&text).unwrap_or_else(|e| panic!("{path} is not JSON: {e}"))
}

/// Checks every case in `file`, running `run` on each.
fn run_cases(file: &str, run: impl Fn(&Value) -> Result<Value>) {
    require_corpus();
    let doc = load(file);
    let cases = doc["cases"].as_array().expect("cases is a list");
    assert_eq!(
        cases.len() as u64,
        doc["count"].as_u64().expect("count is a number"),
        "{file}: count is out of step"
    );

    let mut failures = Vec::new();
    for case in cases {
        let name = case["name"].as_str().unwrap_or("<unnamed>");
        let outcome = run(case);
        match (case.get("error"), case.get("expect"), outcome) {
            (Some(want), _, Err(e)) => {
                if want.as_str() != Some(e.code) {
                    failures.push(format!(
                        "{name}: refused with {} but the corpus says {want}",
                        e.code
                    ));
                }
            }
            (Some(want), _, Ok(v)) => {
                failures.push(format!(
                    "{name}: produced {v} but the corpus refuses it with {want}"
                ));
            }
            (None, Some(want), Ok(got)) => {
                if &got != want {
                    failures.push(format!("{name}:\n  got  {got}\n  want {want}"));
                }
            }
            (None, Some(_), Err(e)) => {
                failures.push(format!(
                    "{name}: refused with {} but the corpus accepts it",
                    e.code
                ));
            }
            (None, None, _) => {}
        }
    }
    assert!(
        failures.is_empty(),
        "{file}: {} of {} cases diverge\n\n{}",
        failures.len(),
        cases.len(),
        failures.join("\n\n")
    );
}

fn ints(value: &Value) -> Vec<i64> {
    value
        .as_array()
        .expect("a list of integers")
        .iter()
        .map(|v| v.as_i64().expect("an integer"))
        .collect()
}

fn rate_of(value: &serde_json::Value) -> splitz::ExchangeRate {
    splitz::ExchangeRate {
        currency: value["currency"].as_str().unwrap_or_default().to_owned(),
        minor_units_per_zec: value["minorUnitsPerZec"].as_i64().unwrap_or(0),
        at: value["at"].as_str().unwrap_or_default().to_owned(),
        source: value["source"].as_str().map(str::to_owned),
    }
}

fn payments_of(c: &serde_json::Value) -> Vec<splitz::Zip321Payment> {
    if let Some(count) = c["paymentCount"].as_u64() {
        // Carried as a count rather than a literal list; §8.2's index cap.
        let repeat = &c["repeatPayment"];
        let address = repeat["address"].as_str().unwrap_or_default().to_owned();
        let from = repeat["zatoshiFrom"].as_i64().unwrap_or(0);
        return (0..count)
            .map(|i| splitz::Zip321Payment {
                address: address.clone(),
                zatoshi: from + i as i64,
                ..Default::default()
            })
            .collect();
    }
    c["payments"]
        .as_array()
        .map(|list| {
            list.iter()
                .map(|raw| splitz::Zip321Payment {
                    address: raw["address"].as_str().unwrap_or_default().to_owned(),
                    zatoshi: raw["zatoshi"].as_i64().unwrap_or(0),
                    fiat: raw["fiat"].as_array().map(|f| splitz::FiatPrice {
                        currency: f[0].as_str().unwrap_or_default().to_owned(),
                        minor_units: f[1].as_i64().unwrap_or(0),
                    }),
                    memo: raw["memo"].as_str().map(|m| m.as_bytes().to_vec()),
                    label: raw["label"].as_str().map(str::to_owned),
                    message: raw["message"].as_str().map(str::to_owned),
                })
                .collect()
        })
        .unwrap_or_default()
}

fn bill_to_json(bill: &splitz::model::Bill) -> Value {
    json!({
        "v": splitz::BILL_VERSION,
        "id": bill.id,
        "name": bill.name,
        "currency": bill.currency,
        "splitMode": bill.split_mode,
        "participants": bill.participants.iter().map(|p| {
            let mut o = serde_json::Map::new();
            o.insert("id".into(), json!(p.id));
            o.insert("name".into(), json!(p.name));
            if let Some(a) = &p.pay_to { o.insert("payTo".into(), json!(a)); }
            if let Some(k) = &p.identity_key { o.insert("identityKey".into(), json!(k)); }
            if !p.payouts.is_empty() {
                o.insert("payouts".into(), json!(p.payouts.iter().map(|po| {
                    let mut q = serde_json::Map::new();
                    q.insert("type".into(), json!(po.kind));
                    if let Some(a) = &po.address { q.insert("address".into(), json!(a)); }
                    if let Some(a) = &po.asset { q.insert("asset".into(), json!(a)); }
                    if let Some(c) = &po.chain { q.insert("chain".into(), json!(c)); }
                    Value::Object(q)
                }).collect::<Vec<_>>()));
            }
            Value::Object(o)
        }).collect::<Vec<_>>(),
        "expenses": bill.expenses.iter().map(|e| json!({
            "id": e.id, "description": e.description, "paidBy": e.paid_by,
            "amount": e.amount, "currency": e.currency, "at": e.at, "split": e.split,
        })).collect::<Vec<_>>(),
        "payments": bill.payments.iter().map(|p| {
            let mut o = serde_json::Map::new();
            o.insert("id".into(), json!(p.id));
            o.insert("from".into(), json!(p.from));
            o.insert("to".into(), json!(p.to));
            o.insert("amount".into(), json!(p.amount));
            o.insert("currency".into(), json!(p.currency));
            o.insert("method".into(), json!(p.method));
            o.insert("at".into(), json!(p.at));
            if let Some(z) = p.zatoshi { o.insert("zatoshi".into(), json!(z)); }
            if let Some(r) = &p.paid_at_rate {
                o.insert("paidAtRate".into(), json!({
                    "currency": r.currency,
                    "minorUnitsPerZec": r.minor_units_per_zec,
                    "at": r.at,
                }));
            }
            Value::Object(o)
        }).collect::<Vec<_>>(),
        "confirmedPayments": bill.confirmed_payments.iter().collect::<Vec<_>>(),
    })
}

#[test]
fn signing() {
    run_cases("signing.json", |c| {
        Ok(json!(splitz::signing_message(&c["entry"])?))
    });
}

#[test]
fn authority() {
    run_cases("authority.json", |c| {
        let entries: Vec<Value> = c["log"].as_array().cloned().unwrap_or_default();
        let verified: std::collections::BTreeSet<String> = c["verifies"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            })
            .unwrap_or_default();
        let create = entries
            .iter()
            .find(|e| e["kind"] == "createBill")
            .expect("a create entry");
        // The curve operation is the host's; the case says what it decided.
        let r = splitz::resolve_identities(&entries, create, |e, _key| {
            verified.contains(e["id"].as_str().unwrap_or(""))
        });
        Ok(json!({
            "bound": r.bound,
            "contested": r.contested.iter().cloned().collect::<Vec<_>>(),
        }))
    });
}

// Section 14 is addressed to a host, so none of it is reachable from the wire
// format: an implementation can keep sections 1 to 12 and still ask a payer
// for a debt they have already paid.
#[test]
fn withholdings() {
    run_cases("withholdings.json", |c| {
        let bill = splitz::decode_bill(&c["bill"])?;
        let plan: Vec<splitz::settle::Settlement> = c["plan"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|s| splitz::settle::Settlement {
                        from: s["from"].as_str().unwrap_or_default().to_owned(),
                        to: s["to"].as_str().unwrap_or_default().to_owned(),
                        amount: s["amount"].as_i64().unwrap_or_default(),
                        covers: Vec::new(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let ids = |key: &str| -> std::collections::BTreeSet<String> {
            c[key]
                .as_array()
                .map(|a| {
                    a.iter()
                        .filter_map(|v| v.as_str())
                        .map(str::to_owned)
                        .collect()
                })
                .unwrap_or_default()
        };
        let w = splitz::withholdings(
            &plan,
            &bill,
            c["payer"].as_str().unwrap_or_default(),
            &ids("contested"),
            &ids("payAnyway"),
        );
        Ok(json!({
            "carried": w.carried.iter().map(|s| json!({
                "from": s.from, "to": s.to, "amount": s.amount,
            })).collect::<Vec<_>>(),
            "awaiting": w.awaiting.iter().map(|a| json!({
                "to": a.to, "owed": a.owed, "paid": a.paid,
            })).collect::<Vec<_>>(),
            "contested": w.contested.iter().map(|x| json!({
                "to": x.to, "amount": x.amount, "address": x.address,
            })).collect::<Vec<_>>(),
        }))
    });
}

#[test]
fn obligations() {
    run_cases("obligations.json", |c| {
        let bill = splitz::decode_bill(&json!({
            "v": splitz::BILL_VERSION,
            "id": "b",
            "name": "",
            "currency": c["currency"],
            "participants": c["participants"],
        }))?;
        let settlements: Vec<splitz::settle::Settlement> = c["settlements"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|s| splitz::settle::Settlement {
                        from: s["from"].as_str().unwrap_or_default().to_owned(),
                        to: s["to"].as_str().unwrap_or_default().to_owned(),
                        amount: s["amount"].as_i64().unwrap_or(0),
                        covers: Vec::new(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        let r = splitz::render_obligation(
            &settlements,
            &bill,
            &rate_of(&c["rate"]),
            c["skipUnpayable"].as_bool().unwrap_or(false),
            c["includeFiat"].as_bool().unwrap_or(false),
        )?;
        Ok(json!({
            "uri": r.uri,
            "payments": r.payments.iter().map(|p| json!({
                "address": p.address, "zatoshi": p.zatoshi,
            })).collect::<Vec<_>>(),
            "unpayable": r.unpayable.iter().map(|u| json!({
                "id": u.id, "reason": u.reason, "minorUnits": u.minor_units,
            })).collect::<Vec<_>>(),
            "carriedMinorUnits": r.carried_minor_units,
            "withheldMinorUnits": r.withheld_minor_units,
            "isComplete": r.is_complete(),
        }))
    });
}

#[test]
fn log() {
    run_cases("log.json", |c| {
        if let Some(entry) = c.get("entry") {
            splitz::check_entry(entry)?;
            return Ok(json!({"accepted": true}));
        }
        if let Some(left) = c["left"].as_array() {
            let right = c["right"].as_array().cloned().unwrap_or_default();
            let answer = |r: splitz::MergeResult| {
                json!({
                    "merged": r.merged,
                    "refused": r.refused.iter().map(|a| json!({
                        "id": a.id, "code": a.code,
                    })).collect::<Vec<_>>(),
                })
            };
            // §10.2's union is commutative, which is a claim about this
            // implementation and not only about the reference that wrote the
            // expectation. Both orders must give one answer.
            let forward = answer(splitz::merge_logs(&[left.clone(), right.clone()])?);
            let backward = answer(splitz::merge_logs(&[right, left.clone()])?);
            assert_eq!(backward, forward, "merge is not commutative");
            return Ok(forward);
        }
        let entries = c["log"].as_array().cloned().unwrap_or_default();
        // A case listing `verifies` is driven with a verifier that accepts
        // exactly those entry ids; one without is driven with none (§10.3).
        let verifies: Option<std::collections::BTreeSet<String>> =
            c["verifies"].as_array().map(|a| {
                a.iter()
                    .filter_map(Value::as_str)
                    .map(str::to_owned)
                    .collect()
            });
        let r = splitz::log::fold_log_verified(
            &entries,
            c["billId"].as_str(),
            verifies.map(|ok| {
                move |e: &Value, _k: &str| ok.contains(e["id"].as_str().unwrap_or_default())
            }),
        )?;

        // §9.1: the decoder carries confirmedPayments through, so the
        // fold's answer survives the round trip with no fixup here.
        let bill = splitz::decode_bill(&r.bill)?;

        Ok(json!({
            "identities": {
                "bound": r.identities.bound,
                "contested": r.identities.contested.iter().collect::<Vec<_>>(),
            },
            "bill": r.bill,
            "creator": r.creator,
            "replacedAddresses": r.replaced_addresses.iter().map(|a| json!({
                "id": a.id, "from": a.from, "to": a.to,
            })).collect::<Vec<_>>(),
            "withdrawn": r.withdrawn,
            // The reason is prose (SPEC.md §12); only the code is compared.
            "setAside": r.set_aside.iter().map(|a| json!({
                "id": a.id, "code": a.code,
            })).collect::<Vec<_>>(),
            "balances": splitz::net_balances(&bill)?,
        }))
    });
}

#[test]
fn invite() {
    run_cases("invite.json", |c| {
        if let Some(uri) = c["uri"].as_str() {
            let i = splitz::parse_invite(uri)?;
            let mut o = serde_json::Map::new();
            o.insert("version".into(), json!(splitz::invite::INVITE_VERSION));
            o.insert("billId".into(), json!(i.bill_id));
            o.insert("key".into(), json!(i.key));
            o.insert("name".into(), json!(i.name));
            if let Some(x) = i.expiry {
                o.insert("expiry".into(), json!(x));
            }
            Ok(Value::Object(o))
        } else {
            let raw = &c["invite"];
            Ok(json!(splitz::render_invite(&splitz::Invite {
                bill_id: raw["billId"].as_str().unwrap_or_default().to_owned(),
                key: raw["key"].as_str().unwrap_or_default().to_owned(),
                name: raw["name"].as_str().unwrap_or_default().to_owned(),
                expiry: raw["expiry"].as_i64(),
            })?))
        }
    });
}

#[test]
fn payload() {
    run_cases("payload.json", |c| {
        if let Some(spec) = c.get("encode") {
            return Ok(json!(splitz::encode_payload(
                spec["prefix"].as_str().unwrap_or_default(),
                &spec["body"],
            )?));
        }
        let p = splitz::decode_payload(c["payload"].as_str().unwrap_or_default())?;
        Ok(json!({
            "prefix": p.prefix,
            "version": p.version,
            "log": p.log,
            "invite": p.invite,
        }))
    });
}

#[test]
fn sealed() {
    run_cases("sealed.json", |c| {
        let f = splitz::parse_sealed_frame(c["frame"].as_str().unwrap_or_default())?;
        Ok(json!({
            "version": f.version,
            "nonce": f.nonce,
            "bodyBytes": f.body_bytes,
        }))
    });
}

#[test]
fn settlement() {
    run_cases("settlement.json", |c| {
        let net: std::collections::BTreeMap<String, i64> = c["balances"]
            .as_object()
            .expect("balances is an object")
            .iter()
            .map(|(k, v)| (k.clone(), v.as_i64().expect("an integer")))
            .collect();
        let limit = c["exactLimit"].as_u64().expect("a limit") as usize;
        let plan = splitz::settle_balances(&net, limit)?;
        Ok(json!({
            "settlements": plan.settlements.iter().map(|s| json!({
                "from": s.from, "to": s.to, "amount": s.amount,
            })).collect::<Vec<_>>(),
            "isOptimal": plan.is_optimal,
            "paymentCount": plan.payment_count(),
        }))
    });
}

#[test]
fn coverage() {
    run_cases("coverage.json", |c| {
        let limit = c["exactLimit"].as_u64().expect("a limit") as usize;
        // A case carries a bill or bare balances: §6.3 coverage needs the
        // debts, and net balances do not carry them.
        let plan = if c.get("bill").is_some() {
            splitz::settle_bill(&splitz::decode_bill(&c["bill"])?, limit)?
        } else {
            let net: std::collections::BTreeMap<String, i64> = c["balances"]
                .as_object()
                .expect("balances is an object")
                .iter()
                .map(|(k, v)| (k.clone(), v.as_i64().expect("an integer")))
                .collect();
            splitz::settle_balances(&net, limit)?
        };
        Ok(json!({
            "settlements": plan.settlements.iter().map(|s| json!({
                "from": s.from,
                "to": s.to,
                "amount": s.amount,
                "covers": s.covers.iter().map(|d| json!({
                    "from": d.from, "to": d.to, "amount": d.amount,
                })).collect::<Vec<_>>(),
                "rerouted": s.is_rerouted(),
                "unexplained": s.unexplained(),
            })).collect::<Vec<_>>(),
            "isOptimal": plan.is_optimal,
            "paymentCount": plan.payment_count(),
        }))
    });
}

#[test]
fn balances() {
    run_cases("balances.json", |c| {
        // §9.1: a document that omits confirmedPayments has confirmed nothing.
        let bill = splitz::decode_bill(&c["bill"])?;
        let net = splitz::net_balances(&bill)?;
        Ok(json!({
            "net": net,
            "creditors": splitz::creditors(&net).iter()
                .map(|p| json!({"id": p.id, "amount": p.amount})).collect::<Vec<_>>(),
            "debtors": splitz::debtors(&net).iter()
                .map(|p| json!({"id": p.id, "amount": p.amount})).collect::<Vec<_>>(),
            "directDebts": splitz::direct_debts(&bill)?.iter()
                .map(|d| json!({"from": d.from, "to": d.to, "amount": d.amount}))
                .collect::<Vec<_>>(),
        }))
    });
}

#[test]
fn bill_json() {
    run_cases("bill-json.json", |c| {
        let bill = splitz::decode_bill(&c["json"])?;
        let mut out = bill_to_json(&bill);
        if let Some(r) = &bill.rate {
            out.as_object_mut().expect("an object").insert(
                "rate".into(),
                json!({
                    "currency": r.currency,
                    "minorUnitsPerZec": r.minor_units_per_zec,
                    "at": r.at,
                }),
            );
        }
        Ok(out)
    });
}

#[test]
fn zip321() {
    run_cases("zip321.json", |c| {
        let payments = payments_of(c);
        let uri = splitz::render_uri(&payments, c["includeFiat"].as_bool().unwrap_or(false))?;
        if let Some(want) = c["expectLength"].as_u64() {
            assert_eq!(uri.len() as u64, want, "URI length");
            return Ok(Value::Null);
        }
        Ok(json!(uri))
    });
}

#[test]
fn rate() {
    run_cases("rate.json", |c| {
        let rate = rate_of(&c["rate"]);
        if c["direction"] == "zatoshiToFiat" {
            Ok(json!(splitz::zatoshi_to_fiat(
                c["zatoshi"].as_i64().expect("zatoshi is an integer"),
                &rate
            )?))
        } else {
            let rounding = match c["rounding"].as_str() {
                Some("down") => splitz::RateRounding::Down,
                Some("nearest") => splitz::RateRounding::Nearest,
                _ => splitz::RateRounding::Up,
            };
            Ok(json!(splitz::fiat_to_zatoshi(
                c["minorUnits"].as_i64().expect("minorUnits is an integer"),
                &rate,
                c["amountCurrency"].as_str(),
                rounding,
            )?))
        }
    });
}

#[test]
fn split_methods() {
    run_cases("split-methods.json", |c| {
        let shares = splitz::split_expense(
            c["total"].as_i64().expect("total is an integer"),
            &c["split"],
        )?;
        Ok(json!(shares))
    });
}

#[test]
fn allocation() {
    run_cases("allocation.json", |c| {
        let parts = splitz::allocate(
            c["total"].as_i64().expect("total is an integer"),
            &ints(&c["weights"]),
        )?;
        Ok(json!(parts))
    });
}
