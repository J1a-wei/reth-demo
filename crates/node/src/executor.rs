//! Dual VM executor

use crate::evm_executor::SimpleEvmExecutor;
use alloy_primitives::B256;
use dex_dexvm::DexVmExecutor;
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

        let mut evm_executor = self
            .evm_executor
            .write()
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
    use alloy_consensus::TxLegacy;
    use dex_dexvm::DexVmState;
    use dex_primitives::DEXVM_ROUTER_ADDRESS;

    #[test]
    fn test_combine_state_roots() {
        let evm_executor = Arc::new(RwLock::new(SimpleEvmExecutor::new(1)));
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
        let evm_executor = Arc::new(RwLock::new(SimpleEvmExecutor::new(1)));
        let dexvm_executor = Arc::new(RwLock::new(DexVmExecutor::new(DexVmState::default())));
        let mut executor = DualVmExecutor::new(evm_executor, dexvm_executor);

        let mut calldata = vec![0u8];
        calldata.extend_from_slice(&10u64.to_be_bytes());

        let tx = TransactionSigned::new_unhashed(
            TxLegacy {
                to: alloy_primitives::TxKind::Call(DEXVM_ROUTER_ADDRESS),
                input: calldata.into(),
                nonce: 0,
                gas_price: 1,
                gas_limit: 100000,
                value: alloy_primitives::U256::ZERO,
                chain_id: Some(1),
            }
            .into(),
            alloy_primitives::Signature::test_signature(),
        );

        let result = executor.execute_transactions(vec![tx]).unwrap();

        assert_eq!(result.dexvm_receipts.len(), 1);
        assert!(result.total_gas_used > 0);
        assert_ne!(result.dexvm_state_root, B256::ZERO);
    }
}
