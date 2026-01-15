//! DualVM primitives
//!
//! Core primitive types for the dual VM system:
//! - Transaction types and routing logic
//! - DexVM receipt types
//! - Constants

pub mod receipt;
pub mod transaction;

pub use receipt::{DexVmExecutionResult, DexVmReceipt};
pub use transaction::{DexVmOperation, DexVmTransaction, DualVmTransaction, DEXVM_ROUTER_ADDRESS};
