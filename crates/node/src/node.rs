//! DualVM node

use crate::{
    consensus::{PoaConfig, PoaConsensus},
    evm_executor::SimpleEvmExecutor,
    executor::DualVmExecutor,
};
use alloy_primitives::{keccak256, Address, B256, U256};
use dex_dexvm::{DexVmExecutor as DexExecutor, DexVmState};
use dex_rpc::{start_evm_rpc_server, DexVmApi, EvmRpcServer};
use dex_storage::{BlockStore, DualvmStorage, StateStore, StoredBlock};
use jsonrpsee::server::ServerHandle;
use std::{
    collections::HashMap,
    path::PathBuf,
    sync::{Arc, RwLock},
};
use tokio::task::JoinHandle;

/// Node configuration
#[derive(Debug, Clone)]
pub struct NodeConfig {
    /// Chain ID
    pub chain_id: u64,
    /// Data directory
    pub datadir: PathBuf,
    /// EVM RPC port
    pub evm_rpc_port: u16,
    /// DexVM RPC port
    pub dexvm_rpc_port: u16,
}

impl Default for NodeConfig {
    fn default() -> Self {
        Self {
            chain_id: 1,
            datadir: PathBuf::from("./data"),
            evm_rpc_port: 8545,
            dexvm_rpc_port: 9845,
        }
    }
}

/// Dual VM node
pub struct DualVmNode {
    config: NodeConfig,
    executor: DualVmExecutor,
    dexvm_executor: Arc<RwLock<DexExecutor>>,
    consensus: Option<PoaConsensus>,
    storage: Arc<DualvmStorage>,
    evm_rpc_server: Option<Arc<EvmRpcServer>>,
}

impl DualVmNode {
    /// Create a new dual VM node
    pub fn new(chain_id: u64) -> Self {
        Self::with_config(NodeConfig { chain_id, ..Default::default() })
    }

    /// Create node with configuration
    pub fn with_config(config: NodeConfig) -> Self {
        let storage = Arc::new(
            DualvmStorage::new(&config.datadir).expect("Failed to initialize MDBX database"),
        );

        // Create EVM executor backed by the shared StateStore
        let evm_executor = Arc::new(RwLock::new(SimpleEvmExecutor::new(
            config.chain_id,
            Arc::clone(&storage.state),
        )));
        let dexvm_executor = Arc::new(RwLock::new(DexExecutor::new(DexVmState::default())));
        let executor = DualVmExecutor::new(evm_executor, Arc::clone(&dexvm_executor));

        if storage.blocks.block_count() == 0 {
            let genesis = StoredBlock::genesis(config.chain_id);
            storage.blocks.store_block(genesis).expect("Failed to store genesis block");
            tracing::info!("Created genesis block");
        }

        Self { config, executor, dexvm_executor, consensus: None, storage, evm_rpc_server: None }
    }

    /// Create dual VM node with genesis allocation
    pub fn with_genesis(chain_id: u64, genesis_alloc: HashMap<Address, U256>) -> Self {
        Self::with_genesis_and_datadir(chain_id, genesis_alloc, PathBuf::from("./data"))
    }

    /// Create dual VM node with genesis allocation and data directory
    pub fn with_genesis_and_datadir(
        chain_id: u64,
        genesis_alloc: HashMap<Address, U256>,
        datadir: PathBuf,
    ) -> Self {
        let config = NodeConfig { chain_id, datadir, ..Default::default() };

        let storage = Arc::new(
            DualvmStorage::new(&config.datadir).expect("Failed to initialize MDBX database"),
        );

        if storage.is_new_database() {
            tracing::info!("New database detected, initializing genesis state");
            storage
                .state
                .init_genesis(genesis_alloc)
                .expect("Failed to init genesis state");

            let mut genesis = StoredBlock::genesis(chain_id);
            genesis.evm_state_root = storage.state.state_root();
            genesis.combined_state_root = genesis.evm_state_root;
            storage.blocks.store_block(genesis).expect("Failed to store genesis block");
            tracing::info!("Created genesis block with initial allocations");
        } else {
            tracing::info!(
                "Existing database detected, loading state. Latest block: {}",
                storage.blocks.latest_block_number()
            );
        }

        // Create EVM executor backed by the shared StateStore
        // No need to manually load accounts - StateStore handles persistence
        let evm_executor = Arc::new(RwLock::new(SimpleEvmExecutor::new(
            chain_id,
            Arc::clone(&storage.state),
        )));
        tracing::info!("EVM executor initialized with {} accounts",
            storage.state.all_accounts().len());

        // Load DexVM state from database
        let dexvm_executor = if storage.is_new_database() {
            Arc::new(RwLock::new(DexExecutor::new(DexVmState::default())))
        } else {
            let mut dexvm_state = DexVmState::new();
            let counters = storage.state.all_counters();
            for (address, value) in counters {
                dexvm_state.set_counter(address, value);
            }
            tracing::info!("Loaded {} DexVM counters from storage", dexvm_state.account_count());
            Arc::new(RwLock::new(DexExecutor::new(dexvm_state)))
        };
        let executor = DualVmExecutor::new(evm_executor, Arc::clone(&dexvm_executor));

        Self { config, executor, dexvm_executor, consensus: None, storage, evm_rpc_server: None }
    }

    /// Create node with full configuration
    pub fn with_full_config(
        chain_id: u64,
        genesis_alloc: HashMap<Address, U256>,
        datadir: PathBuf,
        poa_config: Option<PoaConfig>,
    ) -> Self {
        let mut node = Self::with_genesis_and_datadir(chain_id, genesis_alloc, datadir);
        if let Some(config) = poa_config {
            node.consensus = Some(PoaConsensus::new(config));
        }
        node
    }

    /// Set POA consensus configuration
    pub fn set_consensus(&mut self, config: PoaConfig, last_block_hash: B256) {
        let mut consensus = PoaConsensus::new(config);
        consensus.set_last_block_hash(last_block_hash);
        self.consensus = Some(consensus);
    }

    /// Get executor reference
    pub fn executor(&self) -> &DualVmExecutor {
        &self.executor
    }

    /// Get mutable executor reference
    pub fn executor_mut(&mut self) -> &mut DualVmExecutor {
        &mut self.executor
    }

    /// Get block store reference
    pub fn block_store(&self) -> &BlockStore {
        &self.storage.blocks
    }

    /// Get state store reference
    pub fn state_store(&self) -> &StateStore {
        &self.storage.state
    }

    /// Get storage reference
    pub fn storage(&self) -> &Arc<DualvmStorage> {
        &self.storage
    }

    /// Start DexVM REST API service
    pub async fn start_dexvm_rpc(&self, port: u16) -> eyre::Result<JoinHandle<()>> {
        let api = DexVmApi::new(Arc::clone(&self.dexvm_executor));
        let app = api.routes();

        let addr = format!("0.0.0.0:{}", port);
        let listener = tokio::net::TcpListener::bind(&addr).await?;

        tracing::info!("DexVM REST API listening on {}", addr);

        let handle = tokio::spawn(async move {
            if let Err(e) = axum::serve(listener, app).await {
                tracing::error!("DexVM RPC server error: {}", e);
            }
        });

        Ok(handle)
    }

    /// Start EVM JSON-RPC service
    pub async fn start_evm_rpc(&mut self, port: u16) -> eyre::Result<ServerHandle> {
        // Use the shared block_store and state_store from storage
        let state_store = Arc::clone(&self.storage.state);
        let block_store = Arc::clone(&self.storage.blocks);

        let (handle, server) =
            start_evm_rpc_server(self.config.chain_id, state_store, block_store, port).await?;

        self.evm_rpc_server = Some(server);

        Ok(handle)
    }

    /// Get EVM RPC server reference
    pub fn evm_rpc_server(&self) -> Option<&Arc<EvmRpcServer>> {
        self.evm_rpc_server.as_ref()
    }

    /// Get consensus engine reference
    pub fn consensus(&self) -> Option<&PoaConsensus> {
        self.consensus.as_ref()
    }

    /// Start POA consensus engine
    pub fn start_consensus(&self) -> Option<JoinHandle<()>> {
        self.consensus.as_ref().map(|c| c.start())
    }

    /// Run consensus loop
    pub async fn run_consensus_loop(&mut self) -> eyre::Result<()> {
        let consensus =
            self.consensus.as_ref().ok_or_else(|| eyre::eyre!("No consensus engine configured"))?;

        tracing::info!("Starting consensus loop");

        loop {
            if let Some(proposal) = consensus.recv_proposal() {
                tracing::info!(
                    "Received block proposal: block_number={}, tx_count={}",
                    proposal.number,
                    proposal.transactions.len()
                );

                let pending_txs = if let Some(rpc_server) = &self.evm_rpc_server {
                    let txs = rpc_server.get_pending_transactions();
                    rpc_server.clear_pending_transactions();
                    txs
                } else {
                    vec![]
                };

                let mut all_transactions = proposal.transactions.clone();
                for pending in &pending_txs {
                    all_transactions.push(pending.tx.clone());
                }

                match self.executor.execute_transactions(all_transactions.clone()) {
                    Ok(result) => {
                        tracing::info!(
                            "Block executed successfully: gas_used={}, state_root={:?}",
                            result.total_gas_used,
                            result.combined_state_root
                        );

                        let block_hash = keccak256(format!(
                            "block-{}-{}-{}",
                            proposal.number, proposal.timestamp, result.combined_state_root
                        ));

                        let tx_hashes: Vec<B256> =
                            all_transactions.iter().map(|tx| *tx.tx_hash()).collect();

                        let stored_block = StoredBlock {
                            number: proposal.number,
                            hash: block_hash,
                            parent_hash: proposal.parent_hash,
                            timestamp: proposal.timestamp,
                            gas_limit: 30_000_000,
                            gas_used: result.total_gas_used,
                            miner: proposal.proposer,
                            evm_state_root: result.evm_state_root,
                            dexvm_state_root: result.dexvm_state_root,
                            combined_state_root: result.combined_state_root,
                            transaction_hashes: tx_hashes,
                            transaction_count: all_transactions.len() as u64,
                            signature: proposal.signature.to_bytes(),
                        };

                        if let Err(e) = self.storage.blocks.store_block(stored_block) {
                            tracing::error!("Failed to store block: {}", e);
                        }

                        // Persist DexVM state to database
                        if let Ok(dexvm_exec) = self.dexvm_executor.read() {
                            for (address, &value) in dexvm_exec.state().all_accounts() {
                                if let Err(e) = self.storage.state.set_counter(*address, value) {
                                    tracing::error!("Failed to persist DexVM counter for {}: {}", address, e);
                                }
                            }
                        }

                        consensus.finalize_block(result.combined_state_root);

                        tracing::info!(
                            "Block {} finalized and stored, hash={:?}",
                            proposal.number,
                            block_hash
                        );
                    }
                    Err(e) => {
                        tracing::error!("Block execution failed: {}", e);
                    }
                }
            }

            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    }
}

impl Default for DualVmNode {
    fn default() -> Self {
        Self::new(1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_node_creation() {
        let dir = tempdir().unwrap();
        let config =
            NodeConfig { chain_id: 1, datadir: dir.path().to_path_buf(), ..Default::default() };
        let node = DualVmNode::with_config(config);
        assert!(node.executor.dexvm_executor().read().is_ok());
    }

    #[test]
    fn test_node_with_genesis() {
        use alloy_primitives::address;

        let dir = tempdir().unwrap();
        let mut genesis_alloc = HashMap::new();
        genesis_alloc
            .insert(address!("1111111111111111111111111111111111111111"), U256::from(1000));

        let node =
            DualVmNode::with_genesis_and_datadir(13337, genesis_alloc, dir.path().to_path_buf());
        assert!(node.executor.dexvm_executor().read().is_ok());
        assert_eq!(node.block_store().block_count(), 1);
    }

    #[test]
    fn test_genesis_block_persistence() {
        use alloy_primitives::address;

        let dir = tempdir().unwrap();
        let mut genesis_alloc = HashMap::new();
        genesis_alloc
            .insert(address!("1111111111111111111111111111111111111111"), U256::from(1000));

        let node =
            DualVmNode::with_genesis_and_datadir(13337, genesis_alloc, dir.path().to_path_buf());

        let balance =
            node.state_store().get_balance(&address!("1111111111111111111111111111111111111111"));
        assert_eq!(balance, U256::from(1000));
    }

    #[tokio::test]
    async fn test_start_rpc() {
        let dir = tempdir().unwrap();
        let config =
            NodeConfig { chain_id: 1, datadir: dir.path().to_path_buf(), ..Default::default() };
        let node = DualVmNode::with_config(config);

        let port = 0;
        let handle = node.start_dexvm_rpc(port).await;

        assert!(handle.is_ok());

        if let Ok(h) = handle {
            h.abort();
        }
    }
}
