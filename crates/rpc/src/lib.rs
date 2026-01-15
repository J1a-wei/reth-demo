//! DualVM RPC module
//!
//! This crate provides RPC interfaces:
//! - DexVM REST API (port 9845): Counter operations
//! - EVM JSON-RPC (port 8545): Ethereum-compatible RPC

pub mod api;
pub mod evm_rpc;

pub use api::{
    CounterResponse, DecrementRequest, DexVmApi, HealthResponse, IncrementRequest,
    OperationResponse, StateRootResponse,
};

pub use evm_rpc::{
    start_evm_rpc_server, BlockInfo, EvmRpcServer, Log, PendingTransaction, TransactionReceipt,
    TransactionRequest,
};
