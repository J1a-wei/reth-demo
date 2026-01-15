//! Simple EVM executor

use alloy_consensus::{transaction::SignerRecoverable, Receipt, Transaction};
use alloy_primitives::{Address, B256, U256};
use dex_dexvm::{PrecompileExecutor, DEPOSIT_PRECOMPILE_ADDRESS};
use reth_ethereum_primitives::TransactionSigned;
use reth_execution_errors::BlockExecutionError;
use std::collections::HashMap;

/// Simple EVM executor
pub struct SimpleEvmExecutor {
    /// Account balances
    balances: HashMap<Address, U256>,
    /// Precompile executor
    precompile_executor: PrecompileExecutor,
    /// Chain ID
    chain_id: u64,
}

impl SimpleEvmExecutor {
    /// Create new EVM executor
    pub fn new(chain_id: u64) -> Self {
        Self { balances: HashMap::new(), precompile_executor: PrecompileExecutor::new(), chain_id }
    }

    /// Create executor with genesis state
    pub fn with_genesis(chain_id: u64, genesis_alloc: HashMap<Address, U256>) -> Self {
        let mut executor = Self::new(chain_id);
        executor.balances = genesis_alloc;
        executor
    }

    /// Set account balance
    pub fn set_balance(&mut self, address: Address, balance: U256) {
        self.balances.insert(address, balance);
    }

    /// Get account balance
    pub fn get_balance(&mut self, address: Address) -> U256 {
        self.balances.get(&address).copied().unwrap_or(U256::ZERO)
    }

    /// Get account count
    pub fn account_count(&self) -> usize {
        self.balances.len()
    }

    /// Execute single transaction
    pub fn execute_transaction(
        &mut self,
        tx: &TransactionSigned,
        _block_number: u64,
        _timestamp: u64,
    ) -> Result<Receipt, BlockExecutionError> {
        let caller = tx
            .recover_signer()
            .map_err(|_| BlockExecutionError::msg("Failed to recover transaction signer"))?;

        // Check if it's a precompile call
        if let Some(to) = tx.to() {
            if to == DEPOSIT_PRECOMPILE_ADDRESS {
                return self.execute_precompile_transaction(tx, caller);
            }
        }

        let caller_balance = self.get_balance(caller);
        let tx_value = tx.value();
        let tx_cost = tx_value + U256::from(tx.gas_limit() as u128 * tx.effective_gas_price(None));

        if caller_balance < tx_cost {
            return Ok(Receipt { status: false.into(), cumulative_gas_used: 21000, logs: vec![] });
        }

        self.set_balance(caller, caller_balance - tx_cost);

        if let Some(to) = tx.to() {
            let to_balance = self.get_balance(to);
            self.set_balance(to, to_balance + tx_value);
        }

        Ok(Receipt { status: true.into(), cumulative_gas_used: 21000, logs: vec![] })
    }

    fn execute_precompile_transaction(
        &mut self,
        tx: &TransactionSigned,
        caller: Address,
    ) -> Result<Receipt, BlockExecutionError> {
        let caller_balance = self.get_balance(caller);
        let tx_value = tx.value();
        let gas_cost = U256::from(tx.gas_limit() as u128 * tx.effective_gas_price(None));
        let tx_cost = tx_value + gas_cost;

        if caller_balance < tx_cost {
            tracing::error!("Insufficient balance: have {}, need {}", caller_balance, tx_cost);
            return Ok(Receipt { status: false.into(), cumulative_gas_used: 21000, logs: vec![] });
        }

        self.set_balance(caller, caller_balance - tx_cost);

        let result = self.precompile_executor.execute(
            caller,
            DEPOSIT_PRECOMPILE_ADDRESS,
            tx.input(),
            tx_value,
        )?;

        tracing::debug!(
            "Precompile execution: success={}, gas_used={}",
            result.success,
            result.gas_used
        );

        Ok(Receipt { status: result.success.into(), cumulative_gas_used: result.gas_used, logs: vec![] })
    }

    /// Get precompile balance
    pub fn get_precompile_balance(&self, address: &Address) -> U256 {
        self.precompile_executor.state().get_balance(address)
    }

    /// Calculate state root
    pub fn state_root(&mut self) -> B256 {
        use alloy_primitives::keccak256;

        let mut accounts: Vec<(Address, U256)> =
            self.balances.iter().map(|(addr, balance)| (*addr, *balance)).collect();

        accounts.sort_by_key(|(addr, _)| *addr);

        let mut data = Vec::new();
        for (addr, balance) in accounts {
            data.extend_from_slice(addr.as_slice());
            data.extend_from_slice(&balance.to_be_bytes::<32>());
        }

        if data.is_empty() {
            B256::ZERO
        } else {
            keccak256(&data)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_consensus::TxLegacy;
    use alloy_primitives::{address, Signature, TxKind};

    #[test]
    fn test_create_executor() {
        let executor = SimpleEvmExecutor::new(1);
        assert_eq!(executor.chain_id, 1);
    }

    #[test]
    fn test_set_get_balance() {
        let mut executor = SimpleEvmExecutor::new(1);
        let address = address!("1111111111111111111111111111111111111111");

        executor.set_balance(address, U256::from(1000));
        assert_eq!(executor.get_balance(address), U256::from(1000));
    }

    #[test]
    fn test_precompile_deposit() {
        let mut executor = SimpleEvmExecutor::new(1);

        let tx = TransactionSigned::new_unhashed(
            TxLegacy {
                to: TxKind::Call(DEPOSIT_PRECOMPILE_ADDRESS),
                value: U256::from(1_000_000_000_000_000_000u64),
                input: Default::default(),
                nonce: 0,
                gas_price: 1,
                gas_limit: 100000,
                chain_id: Some(1),
            }
            .into(),
            Signature::test_signature(),
        );

        let caller = tx.recover_signer().unwrap();

        executor.set_balance(caller, U256::from(10_000_000_000_000_000_000u64));

        let receipt = executor.execute_transaction(&tx, 1, 0).unwrap();

        assert_eq!(receipt.status, true.into());
        assert_eq!(
            executor.get_precompile_balance(&caller),
            U256::from(1_000_000_000_000_000_000u64)
        );
    }

    #[test]
    fn test_with_genesis() {
        let mut genesis_alloc = HashMap::new();
        genesis_alloc.insert(address!("1111111111111111111111111111111111111111"), U256::from(1000));
        genesis_alloc.insert(address!("2222222222222222222222222222222222222222"), U256::from(2000));

        let mut executor = SimpleEvmExecutor::with_genesis(1, genesis_alloc);

        assert_eq!(
            executor.get_balance(address!("1111111111111111111111111111111111111111")),
            U256::from(1000)
        );
        assert_eq!(
            executor.get_balance(address!("2222222222222222222222222222222222222222")),
            U256::from(2000)
        );
    }
}
