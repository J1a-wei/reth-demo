use alloy_primitives::{Address, U256};
use reth_execution_errors::BlockExecutionError;
use std::collections::HashMap;

/// Deposit/withdraw precompile address
pub const DEPOSIT_PRECOMPILE_ADDRESS: Address =
    alloy_primitives::address!("0000000000000000000000000000000000000100");

/// Precompile operation type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrecompileOperation {
    /// Deposit - value: deposit amount
    Deposit,
    /// Withdraw - calldata: amount (32 bytes)
    Withdraw,
    /// Query balance - calldata: empty
    GetBalance,
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

/// Precompile state
///
/// Manages ETH balances for the deposit/withdraw precompile
#[derive(Debug, Clone, Default)]
pub struct PrecompileState {
    /// Account balances
    balances: HashMap<Address, U256>,
}

impl PrecompileState {
    /// Create new state
    pub fn new() -> Self {
        Self { balances: HashMap::new() }
    }

    /// Deposit ETH
    pub fn deposit(&mut self, address: Address, amount: U256) {
        let balance = self.balances.entry(address).or_insert(U256::ZERO);
        *balance = balance.saturating_add(amount);
    }

    /// Withdraw ETH
    pub fn withdraw(&mut self, address: Address, amount: U256) -> Result<(), String> {
        let balance = self.balances.entry(address).or_insert(U256::ZERO);

        if *balance < amount {
            return Err(format!("Insufficient balance: have {}, want {}", balance, amount));
        }

        *balance = balance.saturating_sub(amount);
        Ok(())
    }

    /// Get balance
    pub fn get_balance(&self, address: &Address) -> U256 {
        self.balances.get(address).copied().unwrap_or(U256::ZERO)
    }
}

/// Precompile executor
#[derive(Debug, Default)]
pub struct PrecompileExecutor {
    state: PrecompileState,
}

impl PrecompileExecutor {
    /// Create new executor
    pub fn new() -> Self {
        Self { state: PrecompileState::new() }
    }

    /// Execute precompile call
    pub fn execute(
        &mut self,
        caller: Address,
        to: Address,
        input: &[u8],
        value: U256,
    ) -> Result<PrecompileResult, BlockExecutionError> {
        if to != DEPOSIT_PRECOMPILE_ADDRESS {
            return Err(BlockExecutionError::msg(format!("Unknown precompile address: {:?}", to)));
        }

        let operation = self.parse_operation(input, value);

        match operation {
            PrecompileOperation::Deposit => {
                if value.is_zero() {
                    return Ok(PrecompileResult {
                        success: false,
                        return_data: vec![],
                        gas_used: 5000,
                        error: Some("Deposit amount must be greater than 0".to_string()),
                    });
                }

                self.state.deposit(caller, value);

                Ok(PrecompileResult {
                    success: true,
                    return_data: vec![],
                    gas_used: 20000,
                    error: None,
                })
            }
            PrecompileOperation::Withdraw => {
                if input.len() < 32 {
                    return Ok(PrecompileResult {
                        success: false,
                        return_data: vec![],
                        gas_used: 5000,
                        error: Some("Invalid withdraw calldata".to_string()),
                    });
                }

                let amount = U256::from_be_slice(&input[0..32]);

                match self.state.withdraw(caller, amount) {
                    Ok(()) => Ok(PrecompileResult {
                        success: true,
                        return_data: vec![],
                        gas_used: 20000,
                        error: None,
                    }),
                    Err(err) => Ok(PrecompileResult {
                        success: false,
                        return_data: vec![],
                        gas_used: 20000,
                        error: Some(err),
                    }),
                }
            }
            PrecompileOperation::GetBalance => {
                let balance = self.state.get_balance(&caller);
                let balance_bytes = balance.to_be_bytes::<32>();

                Ok(PrecompileResult {
                    success: true,
                    return_data: balance_bytes.to_vec(),
                    gas_used: 5000,
                    error: None,
                })
            }
        }
    }

    fn parse_operation(&self, input: &[u8], value: U256) -> PrecompileOperation {
        if !value.is_zero() {
            return PrecompileOperation::Deposit;
        }

        if input.is_empty() || input.iter().all(|&b| b == 0) {
            return PrecompileOperation::GetBalance;
        }

        PrecompileOperation::Withdraw
    }

    /// Get state reference
    pub fn state(&self) -> &PrecompileState {
        &self.state
    }

    /// Get mutable state reference
    pub fn state_mut(&mut self) -> &mut PrecompileState {
        &mut self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::address;

    #[test]
    fn test_deposit() {
        let mut executor = PrecompileExecutor::new();
        let caller = address!("1111111111111111111111111111111111111111");
        let amount = U256::from(1000);

        let result = executor.execute(caller, DEPOSIT_PRECOMPILE_ADDRESS, &[], amount).unwrap();

        assert!(result.success);
        assert_eq!(result.gas_used, 20000);
        assert_eq!(executor.state().get_balance(&caller), amount);
    }

    #[test]
    fn test_deposit_zero_is_query() {
        let mut executor = PrecompileExecutor::new();
        let caller = address!("2222222222222222222222222222222222222222");

        let result = executor.execute(caller, DEPOSIT_PRECOMPILE_ADDRESS, &[], U256::ZERO).unwrap();

        assert!(result.success);
        assert_eq!(result.gas_used, 5000);
        assert_eq!(result.return_data.len(), 32);
    }

    #[test]
    fn test_withdraw_success() {
        let mut executor = PrecompileExecutor::new();
        let caller = address!("3333333333333333333333333333333333333333");

        // First deposit
        executor.state_mut().deposit(caller, U256::from(1000));

        // Withdraw 500
        let calldata = U256::from(500).to_be_bytes::<32>().to_vec();
        let result =
            executor.execute(caller, DEPOSIT_PRECOMPILE_ADDRESS, &calldata, U256::ZERO).unwrap();

        assert!(result.success);
        assert_eq!(executor.state().get_balance(&caller), U256::from(500));
    }

    #[test]
    fn test_withdraw_insufficient_balance() {
        let mut executor = PrecompileExecutor::new();
        let caller = address!("4444444444444444444444444444444444444444");

        executor.state_mut().deposit(caller, U256::from(100));

        let calldata = U256::from(500).to_be_bytes::<32>().to_vec();
        let result =
            executor.execute(caller, DEPOSIT_PRECOMPILE_ADDRESS, &calldata, U256::ZERO).unwrap();

        assert!(!result.success);
        assert!(result.error.is_some());
        assert_eq!(executor.state().get_balance(&caller), U256::from(100));
    }

    #[test]
    fn test_get_balance() {
        let mut executor = PrecompileExecutor::new();
        let caller = address!("5555555555555555555555555555555555555555");

        executor.state_mut().deposit(caller, U256::from(1000));

        let result = executor.execute(caller, DEPOSIT_PRECOMPILE_ADDRESS, &[], U256::ZERO).unwrap();

        assert!(result.success);
        assert_eq!(result.gas_used, 5000);
        assert_eq!(result.return_data.len(), 32);

        let balance = U256::from_be_slice(&result.return_data);
        assert_eq!(balance, U256::from(1000));
    }
}
