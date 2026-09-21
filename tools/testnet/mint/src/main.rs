//! Mints testnet development wallets.
//!
//! A phrase is written to a file and nowhere else: not to the terminal, not to
//! a log, not into a build. What this prints is the unified address each wallet
//! receives at, which is public and is what a faucet needs.
//!
//!     cargo run --manifest-path tools/testnet/mint/Cargo.toml -- \
//!         --count 4 --out <seed-file>
//!
//! The file is one phrase per line, mode 0600, in the order
//! `splitz_host/tool/seed-driver.py` serves them: line N is index N. It
//! refuses to overwrite, because overwriting a seed file destroys whatever the
//! wallets on the old one still hold.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

use anyhow::{anyhow, bail, Context, Result};
use bip0039::{Count, English, Mnemonic};
use zcash_keys::address::UnifiedAddress;
use zcash_keys::keys::{UnifiedAddressRequest, UnifiedSpendingKey};
use zcash_protocol::consensus::{NetworkType, Parameters};

/// The wallets are testnet, so the whole file is publishable material. That is
/// the point of minting new ones rather than reusing mainnet wallets.
const NETWORK: zcash_protocol::consensus::Network =
    zcash_protocol::consensus::Network::TestNetwork;

fn main() -> Result<()> {
    let mut count: usize = 4;
    let mut out: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--count" => {
                count = args
                    .next()
                    .ok_or_else(|| anyhow!("--count needs a number"))?
                    .parse()
                    .context("--count")?
            }
            "--out" => out = Some(PathBuf::from(args.next().ok_or_else(|| anyhow!("--out needs a path"))?)),
            other => bail!("unknown argument {other}"),
        }
    }
    let out = out.ok_or_else(|| anyhow!("--out <path> is required"))?;
    if out.exists() {
        bail!(
            "{} already exists. Minting over a seed file destroys whatever the \
             wallets on the old one still hold; move it aside first.",
            out.display()
        );
    }

    let mut phrases = String::new();
    let mut addresses = Vec::new();
    for index in 0..count {
        let mnemonic = <Mnemonic<English>>::generate(Count::Words24);
        // The empty passphrase is what every wallet here derives with.
        let seed = mnemonic.to_seed("");
        let usk = UnifiedSpendingKey::from_seed(&NETWORK, &seed, zip32::AccountId::ZERO)
            .map_err(|e| anyhow!("deriving wallet {index}: {e:?}"))?;
        let (address, _) = usk
            .to_unified_full_viewing_key()
            .find_address(zip32::DiversifierIndex::new(), UnifiedAddressRequest::ALLOW_ALL)
            .map_err(|e| anyhow!("wallet {index} produced no address: {e:?}"))?;
        addresses.push(encode(&address));
        phrases.push_str(mnemonic.phrase());
        phrases.push('\n');
    }

    // 0600 before a byte is written, not after.
    let mut file = OpenOptions::new();
    file.write(true).create_new(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        file.mode(0o600);
    }
    let mut file = file
        .open(&out)
        .with_context(|| format!("creating {}", out.display()))?;
    file.write_all(phrases.as_bytes())
        .context("writing the phrases")?;

    println!("{count} testnet wallet(s) written to {}", out.display());
    println!("no phrase was printed, and none is anywhere but that file.\n");
    for (index, address) in addresses.iter().enumerate() {
        println!("  index {index}  {address}");
    }
    println!("\nFund these at a testnet faucet, then point the driver at the file:");
    println!("  python3 splitz_host/tool/seed-driver.py {} --port 39200", out.display());
    Ok(())
}

fn encode(address: &UnifiedAddress) -> String {
    address.encode(&NETWORK)
}

#[allow(dead_code)]
fn network_type() -> NetworkType {
    NETWORK.network_type()
}
