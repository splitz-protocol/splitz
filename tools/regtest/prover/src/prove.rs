//! Carries one splitz payment request the rest of the way.
//!
//! splitz renders a request, librustzcash's `zip321` parses it back, a wallet
//! turns that into a transaction, the regtest node mines it, and the
//! recipient's balance is read to see whether the money that arrived is the
//! money splitz said.

use anyhow::{anyhow, Context, Result};
use secrecy::SecretVec;
use zcash_client_backend::data_api::wallet::input_selection::{GreedyInputSelector, SpendPolicy};
use zcash_client_backend::data_api::wallet::ConfirmationsPolicy;
use zcash_client_backend::data_api::wallet::{
    create_proposed_transactions, propose_transfer, shield_transparent_funds, SpendingKeys,
};
use zcash_client_backend::data_api::{AccountBirthday, WalletRead, WalletWrite};
use zcash_client_backend::fees::{standard, DustOutputPolicy, StandardFeeRule};
use zcash_client_backend::proto::service::RawTransaction;
use zcash_client_backend::proto::service::{
    compact_tx_streamer_client::CompactTxStreamerClient, BlockId,
};
use zcash_client_backend::wallet::OvkPolicy;
use zcash_client_sqlite::util::SystemClock;
use zcash_client_sqlite::wallet::init::init_wallet_db;
use zcash_client_sqlite::WalletDb;
use zcash_keys::address::UnifiedAddress;
use zcash_keys::encoding::AddressCodec as _;
use zcash_keys::keys::UnifiedAddressRequest;
use zcash_proofs::prover::LocalTxProver;
use zcash_protocol::consensus::BlockHeight;
use zcash_protocol::value::Zatoshis;
use zcash_protocol::ShieldedPool;

use crate::cache::MemoryCache;
use crate::wallet::{network, SEED};

/// Where lightwalletd is, as `tools/regtest/docker-compose.yml` publishes it.
const LIGHTWALLETD: &str = "http://127.0.0.1:9067";

/// The height the wallet is born at. The chain is regtest and every upgrade is
/// active at height 1, so there is no history to skip and nothing is gained by
/// a later birthday — a coinbase the wallet was born after is a coinbase it
/// cannot spend.
const BIRTHDAY: u32 = 1;

/// Blocks between a coinbase output and the first height it may be spent at.
/// Zebra states it in the rejection it sends: "spends are invalid before
/// Height(155) ... 100 blocks after it was created at Height(55)".
const COINBASE_MATURITY: u32 = 100;

pub fn run() -> Result<()> {
    // `zcash_client_backend` reports what it scans through `tracing`. Without a
    // subscriber the whole sync is silent, including the part where it stops.
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
        )
        .init();

    let runtime = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .context("starting the async runtime")?;
    runtime.block_on(prove())
}

async fn prove() -> Result<()> {
    let mut client = CompactTxStreamerClient::connect(LIGHTWALLETD)
        .await
        .with_context(|| format!("connecting to lightwalletd at {LIGHTWALLETD}"))?;

    let tip = client
        .get_latest_block(tonic::Request::new(
            zcash_client_backend::proto::service::ChainSpec {},
        ))
        .await
        .context("asking lightwalletd for the chain tip")?
        .into_inner();
    println!("== chain ==");
    println!("   lightwalletd tip: height {}", tip.height);

    // The chain and the wallet have to agree about which upgrades are active.
    // They are configured in two files, so they can disagree, and the way they
    // disagree is silent until the node rejects a transaction that has already
    // been proved, signed and broadcast.
    let info = client
        .get_lightd_info(tonic::Request::new(
            zcash_client_backend::proto::service::Empty {},
        ))
        .await
        .context("asking lightwalletd what the chain is")?
        .into_inner();
    let target = BlockHeight::from_u32(tip.height as u32) + 1;
    let ours = format!(
        "{:08x}",
        u32::from(zcash_protocol::consensus::BranchId::for_height(
            &network(),
            target
        ))
    );
    println!("   node branch id:   {}", info.consensus_branch_id);
    println!("   wallet branch id: {ours} (at height {target:?})");
    if !info.consensus_branch_id.eq_ignore_ascii_case(&ours) {
        return Err(anyhow!(
            "the node is on branch {} and this wallet would build at {ours}. \
             `tools/regtest/zebrad.toml.in` sets the node's activation heights \
             and `wallet::network()` sets the wallet's; they have drifted.",
            info.consensus_branch_id
        ));
    }

    // The wallet database is thrown away with the process. A proof that only
    // reproduces on a database somebody kept is not a proof anybody can run.
    let dir = tempfile::tempdir().context("making a directory for the wallet")?;
    let db_path = dir.path().join("wallet.sqlite");
    let mut db = WalletDb::for_path(&db_path, network(), SystemClock, rand::rngs::OsRng)
        .context("opening the wallet database")?;
    init_wallet_db(&mut db, Some(SecretVec::new(SEED.to_vec())))
        .map_err(|e| anyhow!("initialising the wallet database: {e:?}"))?;

    let treestate = client
        .get_tree_state(tonic::Request::new(BlockId {
            height: u64::from(BIRTHDAY),
            hash: vec![],
        }))
        .await
        .context("asking lightwalletd for the birthday tree state")?
        .into_inner();
    let birthday = AccountBirthday::from_treestate(treestate, None)
        .map_err(|e| anyhow!("reading the birthday tree state: {e:?}"))?;

    let (account_id, usk) = db
        .create_account("splitz", &SecretVec::new(SEED.to_vec()), &birthday, None)
        .map_err(|e| anyhow!("creating the account: {e}"))?;
    println!("   account:          {account_id:?}");

    // `sync::run` sets this itself on its first pass, but the transparent
    // sweep that runs in the same pass reads it, so it is set here first.
    db.update_chain_tip(BlockHeight::from_u32(tip.height as u32))
        .map_err(|e| anyhow!("recording the chain tip: {e}"))?;
    println!(
        "   chain tip recorded: {:?}",
        db.chain_height().ok().flatten()
    );

    println!("== syncing ==");
    let cache = MemoryCache::default();
    zcash_client_backend::sync::run(&mut client, &network(), &cache, &mut db, 100)
        .await
        .map_err(|e| anyhow!("syncing: {e}"))?;

    report_balances(&db, "after the first sync")?;

    // --- shield the coinbase -------------------------------------------------
    //
    // A coinbase output can only be spent into a shielded pool, and only after
    // 100 confirmations. `run.sh up` mines past maturity; this moves the
    // matured balance into Orchard so there is something to send.
    println!("== shielding the coinbase ==");
    let params = zcash_proofs::download_sapling_parameters(Some(900))
        .map_err(|e| anyhow!("fetching the Sapling proving parameters: {e}"))?;
    println!("   spend params:  {}", params.spend.display());
    println!("   output params: {}", params.output.display());
    let prover = LocalTxProver::new(&params.spend, &params.output);

    let miner = crate::wallet::miner_address()?;
    let miner_addr = zcash_transparent::address::TransparentAddress::decode(&network(), &miner)
        .map_err(|e| anyhow!("decoding the miner address {miner}: {e}"))?;

    let input_selector = GreedyInputSelector::new();
    let change_strategy = standard::SingleOutputChangeStrategy::new(
        StandardFeeRule::Zip317,
        None,
        ShieldedPool::Orchard,
        DustOutputPolicy::default(),
    );
    let keys = SpendingKeys::from_unified_spending_key(usk);

    let shielded = shield_transparent_funds(
        &mut db,
        &network(),
        &prover,
        &prover,
        &input_selector,
        &change_strategy,
        Zatoshis::const_from_u64(1),
        &keys,
        &[miner_addr],
        account_id,
        // A coinbase output cannot be spent until 100 blocks after the one
        // that created it, and the input selector does not know that: with a
        // one-confirmation policy it picks the largest UTXO it can see, which
        // on a freshly mined chain is an immature one, and the node rejects
        // the transaction after it has been proved and signed. Asking for 100
        // confirmations and refusing zero-conf shielding leaves exactly the
        // matured coinbase.
        ConfirmationsPolicy::new_symmetrical(
            std::num::NonZeroU32::new(COINBASE_MATURITY).expect("100 is not zero"),
            false,
        ),
    )
    .map_err(|e| anyhow!("shielding the coinbase: {e}"))?;
    println!("   {} shielding transaction(s)", shielded.len());
    submit(&mut client, &db, &shielded).await?;
    // More than one block: a note is not spendable the instant it is mined.
    // The wallet needs the commitment tree to have caught up to the anchor it
    // will build the spend against, and on a chain this short that costs a
    // few blocks rather than none.
    println!("   mined to height {}", generate(12)?);
    resync(&mut client, &cache, &mut db).await?;
    report_balances(&db, "after shielding")?;

    // --- the part this harness exists for ------------------------------------
    //
    // A second account stands in for the payee. It is in the same wallet so
    // its balance can be read directly, which is the only check that answers
    // "does the money arrive".
    let (payee_id, _) = db
        .create_account("payee", &SecretVec::new(SEED.to_vec()), &birthday, None)
        .map_err(|e| anyhow!("creating the payee account: {e}"))?;
    let (payee_address, _) = db
        .get_next_available_address(payee_id, UnifiedAddressRequest::ALLOW_ALL)
        .map_err(|e| anyhow!("deriving the payee's address: {e}"))?
        .ok_or_else(|| anyhow!("the payee account produced no address"))?;
    let payee_ua = UnifiedAddress::encode(&payee_address, &network());
    println!("== the bill ==");
    println!("   payee: {payee_ua}");

    let (uri, owed_minor_units, owed_zatoshi) = splitz_request(&payee_ua)?;
    println!("   splitz says ana owes ben {owed_minor_units} minor units");
    println!("   which at the bill's rate is {owed_zatoshi} zatoshi");
    println!("   {uri}");

    // Parsed by librustzcash's own reader, not by splitz. A URI only splitz
    // can read is a URI no wallet can send.
    let request = zip321::TransactionRequest::from_uri(&uri)
        .map_err(|e| anyhow!("librustzcash refused the request splitz rendered: {e:?}"))?;

    println!("== sending ==");

    // The wallet will not spend a note it cannot anchor, and it cannot anchor
    // one until the scan queue says the chain below it is fully scanned. On
    // this chain it never does: `suggest_scan_ranges` keeps returning one
    // `Historic` range covering the whole chain however many times it is
    // scanned, `block_fully_scanned` stays `None`, and the anchor
    // `get_target_and_anchor_heights` hands the input selector sits at the end
    // of the first batch. Every note received above that height is invisible
    // to selection, which is reported as "insufficient balance (have 0)" even
    // though the same notes are counted as spendable in the wallet summary.
    //
    // Checked here rather than at the failure, because the failure names the
    // symptom and not this.
    let fully_scanned = db
        .block_fully_scanned()
        .map_err(|e| anyhow!("reading the fully-scanned height: {e}"))?
        .map(|m| m.block_height());
    let heights = db
        .get_target_and_anchor_heights(ConfirmationsPolicy::default().trusted())
        .map_err(|e| anyhow!("reading the target and anchor heights: {e}"))?;
    println!("   fully scanned: {fully_scanned:?}");
    println!("   target/anchor: {heights:?}");
    if fully_scanned.is_none() {
        let ranges = db
            .suggest_scan_ranges()
            .map_err(|e| anyhow!("reading the scan queue: {e}"))?;
        for range in ranges.iter().take(4) {
            println!(
                "   still suggested: {:?} {:?}",
                range.block_range(),
                range.priority()
            );
        }
        return Err(anyhow!(
            "the wallet has no fully-scanned height, so no shielded note can be \
             anchored and the transfer cannot be built. Everything above this \
             line ran: the chain, the sync, a shielding transaction broadcast \
             and mined, and the request splitz rendered parsed by librustzcash. \
             See tools/regtest/README.md."
        ));
    }

    let proposal = propose_transfer::<_, _, _, _, std::convert::Infallible>(
        &mut db,
        &network(),
        account_id,
        &input_selector,
        &change_strategy,
        request,
        ConfirmationsPolicy::default(),
        &SpendPolicy::default(),
        None,
        None,
    )
    .map_err(|e| anyhow!("proposing the transfer: {e}"))?;

    let sent = create_proposed_transactions::<
        _,
        _,
        std::convert::Infallible,
        _,
        std::convert::Infallible,
        _,
    >(
        &mut db,
        &network(),
        &prover,
        &prover,
        &keys,
        OvkPolicy::Sender,
        &proposal,
        None,
    )
    .map_err(|e| anyhow!("building the transaction: {e}"))?;
    submit(&mut client, &db, &sent).await?;
    println!("   mined to height {}", generate(1)?);
    resync(&mut client, &cache, &mut db).await?;
    report_balances(&db, "after the payment")?;

    // The claim, checked: what the payee holds is what splitz said they were
    // owed, to the zatoshi.
    let summary = db
        .get_wallet_summary(ConfirmationsPolicy::MIN)
        .map_err(|e| anyhow!("reading the wallet summary: {e}"))?
        .ok_or_else(|| anyhow!("no wallet summary after the payment"))?;
    let arrived = summary
        .account_balances()
        .get(&payee_id)
        .map(|b| u64::from(b.orchard_balance().total()))
        .unwrap_or(0);
    println!("== the check ==");
    println!("   splitz said:  {owed_zatoshi} zatoshi");
    println!("   payee holds:  {arrived} zatoshi");
    if arrived != owed_zatoshi {
        return Err(anyhow!(
            "the payee holds {arrived} zatoshi and splitz said {owed_zatoshi}"
        ));
    }
    println!("   the money that arrived is the money splitz said.");
    Ok(())
}

/// One payer's obligation on a two-person bill, as splitz renders it.
///
/// Built through the library's own public surface — decode, settle, render —
/// so what is broadcast is what a wallet integrating splitz would broadcast,
/// not a URI assembled here for the purpose.
fn splitz_request(payee_address: &str) -> Result<(String, i64, u64)> {
    let document = serde_json::json!({
        "v": 1,
        "id": "b1",
        "name": "Dinner",
        "currency": "EUR",
        "splitMode": "equal",
        "participants": [
            { "id": "ana", "name": "Ana" },
            { "id": "ben", "name": "Ben", "payTo": payee_address },
        ],
        "expenses": [{
            "id": "x1",
            "paidBy": "ben",
            "amount": 9000,
            "at": "2026-10-28T19:30:00.000Z",
            "split": { "type": "equal", "among": ["ana", "ben"] },
        }],
        "payments": [],
        "rate": {
            "currency": "EUR",
            "minorUnitsPerZec": 51234,
            "at": "2026-10-28T19:30:00.000Z",
        },
    });

    let bill = splitz_core::decode_bill(&document).map_err(|e| anyhow!("decoding the bill: {e}"))?;
    let rate = bill
        .rate
        .clone()
        .ok_or_else(|| anyhow!("the bill carries no rate"))?;
    let plan = splitz_core::settle_bill(&bill, splitz_core::DEFAULT_EXACT_LIMIT)
        .map_err(|e| anyhow!("settling the bill: {e}"))?;
    let mine: Vec<splitz_core::Settlement> = plan
        .settlements
        .iter()
        .filter(|s| s.from == "ana")
        .cloned()
        .collect();
    let owed: i64 = mine.iter().map(|s| s.amount).sum();

    // `include_fiat` is off: the `fiat=` parameter is splitz's own and ZIP 321
    // does not define it, so a request carrying it is not a request every
    // wallet can read.
    let obligation = splitz_core::render_obligation(&mine, &bill, &rate, true, false)
        .map_err(|e| anyhow!("rendering the obligation: {e}"))?;
    let uri = obligation
        .uri
        .ok_or_else(|| anyhow!("the obligation carries nothing to send"))?;
    // Taken from the request itself rather than converted a second time: the
    // amount in the URI is the amount the wallet will send, and a figure
    // derived alongside it is a second answer that can differ from it.
    let zatoshi: i64 = obligation.payments.iter().map(|p| p.zatoshi).sum();
    Ok((uri, owed, u64::try_from(zatoshi).unwrap_or(0)))
}

/// Hands each transaction to lightwalletd and reads what it says back.
///
/// `create_proposed_transactions` stores a transaction in the wallet; it does
/// not broadcast one. A run that stopped there would report a transaction that
/// no node has ever seen.
async fn submit<DbT: WalletRead>(
    client: &mut CompactTxStreamerClient<tonic::transport::Channel>,
    db: &DbT,
    txids: &nonempty::NonEmpty<zcash_protocol::TxId>,
) -> Result<()>
where
    DbT::Error: std::fmt::Display,
{
    for txid in txids.iter() {
        let tx = db
            .get_transaction(*txid)
            .map_err(|e| anyhow!("reading {txid} back from the wallet: {e}"))?
            .ok_or_else(|| anyhow!("the wallet built {txid} and then did not store it"))?;
        let mut raw = Vec::new();
        tx.write(&mut raw)
            .with_context(|| format!("serialising {txid}"))?;
        let answer = client
            .send_transaction(RawTransaction {
                data: raw,
                height: 0,
            })
            .await
            .with_context(|| format!("broadcasting {txid}"))?
            .into_inner();
        if answer.error_code != 0 {
            return Err(anyhow!(
                "the node refused {txid}: code {} — {}",
                answer.error_code,
                answer.error_message
            ));
        }
        println!("   broadcast {txid}");
    }
    Ok(())
}

async fn resync(
    client: &mut CompactTxStreamerClient<tonic::transport::Channel>,
    cache: &MemoryCache,
    db: &mut WalletDb<rusqlite::Connection, crate::wallet::Net, SystemClock, rand::rngs::OsRng>,
) -> Result<()> {
    zcash_client_backend::sync::run(client, &network(), cache, db, 100)
        .await
        .map_err(|e| anyhow!("syncing: {e}"))
}

fn report_balances<DbT: WalletRead>(db: &DbT, when: &str) -> Result<()>
where
    DbT::Error: std::fmt::Display,
{
    let summary = db
        .get_wallet_summary(ConfirmationsPolicy::MIN)
        .map_err(|e| anyhow!("reading the wallet summary: {e}"))?
        .ok_or_else(|| anyhow!("the wallet reports no summary after a completed sync"))?;
    println!("   balances {when} (tip {:?}):", summary.chain_tip_height());
    for (id, balance) in summary.account_balances() {
        let orchard = balance.orchard_balance();
        println!(
            "     {id:?}: orchard total {} (spendable {}, pending spendability {}, \
             change pending {}) sapling {} unshielded {}",
            u64::from(orchard.total()),
            u64::from(orchard.spendable_value()),
            u64::from(orchard.value_pending_spendability()),
            u64::from(orchard.change_pending_confirmation()),
            u64::from(balance.sapling_balance().total()),
            u64::from(balance.unshielded_balance().total()),
        );
    }
    Ok(())
}

/// Mines `count` blocks on the regtest chain.
///
/// Zebra's released image is built without the internal miner, so blocks come
/// from the `generate` RPC rather than from a miner thread.
fn generate(count: u32) -> Result<u64> {
    let body = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "generate",
        "params": [count],
    });
    let response: serde_json::Value = ureq::post("http://127.0.0.1:18232/")
        .send_json(body)
        .context("asking the node to mine")?
        .into_json()
        .context("reading the node's answer")?;
    if let Some(error) = response.get("error").filter(|e| !e.is_null()) {
        return Err(anyhow!("the node refused to mine: {error}"));
    }
    let height: serde_json::Value = ureq::post("http://127.0.0.1:18232/")
        .send_json(serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": "getblockcount", "params": []
        }))
        .context("asking the node for its height")?
        .into_json()
        .context("reading the node's height")?;
    height
        .get("result")
        .and_then(serde_json::Value::as_u64)
        .ok_or_else(|| anyhow!("the node's height is not a number: {height}"))
}
