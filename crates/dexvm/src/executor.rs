use crate::state::DexVmState;
use dex_primitives::{DexVmExecutionResult, DexVmOperation, DexVmTransaction};
use reth_execution_errors::BlockExecutionError;

/// Gas cost constants for DexVM operations
const BASE_GAS: u64 = 21000;
const INCREMENT_GAS: u64 = 5000;
const DECREMENT_GAS: u64 = 5000;
const QUERY_GAS: u64 = 3000;

/// DexVM executor
///
/// Executes DexVM transactions against the DexVM state
pub struct DexVmExecutor {
    /// Current state
    state: DexVmState,
    /// Pending state changes (for rollback)
    pending_state: DexVmState,
    /// Whether there are pending changes
    has_pending: bool,
}

impl DexVmExecutor {
    /// Create new executor with given state
    pub fn new(state: DexVmState) -> Self {
        let pending_state = state.clone();
        Self { state, pending_state, has_pending: false }
    }

    /// Execute a transaction
    pub fn execute_transaction(
        &mut self,
        tx: &DexVmTransaction,
    ) -> Result<DexVmExecutionResult, BlockExecutionError> {
        let old_counter = self.pending_state.get_counter(&tx.from);

        let (success, new_counter, gas_used, error) = match tx.operation {
            DexVmOperation::Increment(amount) => {
                let new_val = self.pending_state.increment_counter(tx.from, amount);
                (true, new_val, BASE_GAS + INCREMENT_GAS, None)
            }
            DexVmOperation::Decrement(amount) => {
                match self.pending_state.decrement_counter(tx.from, amount) {
                    Ok(new_val) => (true, new_val, BASE_GAS + DECREMENT_GAS, None),
                    Err(e) => (false, old_counter, BASE_GAS + DECREMENT_GAS, Some(e)),
                }
            }
            DexVmOperation::Query => (true, old_counter, BASE_GAS + QUERY_GAS, None),
        };

        self.has_pending = true;

        Ok(DexVmExecutionResult { success, old_counter, new_counter, gas_used, error })
    }

    /// Commit pending state changes
    pub fn commit(&mut self) {
        if self.has_pending {
            self.state = self.pending_state.clone();
            self.has_pending = false;
        }
    }

    /// Rollback pending state changes
    pub fn rollback(&mut self) {
        if self.has_pending {
            self.pending_state = self.state.clone();
            self.has_pending = false;
        }
    }

    /// Get current state reference
    pub fn state(&self) -> &DexVmState {
        &self.state
    }

    /// Get pending state reference
    pub fn pending_state(&self) -> &DexVmState {
        &self.pending_state
    }

    /// Get state root (from committed state)
    pub fn state_root(&self) -> alloy_primitives::B256 {
        self.state.state_root()
    }

    /// Check if there are pending changes
    pub fn has_pending_changes(&self) -> bool {
        self.has_pending
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::address;

    #[test]
    fn test_increment_transaction() {
        let mut executor = DexVmExecutor::new(DexVmState::new());
        let from = address!("1111111111111111111111111111111111111111");

        let tx = DexVmTransaction { from, operation: DexVmOperation::Increment(10), signature: vec![] };

        let result = executor.execute_transaction(&tx).unwrap();
        assert!(result.success);
        assert_eq!(result.old_counter, 0);
        assert_eq!(result.new_counter, 10);
        assert_eq!(result.gas_used, BASE_GAS + INCREMENT_GAS);

        // Commit to persist changes
        executor.commit();
        assert_eq!(executor.state().get_counter(&from), 10);
    }

    #[test]
    fn test_decrement_transaction() {
        let mut state = DexVmState::new();
        let from = address!("2222222222222222222222222222222222222222");
        state.set_counter(from, 100);

        let mut executor = DexVmExecutor::new(state);

        let tx = DexVmTransaction { from, operation: DexVmOperation::Decrement(30), signature: vec![] };

        let result = executor.execute_transaction(&tx).unwrap();
        assert!(result.success);
        assert_eq!(result.old_counter, 100);
        assert_eq!(result.new_counter, 70);

        executor.commit();
        assert_eq!(executor.state().get_counter(&from), 70);
    }

    #[test]
    fn test_decrement_underflow() {
        let mut state = DexVmState::new();
        let from = address!("3333333333333333333333333333333333333333");
        state.set_counter(from, 10);

        let mut executor = DexVmExecutor::new(state);

        let tx = DexVmTransaction { from, operation: DexVmOperation::Decrement(100), signature: vec![] };

        let result = executor.execute_transaction(&tx).unwrap();
        assert!(!result.success);
        assert!(result.error.is_some());
        assert_eq!(result.old_counter, 10);
        assert_eq!(result.new_counter, 10); // Unchanged
    }

    #[test]
    fn test_rollback() {
        let mut executor = DexVmExecutor::new(DexVmState::new());
        let from = address!("4444444444444444444444444444444444444444");

        let tx = DexVmTransaction { from, operation: DexVmOperation::Increment(50), signature: vec![] };

        executor.execute_transaction(&tx).unwrap();
        assert!(executor.has_pending_changes());

        // Rollback should restore original state
        executor.rollback();
        assert!(!executor.has_pending_changes());
        assert_eq!(executor.state().get_counter(&from), 0);
    }

    #[test]
    fn test_query_transaction() {
        let mut state = DexVmState::new();
        let from = address!("5555555555555555555555555555555555555555");
        state.set_counter(from, 42);

        let mut executor = DexVmExecutor::new(state);

        let tx = DexVmTransaction { from, operation: DexVmOperation::Query, signature: vec![] };

        let result = executor.execute_transaction(&tx).unwrap();
        assert!(result.success);
        assert_eq!(result.old_counter, 42);
        assert_eq!(result.new_counter, 42); // Query doesn't change value
        assert_eq!(result.gas_used, BASE_GAS + QUERY_GAS);
    }
}
