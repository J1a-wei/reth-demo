//! Simple EVM executor

use alloy_consensus::{transaction::SignerRecoverable, Receipt, Transaction};
use alloy_primitives::{Address, B256, U256};
use dex_dexvm::{DexVmState, PrecompileExecutor, COUNTER_PRECOMPILE_ADDRESS};
use dex_storage::StateStore;
use reth_ethereum_primitives::TransactionSigned;
use reth_execution_errors::BlockExecutionError;
use std::sync::Arc;

/// Simple EVM executor backed by persistent StateStore
pub struct SimpleEvmExecutor {
    /// Shared state store (MDBX-backed)
    state_store: Arc<StateStore>,
    /// Precompile executor
    precompile_executor: PrecompileExecutor,
    /// Chain ID
    #[allow(dead_code)]
    chain_id: u64,
}

impl SimpleEvmExecutor {
    /// Create new EVM executor with state store
    pub fn new(chain_id: u64, state_store: Arc<StateStore>) -> Self {
        Self { state_store, precompile_executor: PrecompileExecutor::new(), chain_id }
    }

    /// Set account balance
    pub fn set_balance(&mut self, address: Address, balance: U256) {
        let _ = self.state_store.set_balance(address, balance);
    }

    /// Get account balance
    pub fn get_balance(&self, address: &Address) -> U256 {
        self.state_store.get_balance(address)
    }

    /// Get account count
    pub fn account_count(&self) -> usize {
        self.state_store.all_accounts().len()
    }

    /// Execute single transaction
    pub fn execute_transaction(
        &mut self,
        tx: &TransactionSigned,
        _block_number: u64,
        _timestamp: u64,
    ) -> Result<Receipt, BlockExecutionError> {
        self.execute_transaction_with_dexvm(tx, _block_number, _timestamp, None)
    }

    /// Execute single transaction with DexVM state for cross-VM calls
    pub fn execute_transaction_with_dexvm(
        &mut self,
        tx: &TransactionSigned,
        _block_number: u64,
        _timestamp: u64,
        dexvm_state: Option<&mut DexVmState>,
    ) -> Result<Receipt, BlockExecutionError> {
        let caller = tx
            .recover_signer()
            .map_err(|_| BlockExecutionError::msg("Failed to recover transaction signer"))?;

        // Check if it's a precompile call
        if let Some(to) = tx.to() {
            if to == COUNTER_PRECOMPILE_ADDRESS {
                return self.execute_precompile_transaction_with_dexvm(tx, caller, dexvm_state);
            }
        }

        let caller_balance = self.get_balance(&caller);
        let caller_nonce = self.state_store.get_nonce(&caller);
        let tx_value = tx.value();
        let tx_cost = tx_value + U256::from(tx.gas_limit() as u128 * tx.effective_gas_price(None));

        // Check nonce
        if tx.nonce() != caller_nonce {
            tracing::warn!(
                "Nonce mismatch for {}: expected {}, got {}",
                caller, caller_nonce, tx.nonce()
            );
            return Ok(Receipt { status: false.into(), cumulative_gas_used: 21000, logs: vec![] });
        }

        // Check balance
        if caller_balance < tx_cost {
            tracing::warn!(
                "Insufficient balance for {}: have {}, need {}",
                caller, caller_balance, tx_cost
            );
            return Ok(Receipt { status: false.into(), cumulative_gas_used: 21000, logs: vec![] });
        }

        // Deduct balance and increment nonce
        let new_balance = caller_balance - tx_cost;
        self.set_balance(caller, new_balance);
        let new_nonce = self.state_store.increment_nonce(caller).unwrap_or(caller_nonce + 1);

        tracing::info!(
            "TX executed: from={}, to={:?}, value={}, gas_cost={}, balance: {} -> {}, nonce: {} -> {}",
            caller,
            tx.to(),
            tx_value,
            tx_cost - tx_value,
            caller_balance,
            new_balance,
            caller_nonce,
            new_nonce
        );

        // Transfer value to recipient
        if let Some(to) = tx.to() {
            let to_balance = self.get_balance(&to);
            let to_new_balance = to_balance + tx_value;
            self.set_balance(to, to_new_balance);
            tracing::debug!("Recipient {} balance: {} -> {}", to, to_balance, to_new_balance);
        }

        Ok(Receipt { status: true.into(), cumulative_gas_used: 21000, logs: vec![] })
    }

    fn execute_precompile_transaction_with_dexvm(
        &mut self,
        tx: &TransactionSigned,
        caller: Address,
        dexvm_state: Option<&mut DexVmState>,
    ) -> Result<Receipt, BlockExecutionError> {
        let caller_balance = self.get_balance(&caller);
        let caller_nonce = self.state_store.get_nonce(&caller);
        let tx_value = tx.value();
        let gas_cost = U256::from(tx.gas_limit() as u128 * tx.effective_gas_price(None));
        let tx_cost = tx_value + gas_cost;

        // Check nonce
        if tx.nonce() != caller_nonce {
            tracing::warn!(
                "Nonce mismatch for {}: expected {}, got {}",
                caller, caller_nonce, tx.nonce()
            );
            return Ok(Receipt { status: false.into(), cumulative_gas_used: 21000, logs: vec![] });
        }

        // Check balance
        if caller_balance < tx_cost {
            tracing::error!("Insufficient balance: have {}, need {}", caller_balance, tx_cost);
            return Ok(Receipt { status: false.into(), cumulative_gas_used: 21000, logs: vec![] });
        }

        // Save original balance for potential rollback
        let original_balance = caller_balance;
        self.set_balance(caller, caller_balance - tx_cost);

        let result = self.precompile_executor.execute_with_dexvm(
            caller,
            COUNTER_PRECOMPILE_ADDRESS,
            tx.input(),
            dexvm_state,
        )?;

        tracing::debug!(
            "Precompile execution: success={}, gas_used={}",
            result.success,
            result.gas_used,
        );

        // If counter operation failed, rollback EVM state changes (but still increment nonce)
        if !result.success {
            tracing::warn!("Counter operation failed, rolling back EVM state: {:?}", result.error);
            self.set_balance(caller, original_balance);
        }

        // Increment nonce regardless of success (gas is still consumed)
        let _ = self.state_store.increment_nonce(caller);

        Ok(Receipt { status: result.success.into(), cumulative_gas_used: result.gas_used, logs: vec![] })
    }

    /// Calculate state root
    pub fn state_root(&self) -> B256 {
        self.state_store.state_root()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_consensus::TxLegacy;
    use alloy_primitives::{address, Signature, TxKind};
    use dex_storage::DualvmStorage;
    use tempfile::tempdir;

    fn create_test_state_store() -> (Arc<StateStore>, tempfile::TempDir) {
        let dir = tempdir().unwrap();
        let storage = DualvmStorage::new(dir.path()).unwrap();
        (Arc::clone(&storage.state), dir)
    }

    #[test]
    fn test_create_executor() {
        let (state_store, _dir) = create_test_state_store();
        let executor = SimpleEvmExecutor::new(1, state_store);
        assert_eq!(executor.chain_id, 1);
    }

    #[test]
    fn test_set_get_balance() {
        let (state_store, _dir) = create_test_state_store();
        let mut executor = SimpleEvmExecutor::new(1, state_store);
        let addr = address!("1111111111111111111111111111111111111111");

        executor.set_balance(addr, U256::from(1000));
        assert_eq!(executor.get_balance(&addr), U256::from(1000));
    }

    #[test]
    fn test_precompile_counter_increment() {
        use dex_dexvm::OP_INCREMENT;

        let (state_store, _dir) = create_test_state_store();
        let mut executor = SimpleEvmExecutor::new(1, state_store);
        let mut dexvm_state = DexVmState::new();

        // Create calldata: op (1 byte) + amount (8 bytes)
        let mut calldata = vec![OP_INCREMENT];
        calldata.extend_from_slice(&10u64.to_be_bytes());

        let tx = TransactionSigned::new_unhashed(
            TxLegacy {
                to: TxKind::Call(COUNTER_PRECOMPILE_ADDRESS),
                value: U256::ZERO,
                input: calldata.into(),
                nonce: 0,
                gas_price: 1,
                gas_limit: 100000,
                chain_id: Some(1),
            }
            .into(),
            Signature::test_signature(),
        );

        let caller = tx.recover_signer().unwrap();
        executor.set_balance(caller, U256::from(1_000_000u64));

        let receipt = executor.execute_transaction_with_dexvm(&tx, 1, 0, Some(&mut dexvm_state)).unwrap();

        assert_eq!(receipt.status, true.into());
        assert_eq!(dexvm_state.get_counter(&caller), 10);
    }

    #[test]
    fn test_precompile_counter_decrement_rollback() {
        use dex_dexvm::OP_DECREMENT;

        let (state_store, _dir) = create_test_state_store();
        let mut executor = SimpleEvmExecutor::new(1, state_store);
        let mut dexvm_state = DexVmState::new();

        // Set initial counter to 5
        let caller = address!("1234567890123456789012345678901234567890");
        dexvm_state.set_counter(caller, 5);

        // Try to decrement by 100 (should fail)
        let mut calldata = vec![OP_DECREMENT];
        calldata.extend_from_slice(&100u64.to_be_bytes());

        let tx = TransactionSigned::new_unhashed(
            TxLegacy {
                to: TxKind::Call(COUNTER_PRECOMPILE_ADDRESS),
                value: U256::ZERO,
                input: calldata.into(),
                nonce: 0,
                gas_price: 1,
                gas_limit: 100000,
                chain_id: Some(1),
            }
            .into(),
            Signature::test_signature(),
        );

        // Note: We can't easily test with a specific caller due to signature recovery
        // This test verifies the mechanism works in general
        let recovered_caller = tx.recover_signer().unwrap();
        executor.set_balance(recovered_caller, U256::from(1_000_000u64));

        let original_balance = executor.get_balance(&recovered_caller);
        let receipt = executor.execute_transaction_with_dexvm(&tx, 1, 0, Some(&mut dexvm_state)).unwrap();

        // Transaction should fail (status false)
        assert_eq!(receipt.status, false.into());

        // EVM balance should be restored (rollback)
        assert_eq!(executor.get_balance(&recovered_caller), original_balance);
    }
}
