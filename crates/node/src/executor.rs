//! Dual VM executor

use crate::evm_executor::SimpleEvmExecutor;
use alloy_consensus::Transaction;
use alloy_primitives::B256;
use dex_dexvm::{DexVmExecutor, COUNTER_PRECOMPILE_ADDRESS};
use dex_primitives::{DexVmReceipt, DualVmTransaction};
use reth_ethereum_primitives::TransactionSigned;
use reth_execution_errors::BlockExecutionError;
use std::sync::{Arc, RwLock};

/// Dual VM execution result
#[derive(Debug, Clone)]
pub struct DualVmExecutionResult {
    /// EVM receipts
    pub evm_receipts: Vec<alloy_consensus::Receipt>,
    /// DexVM receipts
    pub dexvm_receipts: Vec<DexVmReceipt>,
    /// Total gas used
    pub total_gas_used: u64,
    /// EVM state root
    pub evm_state_root: B256,
    /// DexVM state root
    pub dexvm_state_root: B256,
    /// Combined state root
    pub combined_state_root: B256,
}

/// Dual VM executor
pub struct DualVmExecutor {
    evm_executor: Arc<RwLock<SimpleEvmExecutor>>,
    dexvm_executor: Arc<RwLock<DexVmExecutor>>,
    current_block: u64,
    current_timestamp: u64,
}

impl DualVmExecutor {
    /// Create new dual VM executor
    pub fn new(
        evm_executor: Arc<RwLock<SimpleEvmExecutor>>,
        dexvm_executor: Arc<RwLock<DexVmExecutor>>,
    ) -> Self {
        Self {
            evm_executor,
            dexvm_executor,
            current_block: 0,
            current_timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        }
    }

    /// Advance to next block
    pub fn advance_block(&mut self) {
        self.current_block += 1;
        self.current_timestamp =
            std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_secs();
    }

    /// Execute transactions
    pub fn execute_transactions(
        &mut self,
        transactions: Vec<TransactionSigned>,
    ) -> Result<DualVmExecutionResult, BlockExecutionError> {
        let mut evm_receipts = Vec::new();
        let mut dexvm_receipts = Vec::new();
        let mut total_gas_used = 0u64;

        for tx in transactions {
            let dual_tx = DualVmTransaction::from_ethereum_tx(tx.clone());

            match dual_tx {
                DualVmTransaction::Evm(_evm_tx) => {
                    // Check if this EVM tx is calling the counter precompile
                    let is_precompile_call = tx.to() == Some(COUNTER_PRECOMPILE_ADDRESS);

                    if is_precompile_call {
                        // Cross-VM call: EVM → DexVM via precompile
                        // Need write access to both executors
                        let receipt = self.execute_cross_vm_transaction(&tx)?;
                        total_gas_used += receipt.cumulative_gas_used;
                        evm_receipts.push(receipt);
                    } else {
                        // Regular EVM transaction
                        let mut executor = self
                            .evm_executor
                            .write()
                            .map_err(|e| BlockExecutionError::msg(format!("Lock error: {}", e)))?;

                        let receipt = executor.execute_transaction(
                            &tx,
                            self.current_block,
                            self.current_timestamp,
                        )?;

                        total_gas_used += receipt.cumulative_gas_used;
                        evm_receipts.push(receipt);
                    }
                }
                DualVmTransaction::DexVm(dexvm_tx) => {
                    let mut executor = self
                        .dexvm_executor
                        .write()
                        .map_err(|e| BlockExecutionError::msg(format!("Lock error: {}", e)))?;

                    let result = executor.execute_transaction(&dexvm_tx)?;
                    total_gas_used += result.gas_used;

                    let receipt = DexVmReceipt::from_result(result, dexvm_tx.from);
                    dexvm_receipts.push(receipt);

                    executor.commit();
                }
            }
        }

        // Sync DexVM pending state to committed state before computing roots
        {
            let mut dexvm_executor = self
                .dexvm_executor
                .write()
                .map_err(|e| BlockExecutionError::msg(format!("DexVM lock error: {}", e)))?;
            dexvm_executor.sync_pending_to_state();
        }

        let evm_executor = self
            .evm_executor
            .read()
            .map_err(|e| BlockExecutionError::msg(format!("Lock error: {}", e)))?;
        let dexvm_executor = self
            .dexvm_executor
            .read()
            .map_err(|e| BlockExecutionError::msg(format!("Lock error: {}", e)))?;

        let evm_state_root = evm_executor.state_root();
        let dexvm_state_root = dexvm_executor.state_root();
        let combined_state_root = self.combine_state_roots(evm_state_root, dexvm_state_root);

        Ok(DualVmExecutionResult {
            evm_receipts,
            dexvm_receipts,
            total_gas_used,
            evm_state_root,
            dexvm_state_root,
            combined_state_root,
        })
    }

    /// Execute a cross-VM transaction (EVM → DexVM via precompile)
    ///
    /// This handles atomic execution: if the DexVM operation fails,
    /// both EVM and DexVM state changes are rolled back.
    fn execute_cross_vm_transaction(
        &mut self,
        tx: &TransactionSigned,
    ) -> Result<alloy_consensus::Receipt, BlockExecutionError> {
        // Get write locks on both executors
        let mut evm_executor = self
            .evm_executor
            .write()
            .map_err(|e| BlockExecutionError::msg(format!("EVM lock error: {}", e)))?;

        let mut dexvm_executor = self
            .dexvm_executor
            .write()
            .map_err(|e| BlockExecutionError::msg(format!("DexVM lock error: {}", e)))?;

        // Get mutable reference to DexVM pending state
        // The pending_state is used for atomic operations
        let dexvm_state = dexvm_executor.pending_state_mut();

        // Execute the EVM transaction with DexVM state access
        let receipt = evm_executor.execute_transaction_with_dexvm(
            tx,
            self.current_block,
            self.current_timestamp,
            Some(dexvm_state),
        )?;

        // If the transaction succeeded, commit DexVM state
        // If failed, DexVM state is already unchanged (operation was rejected)
        if receipt.status.coerce_status() {
            tracing::debug!("Cross-VM transaction succeeded, committing DexVM state");
            // Note: We don't call commit() here because we modified pending_state directly
            // The state will be committed when computing state roots
        } else {
            tracing::debug!("Cross-VM transaction failed, DexVM state unchanged");
        }

        Ok(receipt)
    }

    /// Combine two state roots
    fn combine_state_roots(&self, evm_root: B256, dexvm_root: B256) -> B256 {
        use alloy_primitives::keccak256;

        let mut data = Vec::with_capacity(64);
        data.extend_from_slice(evm_root.as_slice());
        data.extend_from_slice(dexvm_root.as_slice());
        keccak256(&data)
    }

    /// Get DexVM executor reference
    pub fn dexvm_executor(&self) -> Arc<RwLock<DexVmExecutor>> {
        Arc::clone(&self.dexvm_executor)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_consensus::transaction::SignerRecoverable;
    use alloy_consensus::TxLegacy;
    use alloy_primitives::{Signature, TxKind, U256};
    use dex_dexvm::{DexVmState, OP_INCREMENT, OP_QUERY};
    use dex_primitives::DEXVM_ROUTER_ADDRESS;
    use dex_storage::{DualvmStorage, StateStore};
    use tempfile::tempdir;

    fn create_test_state_store() -> (Arc<StateStore>, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let storage = DualvmStorage::new(dir.path()).unwrap();
        (Arc::clone(&storage.state), dir)
    }

    #[test]
    fn test_combine_state_roots() {
        let (state_store, _dir) = create_test_state_store();
        let evm_executor = Arc::new(RwLock::new(SimpleEvmExecutor::new(1, state_store)));
        let dexvm_executor = Arc::new(RwLock::new(DexVmExecutor::new(DexVmState::default())));
        let executor = DualVmExecutor::new(evm_executor, dexvm_executor);

        let evm_root = B256::from([1u8; 32]);
        let dexvm_root = B256::from([2u8; 32]);

        let combined1 = executor.combine_state_roots(evm_root, dexvm_root);
        let combined2 = executor.combine_state_roots(evm_root, dexvm_root);

        assert_eq!(combined1, combined2);

        let different_root = executor.combine_state_roots(B256::from([3u8; 32]), dexvm_root);
        assert_ne!(combined1, different_root);
    }

    #[test]
    fn test_execute_dexvm_transaction() {
        let (state_store, _dir) = create_test_state_store();
        let evm_executor = Arc::new(RwLock::new(SimpleEvmExecutor::new(1, state_store)));
        let dexvm_executor = Arc::new(RwLock::new(DexVmExecutor::new(DexVmState::default())));
        let mut executor = DualVmExecutor::new(evm_executor, dexvm_executor);

        let mut calldata = vec![0u8];
        calldata.extend_from_slice(&10u64.to_be_bytes());

        let tx = TransactionSigned::new_unhashed(
            TxLegacy {
                to: TxKind::Call(DEXVM_ROUTER_ADDRESS),
                input: calldata.into(),
                nonce: 0,
                gas_price: 1,
                gas_limit: 100000,
                value: U256::ZERO,
                chain_id: Some(1),
            }
            .into(),
            Signature::test_signature(),
        );

        let result = executor.execute_transactions(vec![tx]).unwrap();

        assert_eq!(result.dexvm_receipts.len(), 1);
        assert!(result.total_gas_used > 0);
        assert_ne!(result.dexvm_state_root, B256::ZERO);
    }

    #[test]
    fn test_cross_vm_transaction_via_precompile() {
        // Create calldata for counter increment: [0x00][amount: 8 bytes]
        let mut calldata = vec![OP_INCREMENT];
        calldata.extend_from_slice(&25u64.to_be_bytes());

        // Create the transaction first, then get the caller address from it
        let tx = TransactionSigned::new_unhashed(
            TxLegacy {
                to: TxKind::Call(COUNTER_PRECOMPILE_ADDRESS),
                input: calldata.into(),
                nonce: 0,
                gas_price: 1,
                gas_limit: 100000,
                value: U256::ZERO,
                chain_id: Some(1),
            }
            .into(),
            Signature::test_signature(),
        );

        // Get the actual caller address from the transaction we'll execute
        let caller = tx.recover_signer().unwrap();

        // Setup EVM executor with funded account
        let (state_store, _dir) = create_test_state_store();
        let mut evm_exec = SimpleEvmExecutor::new(1, state_store);
        evm_exec.set_balance(caller, U256::from(1_000_000_000u64));

        let evm_executor = Arc::new(RwLock::new(evm_exec));
        let dexvm_executor = Arc::new(RwLock::new(DexVmExecutor::new(DexVmState::default())));
        let mut executor = DualVmExecutor::new(evm_executor.clone(), dexvm_executor.clone());

        let result = executor.execute_transactions(vec![tx]).unwrap();

        // Should have one EVM receipt (the precompile call)
        assert_eq!(result.evm_receipts.len(), 1);
        assert!(result.evm_receipts[0].status.coerce_status());

        // DexVM state should be updated
        let dexvm = dexvm_executor.read().unwrap();
        assert_eq!(dexvm.state().get_counter(&caller), 25);
    }

    #[test]
    fn test_cross_vm_query_via_precompile() {
        // Create calldata for counter query: [0x02][padding: 8 bytes]
        let mut calldata = vec![OP_QUERY];
        calldata.extend_from_slice(&0u64.to_be_bytes());

        // Create the transaction first to get the correct caller
        let tx = TransactionSigned::new_unhashed(
            TxLegacy {
                to: TxKind::Call(COUNTER_PRECOMPILE_ADDRESS),
                input: calldata.into(),
                nonce: 0,
                gas_price: 1,
                gas_limit: 100000,
                value: U256::ZERO,
                chain_id: Some(1),
            }
            .into(),
            Signature::test_signature(),
        );

        let caller = tx.recover_signer().unwrap();

        // Setup EVM executor with funded account
        let (state_store, _dir) = create_test_state_store();
        let mut evm_exec = SimpleEvmExecutor::new(1, state_store);
        evm_exec.set_balance(caller, U256::from(1_000_000_000u64));
        let evm_executor = Arc::new(RwLock::new(evm_exec));

        // Set initial counter value for the caller
        let mut dexvm_state = DexVmState::new();
        dexvm_state.set_counter(caller, 100);
        let dexvm_executor = Arc::new(RwLock::new(DexVmExecutor::new(dexvm_state)));

        let mut executor = DualVmExecutor::new(evm_executor, dexvm_executor.clone());

        let result = executor.execute_transactions(vec![tx]).unwrap();

        assert_eq!(result.evm_receipts.len(), 1);
        assert!(result.evm_receipts[0].status.coerce_status());

        // Counter should remain unchanged (query operation)
        let dexvm = dexvm_executor.read().unwrap();
        assert_eq!(dexvm.state().get_counter(&caller), 100);
    }
}
