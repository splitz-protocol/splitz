//! §15's rules, run against a wallet's own implementation of each seam.
//!
//! A wallet implements the seams this crate declares — a keychain, a store, a
//! relay, a price source — and §15 states what each must do. These run those
//! rules against the real thing and answer what it did that §15 says it must
//! not. An empty answer is the only passing one.
//!
//! Each check writes under the `run_id` it is given and removes what it
//! wrote, so it can run against a wallet's real store. A relay cannot be
//! emptied, so its check uses a channel no bill has: pass a `run_id` unique to
//! the run.
//!
//! Not checked here, because nothing in one process can show it: that a
//! value outlives the process that wrote it (§15.3), and that a read refuses
//! a value that is there and cannot be read (§15.4).

use splitz_core::channel_for;

use crate::pricing::MAX_MINOR_UNITS_PER_ZEC;
use crate::wallet::{BillStorage, SecretStore, SplitsRelay, ZecPrices};

/// One thing a seam did that §15 says it must not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeamFinding {
    /// `SecretStore`, `BillStorage`, `SplitsRelay` or `ZecPrices`.
    pub seam: &'static str,
    /// The rule, as §15 states it.
    pub rule: &'static str,
    /// What the seam did instead.
    pub saw: String,
}

/// Runs `body`, and answers a finding for what it saw or refused.
fn run(
    out: &mut Vec<SeamFinding>,
    seam: &'static str,
    rule: &'static str,
    body: impl FnOnce() -> crate::Result<Option<String>>,
) {
    let saw = match body() {
        Ok(None) => return,
        Ok(Some(saw)) => saw,
        Err(e) => format!("refused: {e}"),
    };
    out.push(SeamFinding { seam, rule, saw });
}

/// §15.3's rules, against `store`.
pub fn check_secret_store(store: &dyn SecretStore, run_id: &str) -> Vec<SeamFinding> {
    const SEAM: &str = "SecretStore";
    let mut out = Vec::new();
    let key = format!("splitz-contract/{run_id}/secret");
    let reads = |want: Option<&str>| -> crate::Result<Option<String>> {
        let got = store.read(&key)?;
        Ok((got.as_deref() != want).then(|| format!("read {got:?}")))
    };
    run(&mut out, SEAM, "a key never written reads as empty", || {
        reads(None)
    });
    run(&mut out, SEAM, "a value written reads back", || {
        store.write(&key, "first")?;
        reads(Some("first"))
    });
    run(
        &mut out,
        SEAM,
        "a value written again replaces the first",
        || {
            store.write(&key, "second")?;
            reads(Some("second"))
        },
    );
    run(&mut out, SEAM, "a deleted key reads as empty", || {
        store.delete(&key)?;
        reads(None)
    });
    run(
        &mut out,
        SEAM,
        "deleting a key that is not there is not an error",
        || {
            store.delete(&key)?;
            Ok(None)
        },
    );
    out
}

/// §15.4's rules, against `storage`.
pub fn check_bill_storage(storage: &dyn BillStorage, run_id: &str) -> Vec<SeamFinding> {
    const SEAM: &str = "BillStorage";
    let mut out = Vec::new();
    let prefix = format!("splitz-contract/{run_id}/");
    let a = format!("{prefix}a");
    let b = format!("{prefix}b");
    let sibling = format!("splitz-contract/{run_id}-sibling/a");
    // Past what a short-string path holds, with a line break and characters
    // outside ASCII: an entry log is all three.
    let long = format!("{}\n{{\"a\":1}}", "é".repeat(40_000));
    run(&mut out, SEAM, "a key never written reads as empty", || {
        Ok(storage
            .read(&a)?
            .map(|got| format!("read {} characters", got.len())))
    });
    run(&mut out, SEAM, "a value written reads back whole", || {
        storage.write(&a, &long)?;
        let got = storage.read(&a)?;
        Ok((got.as_deref() != Some(long.as_str()))
            .then(|| format!("read {:?} characters", got.map(|g| g.len()))))
    });
    run(
        &mut out,
        SEAM,
        "keys answers every key under the prefix, and only those",
        || {
            storage.write(&b, "b")?;
            storage.write(&sibling, "sibling")?;
            let mut keys = storage.keys(&prefix)?;
            keys.sort();
            Ok((keys != [a.clone(), b.clone()]).then(|| format!("answered {keys:?}")))
        },
    );
    run(
        &mut out,
        SEAM,
        "a sweep leaves every finished write",
        || {
            storage.sweep_unfinished_writes()?;
            let got = storage.read(&a)?;
            Ok((got.as_deref() != Some(long.as_str()))
                .then(|| format!("read {:?} characters after it", got.map(|g| g.len()))))
        },
    );
    run(
        &mut out,
        SEAM,
        "a deleted key reads as empty and is not listed",
        || {
            storage.delete(&b)?;
            let got = storage.read(&b)?;
            let keys = storage.keys(&prefix)?;
            Ok((got.is_some() || keys.contains(&b))
                .then(|| format!("read {got:?}, listed {keys:?}")))
        },
    );
    for key in [&a, &b, &sibling] {
        // Cleaning up what the check wrote; a failure here was reported above.
        let _ = storage.delete(key);
    }
    out
}

/// §15.5's rules, against `relay`, on a channel derived from `run_id`.
pub fn check_splits_relay(relay: &dyn SplitsRelay, run_id: &str) -> Vec<SeamFinding> {
    const SEAM: &str = "SplitsRelay";
    let mut out = Vec::new();
    let channel = channel_for(&format!("splitz-contract-{run_id}"));
    let other = channel_for(&format!("splitz-contract-{run_id}-other"));
    let one = format!("contract-{run_id}-one");
    let two = format!("contract-{run_id}-two");
    run(
        &mut out,
        SEAM,
        "a channel nothing was pushed to answers empty",
        || {
            let got = relay.fetch(&channel)?;
            Ok((!got.is_empty()).then(|| format!("answered {} blob(s)", got.len())))
        },
    );
    run(&mut out, SEAM, "fetch answers every blob pushed", || {
        relay.push(&channel, &[one.clone(), two.clone()])?;
        let got = relay.fetch(&channel)?;
        Ok((!(got.contains(&one) && got.contains(&two))).then(|| format!("answered {got:?}")))
    });
    run(
        &mut out,
        SEAM,
        "pushing a blob again changes nothing",
        || {
            relay.push(&channel, std::slice::from_ref(&one))?;
            let got = relay.fetch(&channel)?;
            let copies = got.iter().filter(|b| **b == one).count();
            Ok((copies != 1 || got.len() != 2).then(|| format!("answered {got:?}")))
        },
    );
    run(&mut out, SEAM, "another channel holds none of them", || {
        let got = relay.fetch(&other)?;
        Ok((!got.is_empty()).then(|| format!("answered {got:?}")))
    });
    out
}

/// §15.6's rules, against `prices`. `priced` is a currency the source is
/// expected to price; its answer may still be empty.
pub fn check_zec_prices(prices: &dyn ZecPrices, priced: &str) -> Vec<SeamFinding> {
    const SEAM: &str = "ZecPrices";
    let mut out = Vec::new();
    run(
        &mut out,
        SEAM,
        "a code nobody prices answers empty, not an error",
        || {
            Ok(prices
                .minor_units_per_zec("ZZZ")?
                .map(|got| format!("answered {got}")))
        },
    );
    run(
        &mut out,
        SEAM,
        "an answer is a positive whole number of minor units an IEEE-754 double holds exactly",
        || {
            Ok(prices
                .minor_units_per_zec(priced)?
                .filter(|got| !(1..=MAX_MINOR_UNITS_PER_ZEC).contains(got))
                .map(|got| format!("answered {got}")))
        },
    );
    out
}
