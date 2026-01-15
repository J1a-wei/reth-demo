//! MDBX database adapter for revm
//!
//! Implements revm's Database trait to connect to MDBX storage.

use alloy_primitives::{Address, B256, U256};
use dex_storage::{AccountState, StateStore};
use revm::{
    bytecode::Bytecode,
    database_interface::DBErrorMarker,
    primitives::KECCAK_EMPTY,
    state::{AccountInfo, EvmState},
    Database, DatabaseCommit, DatabaseRef,
};
use std::{collections::HashMap, fmt, sync::Arc};

/// Database error type that implements DBErrorMarker
#[derive(Debug)]
pub struct DbError(pub String);

impl fmt::Display for DbError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)
    }
}

impl std::error::Error for DbError {}

impl DBErrorMarker for DbError {}

impl From<eyre::Report> for DbError {
    fn from(e: eyre::Report) -> Self {
        DbError(e.to_string())
    }
}

impl From<String> for DbError {
    fn from(s: String) -> Self {
        DbError(s)
    }
}

impl From<&str> for DbError {
    fn from(s: &str) -> Self {
        DbError(s.to_string())
    }
}

/// MDBX-backed database for revm execution
pub struct MdbxDatabase {
    /// State store reference
    state_store: Arc<StateStore>,
    /// In-memory code storage (will be persisted to DualvmCode table)
    code_by_hash: HashMap<B256, Bytecode>,
    /// Block hashes cache
    block_hashes: HashMap<u64, B256>,
}

impl MdbxDatabase {
    /// Create new MDBX database wrapper
    pub fn new(state_store: Arc<StateStore>) -> Self {
        Self {
            state_store,
            code_by_hash: HashMap::new(),
            block_hashes: HashMap::new(),
        }
    }

    /// Set block hash for a given number
    pub fn set_block_hash(&mut self, number: u64, hash: B256) {
        self.block_hashes.insert(number, hash);
    }

    /// Insert contract code
    pub fn insert_code(&mut self, code_hash: B256, code: Bytecode) {
        self.code_by_hash.insert(code_hash, code);
    }

    /// Get state store reference
    pub fn state_store(&self) -> &Arc<StateStore> {
        &self.state_store
    }
}

impl DatabaseRef for MdbxDatabase {
    type Error = DbError;

    fn basic_ref(&self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        match self.state_store.get_account(&address) {
            Some(account) => {
                let code_hash = if account.code_hash == B256::ZERO {
                    KECCAK_EMPTY
                } else {
                    account.code_hash
                };

                let code = self.code_by_hash.get(&code_hash).cloned();

                Ok(Some(AccountInfo {
                    balance: account.balance,
                    nonce: account.nonce,
                    code_hash,
                    code,
                }))
            }
            None => Ok(None),
        }
    }

    fn code_by_hash_ref(&self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        if code_hash == KECCAK_EMPTY || code_hash == B256::ZERO {
            return Ok(Bytecode::new());
        }

        self.code_by_hash
            .get(&code_hash)
            .cloned()
            .ok_or_else(|| DbError(format!("Code not found for hash: {}", code_hash)))
    }

    fn storage_ref(&self, address: Address, index: U256) -> Result<U256, Self::Error> {
        Ok(self.state_store.get_storage(&address, index))
    }

    fn block_hash_ref(&self, number: u64) -> Result<B256, Self::Error> {
        self.block_hashes
            .get(&number)
            .copied()
            .ok_or_else(|| DbError(format!("Block hash not found for number: {}", number)))
    }
}

impl Database for MdbxDatabase {
    type Error = DbError;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        self.basic_ref(address)
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        self.code_by_hash_ref(code_hash)
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        self.storage_ref(address, index)
    }

    fn block_hash(&mut self, number: u64) -> Result<B256, Self::Error> {
        self.block_hash_ref(number)
    }
}

/// State changes to be committed
#[derive(Debug, Default)]
pub struct StateChanges {
    /// Account changes
    pub accounts: HashMap<Address, AccountState>,
    /// Storage changes
    pub storage: HashMap<Address, HashMap<U256, U256>>,
    /// New contract codes
    pub codes: HashMap<B256, Vec<u8>>,
}

impl DatabaseCommit for MdbxDatabase {
    fn commit(&mut self, changes: EvmState) {
        for (address, account) in changes {
            // Skip if not touched
            if !account.is_touched() {
                continue;
            }

            // Get code before moving
            let code = account.info.code.clone();

            // Update account info
            let mut state = AccountState {
                balance: account.info.balance,
                nonce: account.info.nonce,
                code_hash: account.info.code_hash,
                code: code.as_ref().map(|c| c.original_bytes().clone().into()),
                storage: HashMap::new(),
            };

            // Collect storage changes
            for (slot, value) in account.storage {
                if value.is_changed() {
                    state.storage.insert(slot, value.present_value);
                }
            }

            // Persist to database
            if let Err(e) = self.state_store.set_account(address, state) {
                tracing::error!("Failed to commit account {}: {}", address, e);
            }

            // Store code if present
            if let Some(code) = code {
                if account.info.code_hash != KECCAK_EMPTY && account.info.code_hash != B256::ZERO {
                    self.code_by_hash.insert(account.info.code_hash, code);
                }
            }
        }
    }
}

/// Cache-based database wrapper for efficient reads
pub struct CachedDatabase {
    /// Inner database
    inner: MdbxDatabase,
    /// Account cache
    account_cache: HashMap<Address, Option<AccountInfo>>,
    /// Storage cache
    storage_cache: HashMap<(Address, U256), U256>,
}

impl CachedDatabase {
    /// Create new cached database
    pub fn new(inner: MdbxDatabase) -> Self {
        Self {
            inner,
            account_cache: HashMap::new(),
            storage_cache: HashMap::new(),
        }
    }

    /// Get inner database reference
    pub fn inner(&self) -> &MdbxDatabase {
        &self.inner
    }

    /// Get mutable inner database reference
    pub fn inner_mut(&mut self) -> &mut MdbxDatabase {
        &mut self.inner
    }

    /// Clear all caches
    pub fn clear_cache(&mut self) {
        self.account_cache.clear();
        self.storage_cache.clear();
    }
}

impl Database for CachedDatabase {
    type Error = DbError;

    fn basic(&mut self, address: Address) -> Result<Option<AccountInfo>, Self::Error> {
        if let Some(cached) = self.account_cache.get(&address) {
            return Ok(cached.clone());
        }

        let result = self.inner.basic(address)?;
        self.account_cache.insert(address, result.clone());
        Ok(result)
    }

    fn code_by_hash(&mut self, code_hash: B256) -> Result<Bytecode, Self::Error> {
        self.inner.code_by_hash(code_hash)
    }

    fn storage(&mut self, address: Address, index: U256) -> Result<U256, Self::Error> {
        let key = (address, index);
        if let Some(&cached) = self.storage_cache.get(&key) {
            return Ok(cached);
        }

        let result = self.inner.storage(address, index)?;
        self.storage_cache.insert(key, result);
        Ok(result)
    }

    fn block_hash(&mut self, number: u64) -> Result<B256, Self::Error> {
        self.inner.block_hash(number)
    }
}

impl DatabaseCommit for CachedDatabase {
    fn commit(&mut self, changes: EvmState) {
        // Clear affected cache entries
        for address in changes.keys() {
            self.account_cache.remove(address);
            // Remove storage entries for this address
            self.storage_cache.retain(|(addr, _), _| addr != address);
        }

        // Delegate to inner database
        self.inner.commit(changes);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dex_storage::DualvmStorage;
    use std::sync::Arc;
    use tempfile::tempdir;

    fn create_test_db() -> (tempfile::TempDir, Arc<StateStore>) {
        let dir = tempdir().unwrap();
        let storage = DualvmStorage::new(dir.path()).unwrap();
        // We need to keep storage alive, so return it wrapped in Arc
        let state_store = StateStore::new(storage.db.clone());
        (dir, Arc::new(state_store))
    }

    #[test]
    fn test_basic_account_lookup() {
        let (_dir, state_store) = create_test_db();
        let db = MdbxDatabase::new(state_store);

        let address = Address::ZERO;
        let result = db.basic_ref(address).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_storage_lookup() {
        let (_dir, state_store) = create_test_db();
        let db = MdbxDatabase::new(state_store);

        let address = Address::ZERO;
        let slot = U256::from(1);
        let result = db.storage_ref(address, slot).unwrap();
        assert_eq!(result, U256::ZERO);
    }
}
