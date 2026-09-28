//! The protocol-level walkthrough INTEGRATING.md's "Rust" section shows,
//! compiled and run by tools/examples/run.sh. `tools/docs/blocks.py` fails
//! when the document and this file drift.
// docs:begin
use splitz_core::{decode_bill, fiat_to_zatoshi, render_uri, settle_bill, FiatPrice};
use splitz_core::{RateRounding, Zip321Payment, DEFAULT_EXACT_LIMIT};

fn main() -> splitz_core::Result<()> {
    // Ben paid 30.00 split with Ana, so Ana owes him 15.00; the bill carries
    // the rate §7 snapshotted onto it. A wallet has this from a fold.
    let bill = decode_bill(&serde_json::json!({
        "v": 1, "id": "b1", "currency": "EUR",
        "participants": [
            {"id": "ana", "name": "Ana", "payTo": "u1ana0000000000000000000"},
            {"id": "ben", "name": "Ben", "payTo": "u1ben0000000000000000000"}
        ],
        "expenses": [{
            "id": "x1", "paidBy": "ben", "amount": 3000,
            "at": "2026-10-28T19:30:00.000Z",
            "split": {"type": "equal", "among": ["ana", "ben"]}
        }],
        "rate": {"currency": "EUR", "minorUnitsPerZec": 300000,
                 "at": "2026-10-28T19:32:00.000Z"}
    }))?;

    let plan = settle_bill(&bill, DEFAULT_EXACT_LIMIT)?;
    // A bill with no `setRate` entry is an ordinary bill; this is a branch,
    // not an `expect`.
    let Some(rate) = bill.rate.as_ref() else {
        return Ok(());
    };
    let mut payments = Vec::new();
    for settlement in plan.settlements.iter().filter(|s| s.from == "ana") {
        let who = bill
            .participant(&settlement.to)
            .expect("the plan names participants");
        // Reported, never dropped: a dropped output settles less than the
        // plan says it does, and the payer cannot tell.
        let Some(address) = who.payable_address() else {
            println!("cannot pay {} here", settlement.to);
            continue;
        };
        payments.push(Zip321Payment {
            address: address.to_owned(),
            zatoshi: fiat_to_zatoshi(
                settlement.amount,
                rate,
                Some(&bill.currency),
                RateRounding::Up,
            )?,
            fiat: Some(FiatPrice {
                currency: bill.currency.clone(),
                minor_units: settlement.amount,
            }),
            label: Some(who.name.clone()),
            ..Default::default()
        });
    }
    let uri = render_uri(&payments, true)?; // one transaction
    println!("request: {uri}");
    assert!(uri.starts_with("zcash:u1ben"), "{uri}");
    Ok(())
}
