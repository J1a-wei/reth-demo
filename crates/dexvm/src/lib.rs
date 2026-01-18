//! DexVM implementation
//!
//! A simple counter-based virtual machine for the dual VM system.

pub mod executor;
pub mod precompiles;
pub mod state;

pub use executor::DexVmExecutor;
pub use precompiles::{
    PrecompileExecutor, PrecompileOperation, PrecompileResult, COUNTER_PRECOMPILE_ADDRESS,
    OP_DECREMENT, OP_INCREMENT, OP_QUERY,
};
pub use state::DexVmState;

// Re-export transaction types for convenience
pub use dex_primitives::{DexVmOperation, DexVmTransaction};
