use alloy_primitives::Address;
use serde::{Deserialize, Serialize};

/// DexVM execution result
#[derive(Debug, Clone)]
pub struct DexVmExecutionResult {
    /// Whether execution succeeded
    pub success: bool,
    /// Old counter value
    pub old_counter: u64,
    /// New counter value
    pub new_counter: u64,
    /// Gas consumed
    pub gas_used: u64,
    /// Error message
    pub error: Option<String>,
}

/// DexVM transaction receipt
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DexVmReceipt {
    /// Transaction sender address
    pub from: Address,
    /// Whether execution succeeded
    pub success: bool,
    /// Counter value before execution
    pub old_counter: u64,
    /// Counter value after execution
    pub new_counter: u64,
    /// Gas consumed
    pub gas_used: u64,
    /// Error message (if any)
    pub error: Option<String>,
}

impl From<DexVmExecutionResult> for DexVmReceipt {
    fn from(result: DexVmExecutionResult) -> Self {
        Self {
            from: Address::ZERO, // Need to get from transaction
            success: result.success,
            old_counter: result.old_counter,
            new_counter: result.new_counter,
            gas_used: result.gas_used,
            error: result.error,
        }
    }
}

impl DexVmReceipt {
    /// Create a new DexVM receipt
    pub fn new(
        from: Address,
        success: bool,
        old_counter: u64,
        new_counter: u64,
        gas_used: u64,
        error: Option<String>,
    ) -> Self {
        Self { from, success, old_counter, new_counter, gas_used, error }
    }

    /// Create receipt from execution result and sender address
    pub fn from_result(result: DexVmExecutionResult, from: Address) -> Self {
        Self {
            from,
            success: result.success,
            old_counter: result.old_counter,
            new_counter: result.new_counter,
            gas_used: result.gas_used,
            error: result.error,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::address;

    #[test]
    fn test_receipt_creation() {
        let from = address!("0000000000000000000000000000000000000001");
        let receipt = DexVmReceipt::new(from, true, 10, 20, 21000, None);

        assert_eq!(receipt.from, from);
        assert!(receipt.success);
        assert_eq!(receipt.old_counter, 10);
        assert_eq!(receipt.new_counter, 20);
        assert_eq!(receipt.gas_used, 21000);
        assert!(receipt.error.is_none());
    }

    #[test]
    fn test_receipt_with_error() {
        let from = address!("0000000000000000000000000000000000000001");
        let error_msg = "Insufficient counter".to_string();
        let receipt = DexVmReceipt::new(from, false, 5, 5, 21000, Some(error_msg.clone()));

        assert!(!receipt.success);
        assert_eq!(receipt.error, Some(error_msg));
    }
}
