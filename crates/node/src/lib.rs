//! DualVM node implementation
//!
//! This crate provides the complete dual VM node:
//! - Dual VM executor: coordinates EVM and DexVM execution
//! - Node type: integrates all components
//! - RPC services: DexVM REST API (9845) + EVM JSON-RPC (8545)
//! - POA consensus: simple single-validator consensus

pub mod consensus;
pub mod evm_executor;
pub mod executor;
pub mod node;

pub use consensus::{BlockProposal, PoaConfig, PoaConsensus};
pub use evm_executor::SimpleEvmExecutor;
pub use executor::{DualVmExecutionResult, DualVmExecutor};
pub use node::{DualVmNode, NodeConfig};
