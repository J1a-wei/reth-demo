//! EVM execution engine using revm
//!
//! This crate provides a complete EVM execution environment backed by MDBX storage.

pub mod db;
pub mod executor;
pub mod inspector;
pub mod precompiles;

pub use db::MdbxDatabase;
pub use executor::{EvmExecutor, ExecutionResult};
pub use inspector::ExecutionInspector;
pub use precompiles::DexPrecompiles;
