//! The 1Click API as the provider publishes it, for tests to hold the client
//! to.
//!
//! `tools/contracts/oneclick.json` is the provider's own schema, pinned by
//! `tools/contracts/oneclick.py`; `tools/contracts/fixtures/` holds its raw
//! answers, captured by `splitz_host/tool/oneclick_live.dart`. Neither is
//! written by hand.
//!
//! Each test binary compiles this file separately and uses a different part
//! of it, so what one leaves unused is not dead.
#![allow(dead_code)]

use std::path::PathBuf;

use serde_json::Value;

fn contracts() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tools/contracts")
}

pub fn schemas() -> Value {
    let text = std::fs::read_to_string(contracts().join("oneclick.json")).unwrap();
    serde_json::from_str::<Value>(&text).unwrap()["schemas"].clone()
}

/// A captured answer from the live API, as text.
pub fn fixture_text(name: &str) -> String {
    std::fs::read_to_string(contracts().join("fixtures").join(name)).unwrap()
}

pub fn fixture(name: &str) -> Value {
    serde_json::from_str(&fixture_text(name)).unwrap()
}

/// What the provider would refuse in `body`, as a quote request; empty when
/// it would accept it.
///
/// The schema's `required` list, its declared properties, their types and
/// enums, and the bound the live API states for `slippageTolerance` when it
/// refuses one (`fixtures/quote_refused.json`): an integer from 0 to 10000.
pub fn quote_request_problems(body: &Value) -> Vec<String> {
    let schema = &schemas()["QuoteRequest"];
    let props = schema["properties"].as_object().unwrap();
    let body = body.as_object().expect("a quote request is an object");
    let mut problems: Vec<String> = schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(Value::as_str)
        .filter(|r| !body.contains_key(*r))
        .map(|r| format!("{r} is required"))
        .collect();
    for (key, value) in body {
        let Some(p) = props.get(key) else {
            problems.push(format!("{key} is not a property the API declares"));
            continue;
        };
        let kind = p["type"].as_str().unwrap_or("");
        let ok = match kind {
            "string" => value.is_string(),
            "number" => value.is_number(),
            "boolean" => value.is_boolean(),
            "array" => value.is_array(),
            _ => true,
        };
        if !ok {
            problems.push(format!("{key} is not a {kind}"));
        }
        if let Some(allowed) = p["enum"].as_array() {
            if !allowed.contains(value) {
                problems.push(format!("{key} {value} is not one of the enum"));
            }
        }
    }
    if let Some(s) = body.get("slippageTolerance").and_then(Value::as_f64) {
        if s.fract() != 0.0 || !(0.0..=10000.0).contains(&s) {
            problems.push("slippageTolerance must be an integer from 0 to 10000".to_owned());
        }
    }
    problems
}

/// Whether `path` — `Schema.field.field`, `[]` stepping into an array's
/// items — is something the schema declares.
pub fn declares(path: &str) -> bool {
    let schemas = schemas();
    let resolve = |node: &Value| -> Value {
        let reference = node["$ref"]
            .as_str()
            .or_else(|| node["allOf"][0]["$ref"].as_str());
        match reference {
            Some(r) => schemas[r.rsplit('/').next().unwrap()].clone(),
            None => node.clone(),
        }
    };
    let mut parts = path.split('.');
    let mut node = schemas[parts.next().unwrap()].clone();
    for part in parts {
        let (name, list) = match part.strip_suffix("[]") {
            Some(n) => (n, true),
            None => (part, false),
        };
        node = resolve(&node)["properties"][name].clone();
        if node.is_null() {
            return false;
        }
        if list {
            node = resolve(&node)["items"].clone();
        }
    }
    !node.is_null()
}
