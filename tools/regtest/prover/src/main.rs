//! Broadcasts a transaction built from a splitz payment request.
//!
//! Every other lane in this repository stops at the URI. This one carries it
//! the rest of the way: splitz renders a request, librustzcash's `zip321`
//! parses it back, a wallet turns that into a transaction, a regtest node
//! mines it, and the recipient's balance is read to see whether the money
//! that arrived is the money splitz said.

use anyhow::Result;

mod cache;
mod prove;

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    match args.get(1).map(String::as_str) {
        Some("miner-address") => {
            println!("{}", wallet::miner_address()?);
            Ok(())
        }
        Some("prove") => prove::run(),
        other => {
            eprintln!("usage: splitz-regtest-prover <miner-address|prove>");
            eprintln!("got: {other:?}");
            std::process::exit(2);
        }
    }
}

pub mod wallet {
    use anyhow::{anyhow, Result};
    use zcash_keys::keys::UnifiedSpendingKey;
    use zcash_protocol::consensus::BlockHeight;
    use zcash_protocol::local_consensus::LocalNetwork;
    use zcash_transparent::keys::IncomingViewingKey as _;

    /// The seed this harness funds. Test-only and fixed on purpose: the chain
    /// is thrown away after every run, and a fixed seed makes the whole proof
    /// reproducible from the commit rather than from somebody's disk.
    pub const SEED: [u8; 32] = *b"splitz regtest seed: not secret.";

    /// The chain `tools/regtest/docker-compose.yml` stands up.
    ///
    /// Regtest, not testnet. `zebrad.toml` activates every upgrade at height
    /// 1, and the wallet's own consensus parameters have to say the same: on
    /// testnet Sapling activates at 280_000, so a wallet told it is on testnet
    /// treats a 110-block chain as entirely pre-Sapling. `update_chain_tip`
    /// then returns without recording anything, the scan queue stays empty,
    /// and every later call fails with "chain height unknown" — which names
    /// the symptom and not this.
    ///
    /// Regtest and testnet share `COIN_TYPE` 1 and the transparent address
    /// prefix `[0x1d, 0x25]`, so the key derivation and the miner address are
    /// identical either way. What differs is the unified address HRP —
    /// `uregtest` against `utest` — and the activation heights.
    /// The concrete parameter type, named so signatures elsewhere can spell it.
    pub type Net = LocalNetwork;

    pub fn network() -> LocalNetwork {
        let genesis = Some(BlockHeight::from_u32(1));
        LocalNetwork {
            overwinter: genesis,
            sapling: genesis,
            blossom: genesis,
            heartwood: genesis,
            canopy: genesis,
            nu5: genesis,
            // `zebrad.toml.in` owns this list, and it stops at NU5. An
            // upgrade named here that the node does not know is not inert:
            // the wallet builds at the branch id of the latest upgrade it
            // believes active, and the node rejects the transaction with
            // "incorrect consensus branch id" after it has been proved,
            // signed and broadcast. `prove` checks the two against each
            // other before building anything.
            nu6: None,
            nu6_1: None,
            nu6_2: None,
            nu6_3: None,
        }
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
