//! A compact-block cache that lives in memory.
//!
//! `zcash_client_backend::sync::run` needs a [`BlockCache`], and neither
//! published crate ships one outside its own test-dependencies feature. The
//! chain this runs against is a regtest chain a few hundred blocks long that
//! is thrown away afterwards, so the whole cache fits in memory and a file
//! format would only be a second thing to get wrong.

use std::collections::BTreeMap;
use std::sync::Mutex;

use zcash_client_backend::data_api::chain::{error::Error, BlockCache, BlockSource};
use zcash_client_backend::data_api::scanning::ScanRange;
use zcash_client_backend::proto::compact_formats::CompactBlock;
use zcash_protocol::consensus::BlockHeight;

/// Nothing in an in-memory cache can fail, but [`BlockSource`] requires an
/// error type and `Infallible` cannot carry the one thing that can go wrong
/// for a caller: asking for a block the cache never received.
#[derive(Debug)]
pub struct CacheError(pub String);

impl std::fmt::Display for CacheError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for CacheError {}

#[derive(Default)]
pub struct MemoryCache(Mutex<BTreeMap<u32, CompactBlock>>);

impl BlockSource for MemoryCache {
    type Error = CacheError;

    fn with_blocks<F, WalletErrT>(
        &self,
        from_height: Option<BlockHeight>,
        limit: Option<usize>,
        mut with_block: F,
    ) -> Result<(), Error<WalletErrT, Self::Error>>
    where
        F: FnMut(CompactBlock) -> Result<(), Error<WalletErrT, Self::Error>>,
    {
        let blocks = self.0.lock().expect("the cache lock is never poisoned");
        let start = from_height.map(u32::from).unwrap_or(0);
        // Contiguity is the caller's requirement: a gap means the scanner
        // would silently skip a block and report the range scanned.
        let mut expected: Option<u32> = None;
        for (seen, (height, block)) in blocks.range(start..).enumerate() {
            if let Some(want) = expected {
                if *height != want {
                    break;
                }
            }
            if limit.is_some_and(|n| seen >= n) {
                break;
            }
            with_block(block.clone())?;
            expected = Some(height + 1);
        }
        Ok(())
    }
}

#[async_trait::async_trait]
impl BlockCache for MemoryCache {
    fn get_tip_height(&self, range: Option<&ScanRange>) -> Result<Option<BlockHeight>, CacheError> {
        let blocks = self.0.lock().expect("the cache lock is never poisoned");
        let tip = match range {
            None => blocks.keys().next_back().copied(),
            Some(range) => {
                let start = u32::from(range.block_range().start);
                let end = u32::from(range.block_range().end);
                blocks.range(start..end).next_back().map(|(h, _)| *h)
            }
        };
        Ok(tip.map(BlockHeight::from_u32))
    }

    async fn read(&self, range: &ScanRange) -> Result<Vec<CompactBlock>, CacheError> {
        let blocks = self.0.lock().expect("the cache lock is never poisoned");
        let start = u32::from(range.block_range().start);
        let end = u32::from(range.block_range().end);
        let mut out = Vec::new();
        for (expected, (height, block)) in (start..).zip(blocks.range(start..end)) {
            if *height != expected {
                break;
            }
            out.push(block.clone());
        }
        Ok(out)
    }

    async fn insert(&self, compact_blocks: Vec<CompactBlock>) -> Result<(), CacheError> {
        let mut blocks = self.0.lock().expect("the cache lock is never poisoned");
        let _n = compact_blocks.len();
        for block in compact_blocks {
            blocks.insert(block.height as u32, block);
        }
        Ok(())
    }

    async fn delete(&self, range: ScanRange) -> Result<(), CacheError> {
        let mut blocks = self.0.lock().expect("the cache lock is never poisoned");
        let start = u32::from(range.block_range().start);
        let end = u32::from(range.block_range().end);
        let doomed: Vec<u32> = blocks.range(start..end).map(|(h, _)| *h).collect();
        for height in doomed {
            blocks.remove(&height);
        }
        Ok(())
    }
}
