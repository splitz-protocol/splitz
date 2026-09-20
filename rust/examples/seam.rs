//! The snippet INTEGRATING.md's "The wallet seam" section shows, compiled and
//! run. A snippet that has never been through a compiler is a claim about the
//! crate that nothing in the tree backs.

use splitz::host::{
    base64url_no_pad, create_bill, join_bill, obligation_for, settle, BillHost, BillLog, Sent,
    CREATOR_KEY_BYTES,
};
use std::collections::BTreeSet;

struct MyWallet;

impl BillHost for MyWallet {
    fn me(&self) -> &str {
        "ana"
    }
    fn pay_to_address(&self) -> Option<&str> {
        Some("u1ana000000000000000000")
    }
    // An RFC 3339 instant, not a date type: this crate depends on no calendar
    // library, and `canonical_instant` refuses anything that is not one.
    fn now(&self) -> String {
        "2026-10-28T19:30:00.000Z".to_owned()
    }
    fn random_bytes(&self, n: usize) -> Vec<u8> {
        (0..n).map(|i| (i * 7 + 1) as u8).collect()
    }
    // Synchronous, because this crate pulls in no async runtime and so cannot
    // own the executor a future would need. Block here, where you know yours.
    fn broadcast(&self, uri: &str) -> Sent {
        Sent::sent(format!("tx-{}", uri.len()))
    }
}

fn main() -> splitz::Result<()> {
    let host = MyWallet;
    let my_key = base64url_no_pad(&[0u8; CREATOR_KEY_BYTES]);

    let mut log = BillLog::new(&host);
    log.add(vec![create_bill(&host, "Dinner", "EUR", "equal", &my_key)?])?;
    log.add(vec![join_bill(
        &host,
        Some("Ana"),
        host.pay_to_address(),
        None,
        None,
    )?])?;

    let folded = log.fold()?; // §10.3, plus what it set aside
    let owed = obligation_for(&host, &folded, &BTreeSet::new())?;
    if let Some(owed) = &owed {
        settle(&host, &mut log, owed)?;
    }

    println!("bill       {}", folded.bill.id);
    println!("entries    {}", log.entries().len());
    println!("setAside   {}", folded.set_aside.len());
    println!(
        "obligation {}",
        owed.as_ref()
            .and_then(|o| o.uri().map(str::to_owned))
            .unwrap_or_else(|| "no rate yet".to_owned())
    );
    println!(
        "identities {} bound, {} contested",
        folded.identities.bound.len(),
        folded.identities.contested.len()
    );
    Ok(())
}
