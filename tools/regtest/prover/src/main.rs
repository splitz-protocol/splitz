//! Broadcasts a transaction built from a splitz payment request.
//!
//! Every other lane in this repository stops at the URI. This one carries it
//! the rest of the way: splitz renders a request, librustzcash's `zip321`
//! parses it back, a wallet turns that into a transaction, a regtest node
//! mines it, and the recipient's balance is read to see whether the money
//! that arrived is the money splitz said.

use anyhow::Result;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("miner-address") => {
            println!("{}", wallet::miner_address()?);
            Ok(())
        }
        other => {
            eprintln!("usage: splitz-regtest-prover <miner-address|prove>");
            eprintln!("got: {other:?}");
            std::process::exit(2);
        }
    }
}

mod wallet {
    use anyhow::{anyhow, Result};
    use zcash_keys::keys::UnifiedSpendingKey;
    use zcash_transparent::keys::IncomingViewingKey as _;
    use zcash_protocol::consensus::Network;

    /// The seed this harness funds. Test-only and fixed on purpose: the chain
    /// is thrown away after every run, and a fixed seed makes the whole proof
    /// reproducible from the commit rather than from somebody's disk.
    pub const SEED: [u8; 32] = *b"splitz regtest seed: not secret.";

    pub fn network() -> Network {
        Network::TestNetwork
    }

    pub fn usk(account: u32) -> Result<UnifiedSpendingKey> {
        let id = zip32::AccountId::try_from(account)
            .map_err(|e| anyhow!("account index {account}: {e:?}"))?;
        UnifiedSpendingKey::from_seed(&network(), &SEED, id)
            .map_err(|e| anyhow!("deriving the spending key: {e:?}"))
    }

    /// The transparent address the node mines to, so the coinbase lands
    /// somewhere this wallet can shield and then spend.
    pub fn miner_address() -> Result<String> {
        let usk = usk(0)?;
        let (addr, _) = usk
            .transparent()
            .to_account_pubkey()
            .derive_external_ivk()
            .map_err(|e| anyhow!("deriving the transparent ivk: {e:?}"))?
            .default_address();
        Ok(zcash_keys::encoding::encode_transparent_address_p(
            &network(),
            &addr,
        ))
    }
}
