//! Block storage module using MDBX database

use crate::tables::{DualvmBlocks, DualvmTransactions, DualvmTxHashes, StoredDualvmBlock, StoredTransaction, StoredTxInfo};
use alloy_primitives::{keccak256, Address, B256};
use eyre::Result;
use reth_db::DatabaseEnv;
use reth_db_api::{
    cursor::DbCursorRO,
    database::Database,
    transaction::{DbTx, DbTxMut},
};
use std::sync::{
    atomic::{AtomicU64, Ordering},
    Arc,
};

/// Stored block data with transaction hashes
#[derive(Debug, Clone)]
pub struct StoredBlock {
    pub number: u64,
    pub hash: B256,
    pub parent_hash: B256,
    pub timestamp: u64,
    pub gas_limit: u64,
    pub gas_used: u64,
    pub miner: Address,
    pub evm_state_root: B256,
    pub dexvm_state_root: B256,
    pub combined_state_root: B256,
    pub transaction_hashes: Vec<B256>,
    pub transaction_count: u64,
    /// Block signature (65 bytes: r[32] + s[32] + v[1])
    pub signature: [u8; 65],
}

impl StoredBlock {
    /// Create genesis block
    pub fn genesis(chain_id: u64) -> Self {
        let hash = keccak256(format!("genesis-{}", chain_id).as_bytes());
        Self {
            number: 0,
            hash,
            parent_hash: B256::ZERO,
            timestamp: 0,
            gas_limit: 30_000_000,
            gas_used: 0,
            miner: Address::ZERO,
            evm_state_root: B256::ZERO,
            dexvm_state_root: B256::ZERO,
            combined_state_root: B256::ZERO,
            transaction_hashes: vec![],
            transaction_count: 0,
            signature: [0u8; 65],
        }
    }
}

impl From<StoredDualvmBlock> for StoredBlock {
    fn from(stored: StoredDualvmBlock) -> Self {
        Self {
            number: 0,
            hash: stored.hash,
            parent_hash: stored.parent_hash,
            timestamp: stored.timestamp,
            gas_limit: stored.gas_limit,
            gas_used: stored.gas_used,
            miner: stored.miner,
            evm_state_root: stored.evm_state_root,
            dexvm_state_root: stored.dexvm_state_root,
            combined_state_root: stored.combined_state_root,
            transaction_hashes: stored.transaction_hashes,
            transaction_count: stored.transaction_count,
            signature: stored.signature,
        }
    }
}

impl From<&StoredBlock> for StoredDualvmBlock {
    fn from(block: &StoredBlock) -> Self {
        Self {
            hash: block.hash,
            parent_hash: block.parent_hash,
            timestamp: block.timestamp,
            gas_limit: block.gas_limit,
            gas_used: block.gas_used,
            miner: block.miner,
            evm_state_root: block.evm_state_root,
            dexvm_state_root: block.dexvm_state_root,
            combined_state_root: block.combined_state_root,
            transaction_count: block.transaction_count,
            signature: block.signature,
            transaction_hashes: block.transaction_hashes.clone(),
        }
    }
}

/// Block store using MDBX database
pub struct BlockStore {
    db: Arc<DatabaseEnv>,
    latest_block: AtomicU64,
}

impl BlockStore {
    /// Create new block store with database
    pub fn new(db: Arc<DatabaseEnv>) -> Result<Self> {
        let store = Self { db, latest_block: AtomicU64::new(0) };
        store.load_latest_block_number()?;
        Ok(store)
    }

    fn load_latest_block_number(&self) -> Result<()> {
        let tx = self.db.tx()?;
        let mut cursor = tx.cursor_read::<DualvmBlocks>()?;

        if let Some((block_number, _)) = cursor.last()? {
            self.latest_block.store(block_number, Ordering::SeqCst);
            tracing::info!("Loaded latest block number: {}", block_number);
        }

        Ok(())
    }

    /// Store a block
    pub fn store_block(&self, block: StoredBlock) -> Result<()> {
        let tx = self.db.tx_mut()?;

        let stored: StoredDualvmBlock = (&block).into();
        tx.put::<DualvmBlocks>(block.number, stored)?;

        for (idx, tx_hash) in block.transaction_hashes.iter().enumerate() {
            tx.put::<DualvmTxHashes>(
                *tx_hash,
                StoredTxInfo { block_number: block.number, tx_index: idx as u64 },
            )?;
        }

        tx.commit()?;

        let current_latest = self.latest_block.load(Ordering::SeqCst);
        if block.number > current_latest {
            self.latest_block.store(block.number, Ordering::SeqCst);
        }

        tracing::debug!("Stored block {} with hash {:?}", block.number, block.hash);
        Ok(())
    }

    /// Get block by number
    pub fn get_block_by_number(&self, number: u64) -> Option<StoredBlock> {
        let tx = self.db.tx().ok()?;
        let stored = tx.get::<DualvmBlocks>(number).ok()??;

        let mut block: StoredBlock = stored.into();
        block.number = number;

        Some(block)
    }

    /// Get block by hash
    pub fn get_block_by_hash(&self, hash: B256) -> Option<StoredBlock> {
        let tx = self.db.tx().ok()?;
        let mut cursor = tx.cursor_read::<DualvmBlocks>().ok()?;
        let walker = cursor.walk(None).ok()?;

        for (number, stored) in walker.flatten() {
            if stored.hash == hash {
                let mut block: StoredBlock = stored.into();
                block.number = number;
                return Some(block);
            }
        }

        None
    }

    /// Get latest block
    pub fn get_latest_block(&self) -> Option<StoredBlock> {
        let latest = self.latest_block.load(Ordering::SeqCst);
        self.get_block_by_number(latest)
    }

    /// Get latest block number
    pub fn latest_block_number(&self) -> u64 {
        self.latest_block.load(Ordering::SeqCst)
    }

    /// Get block count
    pub fn block_count(&self) -> usize {
        let tx = match self.db.tx() {
            Ok(tx) => tx,
            Err(_) => return 0,
        };

        let mut cursor = match tx.cursor_read::<DualvmBlocks>() {
            Ok(cursor) => cursor,
            Err(_) => return 0,
        };

        match cursor.walk(None) {
            Ok(walker) => walker.count(),
            Err(_) => 0,
        }
    }

    /// Get transaction info by hash
    pub fn get_tx_info(&self, tx_hash: B256) -> Option<StoredTxInfo> {
        let tx = self.db.tx().ok()?;
        tx.get::<DualvmTxHashes>(tx_hash).ok()?
    }

    /// Get block number containing a transaction
    pub fn get_tx_block_number(&self, tx_hash: B256) -> Option<u64> {
        self.get_tx_info(tx_hash).map(|info| info.block_number)
    }

    /// Check if genesis block exists
    pub fn has_genesis(&self) -> bool {
        self.get_block_by_number(0).is_some()
    }

    /// Initialize with genesis block
    pub fn init_genesis(&self, chain_id: u64) -> Result<()> {
        if self.has_genesis() {
            tracing::info!("Genesis block already exists");
            return Ok(());
        }

        let genesis = StoredBlock::genesis(chain_id);
        self.store_block(genesis)?;
        tracing::info!("Initialized genesis block for chain {}", chain_id);
        Ok(())
    }

    /// Store a full transaction by its hash
    pub fn store_transaction(&self, tx_hash: B256, rlp_bytes: Vec<u8>) -> Result<()> {
        let tx = self.db.tx_mut()?;
        tx.put::<DualvmTransactions>(tx_hash, StoredTransaction { rlp_bytes })?;
        tx.commit()?;
        tracing::debug!("Stored transaction {:?}", tx_hash);
        Ok(())
    }

    /// Store multiple transactions in a single batch
    pub fn store_transactions(&self, transactions: &[(B256, Vec<u8>)]) -> Result<()> {
        if transactions.is_empty() {
            return Ok(());
        }
        let tx = self.db.tx_mut()?;
        for (tx_hash, rlp_bytes) in transactions {
            tx.put::<DualvmTransactions>(*tx_hash, StoredTransaction { rlp_bytes: rlp_bytes.clone() })?;
        }
        tx.commit()?;
        tracing::debug!("Stored {} transactions", transactions.len());
        Ok(())
    }

    /// Get a transaction by its hash
    pub fn get_transaction(&self, tx_hash: B256) -> Option<Vec<u8>> {
        let tx = self.db.tx().ok()?;
        tx.get::<DualvmTransactions>(tx_hash).ok()?.map(|t| t.rlp_bytes)
    }

    /// Get all transactions for a block by block number
    pub fn get_block_transactions(&self, block_number: u64) -> Option<Vec<Vec<u8>>> {
        let block = self.get_block_by_number(block_number)?;
        let mut txs = Vec::with_capacity(block.transaction_hashes.len());

        let tx = self.db.tx().ok()?;
        for tx_hash in &block.transaction_hashes {
            if let Ok(Some(stored_tx)) = tx.get::<DualvmTransactions>(*tx_hash) {
                txs.push(stored_tx.rlp_bytes);
            }
        }

        Some(txs)
    }

    /// Get transactions by their hashes
    pub fn get_transactions_by_hashes(&self, hashes: &[B256]) -> Vec<Option<Vec<u8>>> {
        let tx = match self.db.tx() {
            Ok(tx) => tx,
            Err(_) => return hashes.iter().map(|_| None).collect(),
        };

        hashes.iter().map(|hash| {
            tx.get::<DualvmTransactions>(*hash).ok().flatten().map(|t| t.rlp_bytes)
        }).collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::address;
    use reth_db::{mdbx::DatabaseArguments, mdbx::init_db_for, models::ClientVersion};
    use tempfile::tempdir;

    fn create_test_db() -> Arc<DatabaseEnv> {
        let dir = tempdir().unwrap();
        let db = init_db_for::<_, crate::tables::DualvmTableSet>(
            dir.path(),
            DatabaseArguments::new(ClientVersion::default()),
        )
        .unwrap();
        Arc::new(db)
    }

    #[test]
    fn test_block_store() {
        let db = create_test_db();
        let store = BlockStore::new(db).unwrap();

        let block = StoredBlock {
            number: 1,
            hash: B256::repeat_byte(0x11),
            parent_hash: B256::ZERO,
            timestamp: 1000,
            gas_limit: 30_000_000,
            gas_used: 21000,
            miner: address!("1111111111111111111111111111111111111111"),
            evm_state_root: B256::repeat_byte(0x22),
            dexvm_state_root: B256::repeat_byte(0x33),
            combined_state_root: B256::repeat_byte(0x44),
            transaction_hashes: vec![],
            transaction_count: 0,
            signature: [0u8; 65],
        };

        store.store_block(block.clone()).unwrap();

        let retrieved = store.get_block_by_number(1).unwrap();
        assert_eq!(retrieved.number, 1);
        assert_eq!(retrieved.hash, block.hash);
    }

    #[test]
    fn test_genesis() {
        let db = create_test_db();
        let store = BlockStore::new(db).unwrap();

        assert!(!store.has_genesis());

        store.init_genesis(13337).unwrap();

        assert!(store.has_genesis());
        let genesis = store.get_block_by_number(0).unwrap();
        assert_eq!(genesis.number, 0);
    }
}
