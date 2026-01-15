use alloy_consensus::Transaction;
use alloy_primitives::{Address, B256};
use reth_ethereum_primitives::TransactionSigned;
use reth_primitives_traits::SignerRecoverable;

/// DexVM router address - transactions sent to this address are routed to DexVM
pub const DEXVM_ROUTER_ADDRESS: Address =
    alloy_primitives::address!("ddddddddddddddddddddddddddddddddddddddd1");

/// DexVM operation type
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DexVmOperation {
    /// Increment counter
    Increment(u64),
    /// Decrement counter
    Decrement(u64),
    /// Query counter
    Query,
}

/// DexVM transaction
#[derive(Debug, Clone)]
pub struct DexVmTransaction {
    /// Sender address
    pub from: Address,
    /// Operation type
    pub operation: DexVmOperation,
    /// Signature (simplified)
    pub signature: Vec<u8>,
}

impl DexVmTransaction {
    /// Decode DexVM transaction from calldata
    /// Format: [op_type: u8][amount: u64]
    /// op_type: 0 = Increment, 1 = Decrement, 2 = Query
    pub fn decode_calldata(from: Address, calldata: &[u8]) -> Result<Self, String> {
        if calldata.is_empty() {
            return Err("Empty calldata".to_string());
        }

        let op_type = calldata[0];
        let operation = match op_type {
            0 => {
                if calldata.len() < 9 {
                    return Err("Invalid increment calldata length".to_string());
                }
                let amount = u64::from_be_bytes(
                    calldata[1..9].try_into().map_err(|_| "Invalid amount bytes")?,
                );
                DexVmOperation::Increment(amount)
            }
            1 => {
                if calldata.len() < 9 {
                    return Err("Invalid decrement calldata length".to_string());
                }
                let amount = u64::from_be_bytes(
                    calldata[1..9].try_into().map_err(|_| "Invalid amount bytes")?,
                );
                DexVmOperation::Decrement(amount)
            }
            2 => DexVmOperation::Query,
            _ => return Err(format!("Unknown operation type: {}", op_type)),
        };

        Ok(Self { from, operation, signature: vec![] })
    }

    /// Calculate transaction hash (simplified)
    pub fn hash(&self) -> B256 {
        use alloy_primitives::keccak256;
        let mut data = Vec::new();
        data.extend_from_slice(self.from.as_slice());
        match self.operation {
            DexVmOperation::Increment(amount) => {
                data.push(0);
                data.extend_from_slice(&amount.to_be_bytes());
            }
            DexVmOperation::Decrement(amount) => {
                data.push(1);
                data.extend_from_slice(&amount.to_be_bytes());
            }
            DexVmOperation::Query => {
                data.push(2);
            }
        }
        keccak256(&data)
    }
}

/// Dual VM transaction enum
#[derive(Debug, Clone)]
pub enum DualVmTransaction {
    /// EVM transaction
    Evm(TransactionSigned),
    /// DexVM transaction
    DexVm(DexVmTransaction),
}

impl DualVmTransaction {
    /// Parse from Ethereum transaction
    /// Rule: if to address is the special DexVM contract address, route to DexVM
    pub fn from_ethereum_tx(tx: TransactionSigned) -> Self {
        if let Some(to) = tx.to() {
            if to == DEXVM_ROUTER_ADDRESS {
                // Try to recover signer address
                if let Ok(from) = tx.recover_signer() {
                    // Parse calldata as DexVM operation
                    if let Ok(dexvm_tx) = DexVmTransaction::decode_calldata(from, tx.input()) {
                        return Self::DexVm(dexvm_tx);
                    }
                }
            }
        }

        // Default route to EVM
        Self::Evm(tx)
    }

    /// Check if this is a DexVM transaction
    pub fn is_dexvm(&self) -> bool {
        matches!(self, Self::DexVm(_))
    }

    /// Check if this is an EVM transaction
    pub fn is_evm(&self) -> bool {
        matches!(self, Self::Evm(_))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_consensus::TxLegacy;
    use alloy_primitives::address;

    #[test]
    fn test_route_to_dexvm() {
        // Create a transaction sent to DexVM router address
        let mut calldata = vec![0u8]; // Increment
        calldata.extend_from_slice(&100u64.to_be_bytes());

        let tx = TransactionSigned::new_unhashed(
            TxLegacy {
                to: alloy_primitives::TxKind::Call(DEXVM_ROUTER_ADDRESS),
                input: calldata.into(),
                ..Default::default()
            }
            .into(),
            alloy_primitives::Signature::test_signature(),
        );

        let dual_tx = DualVmTransaction::from_ethereum_tx(tx);
        assert!(dual_tx.is_dexvm());
    }

    #[test]
    fn test_route_to_evm() {
        // Create a transaction sent to a normal address
        let tx = TransactionSigned::new_unhashed(
            TxLegacy {
                to: alloy_primitives::TxKind::Call(address!(
                    "1111111111111111111111111111111111111111"
                )),
                ..Default::default()
            }
            .into(),
            alloy_primitives::Signature::test_signature(),
        );

        let dual_tx = DualVmTransaction::from_ethereum_tx(tx);
        assert!(dual_tx.is_evm());
    }

    #[test]
    fn test_contract_creation_routes_to_evm() {
        // Contract creation transactions should route to EVM
        let tx = TransactionSigned::new_unhashed(
            TxLegacy { to: alloy_primitives::TxKind::Create, ..Default::default() }.into(),
            alloy_primitives::Signature::test_signature(),
        );

        let dual_tx = DualVmTransaction::from_ethereum_tx(tx);
        assert!(dual_tx.is_evm());
    }
}
