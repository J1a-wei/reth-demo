//! Combined storage wrapper

use crate::{block_store::BlockStore, state_store::StateStore, tables::DualvmTableSet};
use eyre::Result;
use reth_db::{mdbx::DatabaseArguments, mdbx::init_db_for, models::ClientVersion, DatabaseEnv};
use std::{
    path::Path,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};

/// Combined DualVM storage
pub struct DualvmStorage {
    /// Database environment
    pub db: Arc<DatabaseEnv>,
    /// Block store
    pub blocks: BlockStore,
    /// State store
    pub state: StateStore,
    /// Whether this is a new database
    is_new: AtomicBool,
}

impl DualvmStorage {
    /// Create new storage from path
    pub fn new(path: &Path) -> Result<Self> {
        // Check if database already exists
        let db_path = path.join("mdbx.dat");
        let is_new = !db_path.exists();

        // Ensure directory exists
        std::fs::create_dir_all(path)?;

        // Initialize MDBX database
        let db = init_db_for::<_, DualvmTableSet>(
            path,
            DatabaseArguments::new(ClientVersion::default()),
        )?;
        let db = Arc::new(db);

        let blocks = BlockStore::new(Arc::clone(&db))?;
        let state = StateStore::new(Arc::clone(&db));

        Ok(Self { db, blocks, state, is_new: AtomicBool::new(is_new) })
    }

    /// Check if this is a new database
    pub fn is_new_database(&self) -> bool {
        self.is_new.load(Ordering::SeqCst)
    }

    /// Mark database as initialized
    pub fn mark_initialized(&self) {
        self.is_new.store(false, Ordering::SeqCst);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_storage_creation() {
        let dir = tempdir().unwrap();
        let storage = DualvmStorage::new(dir.path()).unwrap();

        assert!(storage.is_new_database());

        // Second open should not be new
        drop(storage);
        let storage2 = DualvmStorage::new(dir.path()).unwrap();
        assert!(!storage2.is_new_database());
    }
}
