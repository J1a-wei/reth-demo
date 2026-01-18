//! State storage module using MDBX database

use crate::tables::{
    DualvmAccounts, DualvmCounters, DualvmStorage, StorageKey, StoredCounter, StoredDualvmAccount,
    StoredStorageValue,
};
use alloy_primitives::{keccak256, Address, Bytes, B256, U256};
use eyre::Result;
use reth_db::DatabaseEnv;
use reth_db_api::{
    cursor::{DbCursorRO, DbCursorRW},
    database::Database,
    transaction::{DbTx, DbTxMut},
};
use std::{collections::HashMap, sync::Arc};

/// Account state representation
#[derive(Debug, Clone, Default)]
pub struct AccountState {
    pub balance: U256,
    pub nonce: u64,
    pub code_hash: B256,
    pub code: Option<Bytes>,
    pub storage: HashMap<U256, U256>,
}

impl AccountState {
    /// Create new EOA account
    pub fn new_eoa(balance: U256) -> Self {
        Self { balance, nonce: 0, code_hash: B256::ZERO, code: None, storage: HashMap::new() }
    }

    /// Create new contract account
    pub fn new_contract(balance: U256, code: Bytes) -> Self {
        let code_hash = keccak256(&code);
        Self { balance, nonce: 1, code_hash, code: Some(code), storage: HashMap::new() }
    }
}

impl From<StoredDualvmAccount> for AccountState {
    fn from(stored: StoredDualvmAccount) -> Self {
        Self {
            balance: stored.balance,
            nonce: stored.nonce,
            code_hash: stored.code_hash,
            code: None,
            storage: HashMap::new(),
        }
    }
}

impl From<&AccountState> for StoredDualvmAccount {
    fn from(state: &AccountState) -> Self {
        Self {
            balance: state.balance,
            nonce: state.nonce,
            code_hash: state.code_hash,
            is_contract: state.code.is_some(),
        }
    }
}

/// State store using MDBX database
pub struct StateStore {
    db: Arc<DatabaseEnv>,
}

impl StateStore {
    /// Create new state store with database
    pub fn new(db: Arc<DatabaseEnv>) -> Self {
        Self { db }
    }

    /// Get account state
    pub fn get_account(&self, address: &Address) -> Option<AccountState> {
        let tx = self.db.tx().ok()?;
        let stored = tx.get::<DualvmAccounts>(*address).ok()??;

        let mut account: AccountState = stored.into();

        // Load storage for this account
        let mut cursor = tx.cursor_read::<DualvmStorage>().ok()?;
        let start_key = StorageKey { address: *address, slot: U256::ZERO };
        let walker = cursor.walk(Some(start_key)).ok()?;

        for result in walker {
            if let Ok((key, value)) = result {
                if key.address != *address {
                    break;
                }
                account.storage.insert(key.slot, value.value);
            } else {
                break;
            }
        }

        Some(account)
    }

    /// Set account state
    pub fn set_account(&self, address: Address, state: AccountState) -> Result<()> {
        let tx = self.db.tx_mut()?;

        let stored: StoredDualvmAccount = (&state).into();
        tx.put::<DualvmAccounts>(address, stored)?;

        for (slot, value) in &state.storage {
            let key = StorageKey { address, slot: *slot };
            if *value == U256::ZERO {
                let mut cursor = tx.cursor_write::<DualvmStorage>()?;
                if cursor.seek_exact(key.clone())?.is_some() {
                    cursor.delete_current()?;
                }
            } else {
                tx.put::<DualvmStorage>(key, StoredStorageValue { value: *value })?;
            }
        }

        tx.commit()?;
        Ok(())
    }

    /// Get account balance
    pub fn get_balance(&self, address: &Address) -> U256 {
        self.db
            .tx()
            .ok()
            .and_then(|tx| tx.get::<DualvmAccounts>(*address).ok())
            .flatten()
            .map(|a| a.balance)
            .unwrap_or(U256::ZERO)
    }

    /// Set account balance
    pub fn set_balance(&self, address: Address, balance: U256) -> Result<()> {
        let tx = self.db.tx_mut()?;

        let mut account =
            tx.get::<DualvmAccounts>(address)?.unwrap_or_else(StoredDualvmAccount::default);

        account.balance = balance;
        tx.put::<DualvmAccounts>(address, account)?;
        tx.commit()?;
        Ok(())
    }

    /// Get account nonce
    pub fn get_nonce(&self, address: &Address) -> u64 {
        self.db
            .tx()
            .ok()
            .and_then(|tx| tx.get::<DualvmAccounts>(*address).ok())
            .flatten()
            .map(|a| a.nonce)
            .unwrap_or(0)
    }

    /// Set account nonce
    pub fn set_nonce(&self, address: Address, nonce: u64) -> Result<()> {
        let tx = self.db.tx_mut()?;

        let mut account =
            tx.get::<DualvmAccounts>(address)?.unwrap_or_else(StoredDualvmAccount::default);

        account.nonce = nonce;
        tx.put::<DualvmAccounts>(address, account)?;
        tx.commit()?;
        Ok(())
    }

    /// Increment nonce and return new value
    pub fn increment_nonce(&self, address: Address) -> Result<u64> {
        let tx = self.db.tx_mut()?;

        let mut account =
            tx.get::<DualvmAccounts>(address)?.unwrap_or_else(StoredDualvmAccount::default);

        account.nonce += 1;
        let new_nonce = account.nonce;
        tx.put::<DualvmAccounts>(address, account)?;
        tx.commit()?;
        Ok(new_nonce)
    }

    /// Get contract code
    pub fn get_code(&self, _address: &Address) -> Option<Bytes> {
        None // Simplified: code storage not implemented
    }

    /// Set contract code
    pub fn set_code(&self, address: Address, code: Bytes) -> Result<()> {
        let tx = self.db.tx_mut()?;

        let code_hash = keccak256(&code);
        let mut account =
            tx.get::<DualvmAccounts>(address)?.unwrap_or_else(StoredDualvmAccount::default);

        account.code_hash = code_hash;
        account.is_contract = true;
        tx.put::<DualvmAccounts>(address, account)?;
        tx.commit()?;
        Ok(())
    }

    /// Get storage value
    pub fn get_storage(&self, address: &Address, slot: U256) -> U256 {
        let key = StorageKey { address: *address, slot };
        self.db
            .tx()
            .ok()
            .and_then(|tx| tx.get::<DualvmStorage>(key).ok())
            .flatten()
            .map(|v| v.value)
            .unwrap_or(U256::ZERO)
    }

    /// Set storage value
    pub fn set_storage(&self, address: Address, slot: U256, value: U256) -> Result<()> {
        let tx = self.db.tx_mut()?;
        let key = StorageKey { address, slot };

        if value == U256::ZERO {
            let mut cursor = tx.cursor_write::<DualvmStorage>()?;
            if cursor.seek_exact(key)?.is_some() {
                cursor.delete_current()?;
            }
        } else {
            tx.put::<DualvmStorage>(key, StoredStorageValue { value })?;
        }

        tx.commit()?;
        Ok(())
    }

    /// Get counter value (for DexVM)
    pub fn get_counter(&self, address: &Address) -> u64 {
        self.db
            .tx()
            .ok()
            .and_then(|tx| tx.get::<DualvmCounters>(*address).ok())
            .flatten()
            .map(|c| c.value)
            .unwrap_or(0)
    }

    /// Set counter value (for DexVM)
    pub fn set_counter(&self, address: Address, value: u64) -> Result<()> {
        let tx = self.db.tx_mut()?;
        tx.put::<DualvmCounters>(address, StoredCounter { value })?;
        tx.commit()?;
        Ok(())
    }

    /// Increment counter and return new value
    pub fn increment_counter(&self, address: Address, amount: u64) -> Result<u64> {
        let tx = self.db.tx_mut()?;

        let current = tx.get::<DualvmCounters>(address)?.map(|c| c.value).unwrap_or(0);

        let new_value = current.saturating_add(amount);
        tx.put::<DualvmCounters>(address, StoredCounter { value: new_value })?;
        tx.commit()?;
        Ok(new_value)
    }

    /// Decrement counter and return new value
    pub fn decrement_counter(&self, address: Address, amount: u64) -> Result<u64> {
        let tx = self.db.tx_mut()?;

        let current = tx.get::<DualvmCounters>(address)?.map(|c| c.value).unwrap_or(0);

        if amount > current {
            return Err(eyre::eyre!("Counter underflow"));
        }

        let new_value = current - amount;
        tx.put::<DualvmCounters>(address, StoredCounter { value: new_value })?;
        tx.commit()?;
        Ok(new_value)
    }

    /// Initialize from genesis allocation
    pub fn init_genesis(&self, alloc: HashMap<Address, U256>) -> Result<()> {
        let tx = self.db.tx_mut()?;

        for (address, balance) in alloc {
            let account = StoredDualvmAccount {
                balance,
                nonce: 0,
                code_hash: B256::ZERO,
                is_contract: false,
            };
            tx.put::<DualvmAccounts>(address, account)?;
        }

        tx.commit()?;
        Ok(())
    }

    /// Calculate state root
    pub fn state_root(&self) -> B256 {
        let tx = match self.db.tx() {
            Ok(tx) => tx,
            Err(_) => return B256::ZERO,
        };

        let mut cursor = match tx.cursor_read::<DualvmAccounts>() {
            Ok(cursor) => cursor,
            Err(_) => return B256::ZERO,
        };

        let mut data = Vec::new();
        let walker = match cursor.walk(None) {
            Ok(walker) => walker,
            Err(_) => return B256::ZERO,
        };

        for result in walker {
            if let Ok((addr, account)) = result {
                data.extend_from_slice(addr.as_slice());
                data.extend_from_slice(&account.balance.to_be_bytes::<32>());
                data.extend_from_slice(&account.nonce.to_be_bytes());
                data.extend_from_slice(account.code_hash.as_slice());
            }
        }

        if data.is_empty() {
            B256::ZERO
        } else {
            keccak256(&data)
        }
    }

    /// Get all accounts
    pub fn all_accounts(&self) -> HashMap<Address, AccountState> {
        let mut result = HashMap::new();

        let tx = match self.db.tx() {
            Ok(tx) => tx,
            Err(_) => return result,
        };

        let mut cursor = match tx.cursor_read::<DualvmAccounts>() {
            Ok(cursor) => cursor,
            Err(_) => return result,
        };

        let walker = match cursor.walk(None) {
            Ok(walker) => walker,
            Err(_) => return result,
        };

        for entry in walker {
            if let Ok((addr, stored)) = entry {
                result.insert(addr, stored.into());
            }
        }

        result
    }

    /// Get all counters (for DexVM state recovery)
    pub fn all_counters(&self) -> HashMap<Address, u64> {
        let mut result = HashMap::new();

        let tx = match self.db.tx() {
            Ok(tx) => tx,
            Err(_) => return result,
        };

        let mut cursor = match tx.cursor_read::<DualvmCounters>() {
            Ok(cursor) => cursor,
            Err(_) => return result,
        };

        let walker = match cursor.walk(None) {
            Ok(walker) => walker,
            Err(_) => return result,
        };

        for entry in walker {
            if let Ok((addr, stored)) = entry {
                result.insert(addr, stored.value);
            }
        }

        result
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
    fn test_balance() {
        let db = create_test_db();
        let store = StateStore::new(db);

        let addr = address!("1111111111111111111111111111111111111111");
        assert_eq!(store.get_balance(&addr), U256::ZERO);

        store.set_balance(addr, U256::from(1000)).unwrap();
        assert_eq!(store.get_balance(&addr), U256::from(1000));
    }

    #[test]
    fn test_counter() {
        let db = create_test_db();
        let store = StateStore::new(db);

        let addr = address!("2222222222222222222222222222222222222222");
        assert_eq!(store.get_counter(&addr), 0);

        let new_val = store.increment_counter(addr, 10).unwrap();
        assert_eq!(new_val, 10);
        assert_eq!(store.get_counter(&addr), 10);

        let new_val = store.decrement_counter(addr, 3).unwrap();
        assert_eq!(new_val, 7);
        assert_eq!(store.get_counter(&addr), 7);
    }

    #[test]
    fn test_genesis() {
        let db = create_test_db();
        let store = StateStore::new(db);

        let mut alloc = HashMap::new();
        let addr1 = address!("3333333333333333333333333333333333333333");
        let addr2 = address!("4444444444444444444444444444444444444444");
        alloc.insert(addr1, U256::from(1000));
        alloc.insert(addr2, U256::from(2000));

        store.init_genesis(alloc).unwrap();

        assert_eq!(store.get_balance(&addr1), U256::from(1000));
        assert_eq!(store.get_balance(&addr2), U256::from(2000));
    }
}
