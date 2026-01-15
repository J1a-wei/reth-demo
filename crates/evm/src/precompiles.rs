//! Custom precompiled contracts for DEX functionality
//!
//! Provides precompiles for cross-VM communication and DEX-specific operations.

use alloy_primitives::{address, Address, Bytes, U256};
use revm::precompile::{PrecompileError, PrecompileOutput, PrecompileResult};
use std::collections::HashMap;

/// DEX precompile address for cross-VM bridge
pub const DEX_BRIDGE_ADDRESS: Address = address!("0000000000000000000000000000000000000200");

/// DEX deposit/withdraw precompile address
pub const DEX_DEPOSIT_ADDRESS: Address = address!("0000000000000000000000000000000000000100");

/// Custom precompiles for DEX functionality
pub struct DexPrecompiles {
    /// Registered precompiles
    precompiles: HashMap<Address, Box<dyn Fn(&Bytes, u64) -> PrecompileResult + Send + Sync>>,
}

impl Default for DexPrecompiles {
    fn default() -> Self {
        Self::new()
    }
}

impl DexPrecompiles {
    /// Create new DEX precompiles set
    pub fn new() -> Self {
        let mut precompiles = HashMap::<
            Address,
            Box<dyn Fn(&Bytes, u64) -> PrecompileResult + Send + Sync>,
        >::new();

        // Register DEX bridge precompile
        precompiles.insert(
            DEX_BRIDGE_ADDRESS,
            Box::new(|input, gas_limit| dex_bridge_precompile(input, gas_limit)),
        );

        // Register deposit precompile
        precompiles.insert(
            DEX_DEPOSIT_ADDRESS,
            Box::new(|input, gas_limit| deposit_precompile(input, gas_limit)),
        );

        Self { precompiles }
    }

    /// Check if address is a DEX precompile
    pub fn contains(&self, address: &Address) -> bool {
        self.precompiles.contains_key(address)
    }

    /// Execute precompile
    pub fn execute(&self, address: &Address, input: &Bytes, gas_limit: u64) -> Option<PrecompileResult> {
        self.precompiles
            .get(address)
            .map(|f| f(input, gas_limit))
    }

    /// Get all precompile addresses
    pub fn addresses(&self) -> Vec<Address> {
        self.precompiles.keys().copied().collect()
    }
}

/// DEX bridge precompile for cross-VM communication
///
/// Input format:
/// - [0]: operation type (0=query_counter, 1=increment, 2=decrement)
/// - [1..21]: target address (20 bytes)
/// - [21..29]: amount (8 bytes, big-endian, for increment/decrement)
///
/// Output format:
/// - [0..8]: counter value (8 bytes, big-endian)
fn dex_bridge_precompile(input: &Bytes, gas_limit: u64) -> PrecompileResult {
    const BASE_GAS: u64 = 100;
    const QUERY_GAS: u64 = 50;
    const MODIFY_GAS: u64 = 200;

    if gas_limit < BASE_GAS {
        return Err(PrecompileError::OutOfGas);
    }

    if input.is_empty() {
        // Empty call returns zero counter
        return Ok(PrecompileOutput::new(
            BASE_GAS,
            Bytes::from(0u64.to_be_bytes().to_vec()),
        ));
    }

    let op_type = input[0];

    match op_type {
        0 => {
            // Query counter
            let gas_used = BASE_GAS + QUERY_GAS;
            if gas_limit < gas_used {
                return Err(PrecompileError::OutOfGas);
            }

            // In a real implementation, this would query the DexVM state
            // For now, return a placeholder value
            Ok(PrecompileOutput::new(
                gas_used,
                Bytes::from(0u64.to_be_bytes().to_vec()),
            ))
        }
        1 | 2 => {
            // Increment or decrement
            let gas_used = BASE_GAS + MODIFY_GAS;
            if gas_limit < gas_used {
                return Err(PrecompileError::OutOfGas);
            }

            if input.len() < 29 {
                return Err(PrecompileError::other(
                    "Invalid input length for increment/decrement",
                ));
            }

            // In a real implementation, this would modify the DexVM state
            // For now, return success with zero value
            Ok(PrecompileOutput::new(
                gas_used,
                Bytes::from(0u64.to_be_bytes().to_vec()),
            ))
        }
        _ => Err(PrecompileError::other(
            "Unknown operation type",
        )),
    }
}

/// Deposit precompile for ETH deposits to DEX
///
/// When called with ETH value:
/// - Empty input: Deposit ETH to sender's DEX balance
/// - Input with amount: Withdraw specified amount
/// - Input with address: Query DEX balance
fn deposit_precompile(input: &Bytes, gas_limit: u64) -> PrecompileResult {
    const BASE_GAS: u64 = 50;
    const DEPOSIT_GAS: u64 = 100;
    const WITHDRAW_GAS: u64 = 150;
    const QUERY_GAS: u64 = 30;

    if gas_limit < BASE_GAS {
        return Err(PrecompileError::OutOfGas);
    }

    if input.is_empty() {
        // Deposit operation (when called with value) or query
        let gas_used = BASE_GAS + QUERY_GAS;
        if gas_limit < gas_used {
            return Err(PrecompileError::OutOfGas);
        }

        // Return current balance (placeholder)
        let balance = U256::ZERO;
        Ok(PrecompileOutput::new(
            gas_used,
            Bytes::from(balance.to_be_bytes::<32>().to_vec()),
        ))
    } else if input.len() == 8 {
        // Withdraw specified amount
        let gas_used = BASE_GAS + WITHDRAW_GAS;
        if gas_limit < gas_used {
            return Err(PrecompileError::OutOfGas);
        }

        let _amount = u64::from_be_bytes(input[..8].try_into().unwrap());

        // In a real implementation, this would withdraw from DEX balance
        Ok(PrecompileOutput::new(gas_used, Bytes::new()))
    } else if input.len() == 20 {
        // Query balance for address
        let gas_used = BASE_GAS + QUERY_GAS;
        if gas_limit < gas_used {
            return Err(PrecompileError::OutOfGas);
        }

        let _address = Address::from_slice(&input[..20]);

        // Return balance (placeholder)
        let balance = U256::ZERO;
        Ok(PrecompileOutput::new(
            gas_used,
            Bytes::from(balance.to_be_bytes::<32>().to_vec()),
        ))
    } else {
        Err(PrecompileError::other(
            "Invalid input length",
        ))
    }
}

/// Precompile registry that combines standard and DEX precompiles
pub struct PrecompileRegistry {
    /// DEX-specific precompiles
    dex_precompiles: DexPrecompiles,
}

impl Default for PrecompileRegistry {
    fn default() -> Self {
        Self::new()
    }
}

impl PrecompileRegistry {
    /// Create new precompile registry
    pub fn new() -> Self {
        Self {
            dex_precompiles: DexPrecompiles::new(),
        }
    }

    /// Check if address is a custom precompile
    pub fn is_precompile(&self, address: &Address) -> bool {
        self.dex_precompiles.contains(address)
    }

    /// Execute custom precompile
    pub fn execute(&self, address: &Address, input: &Bytes, gas_limit: u64) -> Option<PrecompileResult> {
        self.dex_precompiles.execute(address, input, gas_limit)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dex_precompiles_creation() {
        let precompiles = DexPrecompiles::new();
        assert!(precompiles.contains(&DEX_BRIDGE_ADDRESS));
        assert!(precompiles.contains(&DEX_DEPOSIT_ADDRESS));
    }

    #[test]
    fn test_bridge_query() {
        let precompiles = DexPrecompiles::new();
        let input = Bytes::from(vec![0u8]); // Query operation
        let result = precompiles.execute(&DEX_BRIDGE_ADDRESS, &input, 1000);
        assert!(result.is_some());
        assert!(result.unwrap().is_ok());
    }

    #[test]
    fn test_deposit_query() {
        let precompiles = DexPrecompiles::new();
        let input = Bytes::new(); // Empty = query
        let result = precompiles.execute(&DEX_DEPOSIT_ADDRESS, &input, 1000);
        assert!(result.is_some());
        assert!(result.unwrap().is_ok());
    }

    #[test]
    fn test_precompile_registry() {
        let registry = PrecompileRegistry::new();
        assert!(registry.is_precompile(&DEX_BRIDGE_ADDRESS));
        assert!(!registry.is_precompile(&Address::ZERO));
    }
}
