//! EVM executor using revm
//!
//! Provides complete EVM execution environment for transaction processing.

use crate::db::MdbxDatabase;
use alloy_primitives::{Address, Bytes, TxKind, B256, U256};
use dex_storage::StateStore;
use revm::{
    context::{
        result::{ExecutionResult as RevmExecutionResult, Output},
        BlockEnv, CfgEnv, Context, JournalTr, LocalContext, TxEnv,
    },
    database::State,
    primitives::hardfork::SpecId,
    ExecuteCommitEvm, ExecuteEvm, Journal, JournalEntry, MainBuilder,
};
use std::sync::Arc;

/// Execution result from EVM
#[derive(Debug, Clone)]
pub struct ExecutionResult {
    /// Whether execution succeeded
    pub success: bool,
    /// Gas used
    pub gas_used: u64,
    /// Gas refunded
    pub gas_refunded: u64,
    /// Output data (return data or deployed address)
    pub output: ExecutionOutput,
    /// Logs emitted
    pub logs: Vec<Log>,
    /// Revert reason if failed
    pub revert_reason: Option<String>,
}

/// Output type from execution
#[derive(Debug, Clone)]
pub enum ExecutionOutput {
    /// Call returned data
    Call(Bytes),
    /// Contract created at address
    Create(Address, Option<Bytes>),
}

/// Log entry from execution
#[derive(Debug, Clone)]
pub struct Log {
    /// Contract address that emitted the log
    pub address: Address,
    /// Log topics
    pub topics: Vec<B256>,
    /// Log data
    pub data: Bytes,
}

impl From<alloy_primitives::Log> for Log {
    fn from(log: alloy_primitives::Log) -> Self {
        Self {
            address: log.address,
            topics: log.data.topics().to_vec(),
            data: log.data.data.clone(),
        }
    }
}

/// EVM executor configuration
#[derive(Debug, Clone)]
pub struct ExecutorConfig {
    /// Chain ID
    pub chain_id: u64,
    /// Spec ID (EVM version)
    pub spec_id: SpecId,
    /// Whether to enable custom precompiles
    pub enable_dex_precompiles: bool,
}

impl Default for ExecutorConfig {
    fn default() -> Self {
        Self {
            chain_id: 1,
            spec_id: SpecId::CANCUN,
            enable_dex_precompiles: true,
        }
    }
}

impl ExecutorConfig {
    /// Create config with chain ID
    pub fn with_chain_id(mut self, chain_id: u64) -> Self {
        self.chain_id = chain_id;
        self
    }

    /// Create config with spec ID
    pub fn with_spec_id(mut self, spec_id: SpecId) -> Self {
        self.spec_id = spec_id;
        self
    }
}

/// EVM executor using revm
pub struct EvmExecutor {
    /// Configuration
    config: ExecutorConfig,
    /// State store
    state_store: Arc<StateStore>,
}

impl EvmExecutor {
    /// Create new executor with state store
    pub fn new(state_store: Arc<StateStore>) -> Self {
        Self {
            config: ExecutorConfig::default(),
            state_store,
        }
    }

    /// Create executor with custom config
    pub fn with_config(state_store: Arc<StateStore>, config: ExecutorConfig) -> Self {
        Self { config, state_store }
    }

    /// Get configuration
    pub fn config(&self) -> &ExecutorConfig {
        &self.config
    }

    /// Get state store
    pub fn state_store(&self) -> &Arc<StateStore> {
        &self.state_store
    }

    /// Execute a transaction with full EVM execution
    pub fn execute_transaction(
        &self,
        tx: &TransactionInput,
        block: &BlockContext,
    ) -> Result<ExecutionResult, ExecutionError> {
        // Create database wrapper
        let mut db = MdbxDatabase::new(Arc::clone(&self.state_store));

        // Set block hashes
        for (number, hash) in &block.block_hashes {
            db.set_block_hash(*number, *hash);
        }

        // Simple validation
        if tx.gas_limit < 21000 {
            return Err(ExecutionError::InvalidTransaction(
                "Gas limit too low".to_string(),
            ));
        }

        // Create revm State database with journaling
        let state_db = State::builder().with_database(db).build();

        // Create block environment
        let block_env = BlockEnv {
            number: U256::from(block.number),
            beneficiary: block.coinbase,
            timestamp: U256::from(block.timestamp),
            gas_limit: block.gas_limit,
            basefee: block.base_fee,
            prevrandao: Some(block.prevrandao),
            ..Default::default()
        };

        // Create transaction environment
        let tx_env = TxEnv {
            caller: tx.from,
            kind: match tx.to {
                Some(to) => TxKind::Call(to),
                None => TxKind::Create,
            },
            value: tx.value,
            data: tx.data.clone(),
            gas_limit: tx.gas_limit,
            gas_price: tx.gas_price.to(),
            nonce: tx.nonce.unwrap_or(0),
            ..Default::default()
        };

        // Create config
        let cfg = CfgEnv::new_with_spec(self.config.spec_id)
            .with_chain_id(self.config.chain_id);

        // Create journal for state management
        let journal: Journal<State<MdbxDatabase>, JournalEntry> = Journal::new(state_db);

        // Create context with configuration
        let ctx = Context {
            block: block_env,
            tx: tx_env.clone(),
            cfg,
            journaled_state: journal,
            chain: (),
            local: LocalContext::default(),
            error: Ok(()),
        };

        // Build the mainnet EVM
        let mut evm = ctx.build_mainnet();

        // Execute transaction and commit state
        match evm.transact_commit(tx_env) {
            Ok(result) => Ok(self.convert_result(&result)),
            Err(e) => Err(ExecutionError::Execution(format!("{:?}", e))),
        }
    }

    /// Execute a call (read-only, no state changes)
    pub fn execute_call(
        &self,
        tx: &TransactionInput,
        block: &BlockContext,
    ) -> Result<ExecutionResult, ExecutionError> {
        // Create database wrapper
        let mut db = MdbxDatabase::new(Arc::clone(&self.state_store));

        // Set block hashes
        for (number, hash) in &block.block_hashes {
            db.set_block_hash(*number, *hash);
        }

        // Create revm State database with journaling
        let state_db = State::builder().with_database(db).build();

        // Create block environment
        let block_env = BlockEnv {
            number: U256::from(block.number),
            beneficiary: block.coinbase,
            timestamp: U256::from(block.timestamp),
            gas_limit: block.gas_limit,
            basefee: block.base_fee,
            prevrandao: Some(block.prevrandao),
            ..Default::default()
        };

        // Create transaction environment
        let tx_env = TxEnv {
            caller: tx.from,
            kind: match tx.to {
                Some(to) => TxKind::Call(to),
                None => TxKind::Create,
            },
            value: tx.value,
            data: tx.data.clone(),
            gas_limit: tx.gas_limit,
            gas_price: tx.gas_price.to(),
            nonce: tx.nonce.unwrap_or(0),
            ..Default::default()
        };

        // Create config
        let cfg = CfgEnv::new_with_spec(self.config.spec_id)
            .with_chain_id(self.config.chain_id);

        // Create journal for state management
        let journal: Journal<State<MdbxDatabase>, JournalEntry> = Journal::new(state_db);

        // Create context with configuration
        let ctx = Context {
            block: block_env,
            tx: tx_env.clone(),
            cfg,
            journaled_state: journal,
            chain: (),
            local: LocalContext::default(),
            error: Ok(()),
        };

        // Build the mainnet EVM
        let mut evm = ctx.build_mainnet();

        // Execute transaction without committing (read-only call)
        match evm.transact(tx_env) {
            Ok(result_and_state) => Ok(self.convert_result(&result_and_state.result)),
            Err(e) => Err(ExecutionError::Execution(format!("{:?}", e))),
        }
    }

    /// Estimate gas for a transaction
    pub fn estimate_gas(
        &self,
        tx: &TransactionInput,
        block: &BlockContext,
    ) -> Result<u64, ExecutionError> {
        // Start with a high gas limit
        let mut high = block.gas_limit;
        let mut low: u64 = 21000; // Minimum gas for a transaction

        // Binary search for minimum gas
        while low + 1 < high {
            let mid = (low + high) / 2;

            let mut test_tx = tx.clone();
            test_tx.gas_limit = mid;

            match self.execute_call(&test_tx, block) {
                Ok(result) if result.success => {
                    high = mid;
                }
                _ => {
                    low = mid;
                }
            }
        }

        // Add safety margin
        Ok((high as f64 * 1.1) as u64)
    }

    /// Convert revm result to our result type
    #[allow(dead_code)]
    fn convert_result(&self, result: &RevmExecutionResult) -> ExecutionResult {
        match result {
            RevmExecutionResult::Success { reason: _, gas_used, gas_refunded, output, logs } => {
                let output = match output {
                    Output::Call(data) => ExecutionOutput::Call(data.clone()),
                    Output::Create(data, addr) => ExecutionOutput::Create(
                        addr.unwrap_or(Address::ZERO),
                        if data.is_empty() { None } else { Some(data.clone()) },
                    ),
                };

                ExecutionResult {
                    success: true,
                    gas_used: *gas_used,
                    gas_refunded: *gas_refunded,
                    output,
                    logs: logs.iter().cloned().map(Into::into).collect(),
                    revert_reason: None,
                }
            }
            RevmExecutionResult::Revert { gas_used, output } => {
                // Try to decode revert reason
                let revert_reason = if output.len() >= 4 {
                    // Check for Error(string) selector
                    if output[0..4] == [0x08, 0xc3, 0x79, 0xa0] {
                        decode_revert_reason(&output[4..])
                    } else {
                        None
                    }
                } else {
                    None
                };

                ExecutionResult {
                    success: false,
                    gas_used: *gas_used,
                    gas_refunded: 0,
                    output: ExecutionOutput::Call(output.clone()),
                    logs: Vec::new(),
                    revert_reason,
                }
            }
            RevmExecutionResult::Halt { reason, gas_used } => ExecutionResult {
                success: false,
                gas_used: *gas_used,
                gas_refunded: 0,
                output: ExecutionOutput::Call(Bytes::new()),
                logs: Vec::new(),
                revert_reason: Some(format!("Halt: {:?}", reason)),
            },
        }
    }
}

/// Transaction input for EVM execution
#[derive(Debug, Clone)]
pub struct TransactionInput {
    /// Sender address
    pub from: Address,
    /// Recipient address (None for contract creation)
    pub to: Option<Address>,
    /// Value to transfer
    pub value: U256,
    /// Input data
    pub data: Bytes,
    /// Gas limit
    pub gas_limit: u64,
    /// Gas price
    pub gas_price: U256,
    /// Max priority fee per gas (EIP-1559)
    pub max_priority_fee_per_gas: Option<U256>,
    /// Nonce
    pub nonce: Option<u64>,
}

impl Default for TransactionInput {
    fn default() -> Self {
        Self {
            from: Address::ZERO,
            to: None,
            value: U256::ZERO,
            data: Bytes::new(),
            gas_limit: 21000,
            gas_price: U256::ZERO,
            max_priority_fee_per_gas: None,
            nonce: None,
        }
    }
}

impl TransactionInput {
    /// Create new transaction input
    pub fn new() -> Self {
        Self::default()
    }

    /// Set sender
    pub fn with_from(mut self, from: Address) -> Self {
        self.from = from;
        self
    }

    /// Set recipient
    pub fn with_to(mut self, to: Address) -> Self {
        self.to = Some(to);
        self
    }

    /// Set value
    pub fn with_value(mut self, value: U256) -> Self {
        self.value = value;
        self
    }

    /// Set data
    pub fn with_data(mut self, data: Bytes) -> Self {
        self.data = data;
        self
    }

    /// Set gas limit
    pub fn with_gas_limit(mut self, gas_limit: u64) -> Self {
        self.gas_limit = gas_limit;
        self
    }

    /// Set gas price
    pub fn with_gas_price(mut self, gas_price: U256) -> Self {
        self.gas_price = gas_price;
        self
    }

    /// Set nonce
    pub fn with_nonce(mut self, nonce: u64) -> Self {
        self.nonce = Some(nonce);
        self
    }
}

/// Block context for EVM execution
#[derive(Debug, Clone)]
pub struct BlockContext {
    /// Block number
    pub number: u64,
    /// Block timestamp
    pub timestamp: u64,
    /// Block gas limit
    pub gas_limit: u64,
    /// Base fee
    pub base_fee: u64,
    /// Coinbase (block producer)
    pub coinbase: Address,
    /// Previous block randomness (prevrandao)
    pub prevrandao: B256,
    /// Historical block hashes
    pub block_hashes: Vec<(u64, B256)>,
}

impl Default for BlockContext {
    fn default() -> Self {
        Self {
            number: 0,
            timestamp: 0,
            gas_limit: 30_000_000,
            base_fee: 0,
            coinbase: Address::ZERO,
            prevrandao: B256::ZERO,
            block_hashes: Vec::new(),
        }
    }
}

impl BlockContext {
    /// Create new block context
    pub fn new(number: u64, timestamp: u64) -> Self {
        Self {
            number,
            timestamp,
            ..Default::default()
        }
    }

    /// Set gas limit
    pub fn with_gas_limit(mut self, gas_limit: u64) -> Self {
        self.gas_limit = gas_limit;
        self
    }

    /// Set base fee
    pub fn with_base_fee(mut self, base_fee: u64) -> Self {
        self.base_fee = base_fee;
        self
    }

    /// Set coinbase
    pub fn with_coinbase(mut self, coinbase: Address) -> Self {
        self.coinbase = coinbase;
        self
    }

    /// Add block hash
    pub fn with_block_hash(mut self, number: u64, hash: B256) -> Self {
        self.block_hashes.push((number, hash));
        self
    }
}

/// Execution error types
#[derive(Debug, thiserror::Error)]
pub enum ExecutionError {
    #[error("Database error: {0}")]
    Database(String),

    #[error("Execution error: {0}")]
    Execution(String),

    #[error("Invalid transaction: {0}")]
    InvalidTransaction(String),
}

/// Decode revert reason from ABI-encoded Error(string)
fn decode_revert_reason(data: &[u8]) -> Option<String> {
    if data.len() < 64 {
        return None;
    }

    // Decode string offset (should be 32)
    let offset = U256::from_be_slice(&data[0..32]).to::<usize>();
    if offset != 32 || data.len() < 64 {
        return None;
    }

    // Decode string length
    let length = U256::from_be_slice(&data[32..64]).to::<usize>();
    if data.len() < 64 + length {
        return None;
    }

    // Decode string data
    String::from_utf8(data[64..64 + length].to_vec()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use dex_storage::{DualvmStorage, StateStore};
    use tempfile::tempdir;

    fn create_test_executor() -> (tempfile::TempDir, EvmExecutor) {
        let dir = tempdir().unwrap();
        let storage = DualvmStorage::new(dir.path()).unwrap();
        let state_store = StateStore::new(storage.db.clone());
        (dir, EvmExecutor::new(Arc::new(state_store)))
    }

    #[test]
    fn test_executor_creation() {
        let (_dir, executor) = create_test_executor();
        assert_eq!(executor.config().chain_id, 1);
    }

    #[test]
    fn test_transaction_input() {
        let tx = TransactionInput::new()
            .with_from(Address::ZERO)
            .with_to(Address::ZERO)
            .with_gas_limit(21000);

        assert_eq!(tx.gas_limit, 21000);
    }

    #[test]
    fn test_block_context() {
        let block = BlockContext::new(1, 1000)
            .with_gas_limit(30_000_000)
            .with_base_fee(1000);

        assert_eq!(block.number, 1);
        assert_eq!(block.timestamp, 1000);
    }
}
