use crate::state::DexVmState;
use alloy_primitives::Address;
use reth_execution_errors::BlockExecutionError;

/// Counter precompile address (for EVM → DexVM cross-VM calls)
pub const COUNTER_PRECOMPILE_ADDRESS: Address =
    alloy_primitives::address!("0000000000000000000000000000000000000100");

/// Counter operation opcodes
pub const OP_INCREMENT: u8 = 0x00;
pub const OP_DECREMENT: u8 = 0x01;
pub const OP_QUERY: u8 = 0x02;

/// Precompile operation type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrecompileOperation {
    /// Increment counter - calldata: [0x00][amount: 8 bytes]
    IncrementCounter(u64),
    /// Decrement counter - calldata: [0x01][amount: 8 bytes]
    DecrementCounter(u64),
    /// Query counter - calldata: [0x02][padding: 8 bytes]
    QueryCounter,
    /// Invalid operation
    Invalid,
}

/// Precompile execution result
#[derive(Debug, Clone)]
pub struct PrecompileResult {
    /// Whether execution succeeded
    pub success: bool,
    /// Return data
    pub return_data: Vec<u8>,
    /// Gas consumed
    pub gas_used: u64,
    /// Error message
    pub error: Option<String>,
}

/// Gas constants for counter operations
const COUNTER_INCREMENT_GAS: u64 = 26000;
const COUNTER_DECREMENT_GAS: u64 = 26000;
const COUNTER_QUERY_GAS: u64 = 24000;

/// Precompile executor for counter operations
#[derive(Debug, Default)]
pub struct PrecompileExecutor;

impl PrecompileExecutor {
    /// Create new executor
    pub fn new() -> Self {
        Self
    }

    /// Execute precompile call with DexVM state for counter operations
    pub fn execute_with_dexvm(
        &self,
        caller: Address,
        to: Address,
        input: &[u8],
        dexvm_state: Option<&mut DexVmState>,
    ) -> Result<PrecompileResult, BlockExecutionError> {
        if to != COUNTER_PRECOMPILE_ADDRESS {
            return Err(BlockExecutionError::msg(format!("Unknown precompile address: {:?}", to)));
        }

        let operation = Self::parse_operation(input);

        match operation {
            PrecompileOperation::IncrementCounter(amount) => {
                let dexvm = dexvm_state.ok_or_else(|| {
                    BlockExecutionError::msg("DexVM state required for counter operations")
                })?;

                let new_value = dexvm.increment_counter(caller, amount);
                tracing::debug!(
                    "Counter increment: address={}, amount={}, new_value={}",
                    caller,
                    amount,
                    new_value
                );

                Ok(PrecompileResult {
                    success: true,
                    return_data: new_value.to_be_bytes().to_vec(),
                    gas_used: COUNTER_INCREMENT_GAS,
                    error: None,
                })
            }
            PrecompileOperation::DecrementCounter(amount) => {
                let dexvm = dexvm_state.ok_or_else(|| {
                    BlockExecutionError::msg("DexVM state required for counter operations")
                })?;

                match dexvm.decrement_counter(caller, amount) {
                    Ok(new_value) => {
                        tracing::debug!(
                            "Counter decrement: address={}, amount={}, new_value={}",
                            caller,
                            amount,
                            new_value
                        );
                        Ok(PrecompileResult {
                            success: true,
                            return_data: new_value.to_be_bytes().to_vec(),
                            gas_used: COUNTER_DECREMENT_GAS,
                            error: None,
                        })
                    }
                    Err(err) => {
                        tracing::warn!("Counter decrement failed: address={}, error={}", caller, err);
                        Ok(PrecompileResult {
                            success: false,
                            return_data: vec![],
                            gas_used: COUNTER_DECREMENT_GAS,
                            error: Some(err),
                        })
                    }
                }
            }
            PrecompileOperation::QueryCounter => {
                let dexvm = dexvm_state.ok_or_else(|| {
                    BlockExecutionError::msg("DexVM state required for counter operations")
                })?;

                let value = dexvm.get_counter(&caller);
                tracing::debug!("Counter query: address={}, value={}", caller, value);

                Ok(PrecompileResult {
                    success: true,
                    return_data: value.to_be_bytes().to_vec(),
                    gas_used: COUNTER_QUERY_GAS,
                    error: None,
                })
            }
            PrecompileOperation::Invalid => {
                Ok(PrecompileResult {
                    success: false,
                    return_data: vec![],
                    gas_used: 3000,
                    error: Some("Invalid counter operation".to_string()),
                })
            }
        }
    }

    /// Parse calldata to determine operation type
    ///
    /// Counter operation format: [op: 1 byte][amount: 8 bytes big-endian]
    /// - op = 0x00 → Increment
    /// - op = 0x01 → Decrement
    /// - op = 0x02 → Query
    fn parse_operation(input: &[u8]) -> PrecompileOperation {
        if input.len() != 9 {
            return PrecompileOperation::Invalid;
        }

        let op = input[0];
        let amount = u64::from_be_bytes(input[1..9].try_into().unwrap());

        match op {
            OP_INCREMENT => PrecompileOperation::IncrementCounter(amount),
            OP_DECREMENT => PrecompileOperation::DecrementCounter(amount),
            OP_QUERY => PrecompileOperation::QueryCounter,
            _ => PrecompileOperation::Invalid,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::address;

    // Helper to create counter operation calldata
    fn make_counter_calldata(op: u8, amount: u64) -> Vec<u8> {
        let mut data = vec![op];
        data.extend_from_slice(&amount.to_be_bytes());
        data
    }

    #[test]
    fn test_counter_increment() {
        let executor = PrecompileExecutor::new();
        let mut dexvm_state = DexVmState::new();
        let caller = address!("6666666666666666666666666666666666666666");

        let calldata = make_counter_calldata(OP_INCREMENT, 10);
        let result = executor
            .execute_with_dexvm(caller, COUNTER_PRECOMPILE_ADDRESS, &calldata, Some(&mut dexvm_state))
            .unwrap();

        assert!(result.success);
        assert_eq!(result.gas_used, COUNTER_INCREMENT_GAS);

        let new_value = u64::from_be_bytes(result.return_data.try_into().unwrap());
        assert_eq!(new_value, 10);
        assert_eq!(dexvm_state.get_counter(&caller), 10);
    }

    #[test]
    fn test_counter_decrement() {
        let executor = PrecompileExecutor::new();
        let mut dexvm_state = DexVmState::new();
        let caller = address!("7777777777777777777777777777777777777777");

        dexvm_state.set_counter(caller, 100);

        let calldata = make_counter_calldata(OP_DECREMENT, 30);
        let result = executor
            .execute_with_dexvm(caller, COUNTER_PRECOMPILE_ADDRESS, &calldata, Some(&mut dexvm_state))
            .unwrap();

        assert!(result.success);
        assert_eq!(result.gas_used, COUNTER_DECREMENT_GAS);

        let new_value = u64::from_be_bytes(result.return_data.try_into().unwrap());
        assert_eq!(new_value, 70);
        assert_eq!(dexvm_state.get_counter(&caller), 70);
    }

    #[test]
    fn test_counter_decrement_underflow() {
        let executor = PrecompileExecutor::new();
        let mut dexvm_state = DexVmState::new();
        let caller = address!("8888888888888888888888888888888888888888");

        dexvm_state.set_counter(caller, 10);

        let calldata = make_counter_calldata(OP_DECREMENT, 100);
        let result = executor
            .execute_with_dexvm(caller, COUNTER_PRECOMPILE_ADDRESS, &calldata, Some(&mut dexvm_state))
            .unwrap();

        assert!(!result.success);
        assert!(result.error.is_some());
        assert_eq!(dexvm_state.get_counter(&caller), 10);
    }

    #[test]
    fn test_counter_query() {
        let executor = PrecompileExecutor::new();
        let mut dexvm_state = DexVmState::new();
        let caller = address!("9999999999999999999999999999999999999999");

        dexvm_state.set_counter(caller, 42);

        let calldata = make_counter_calldata(OP_QUERY, 0);
        let result = executor
            .execute_with_dexvm(caller, COUNTER_PRECOMPILE_ADDRESS, &calldata, Some(&mut dexvm_state))
            .unwrap();

        assert!(result.success);
        assert_eq!(result.gas_used, COUNTER_QUERY_GAS);

        let value = u64::from_be_bytes(result.return_data.try_into().unwrap());
        assert_eq!(value, 42);
    }

    #[test]
    fn test_invalid_operation() {
        let executor = PrecompileExecutor::new();
        let mut dexvm_state = DexVmState::new();
        let caller = address!("aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa");

        // Invalid calldata (wrong length)
        let result = executor
            .execute_with_dexvm(caller, COUNTER_PRECOMPILE_ADDRESS, &[0x00], Some(&mut dexvm_state))
            .unwrap();

        assert!(!result.success);
        assert!(result.error.is_some());
    }

    #[test]
    fn test_counter_operation_without_dexvm_state() {
        let executor = PrecompileExecutor::new();
        let caller = address!("bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb");

        let calldata = make_counter_calldata(OP_INCREMENT, 10);
        let result = executor.execute_with_dexvm(caller, COUNTER_PRECOMPILE_ADDRESS, &calldata, None);

        assert!(result.is_err());
    }
}
