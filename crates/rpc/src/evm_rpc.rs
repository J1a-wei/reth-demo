//! EVM JSON-RPC service

use alloy_consensus::{transaction::SignerRecoverable, Transaction};
use alloy_primitives::{Address, Bytes, B256, B64, U256, U64};
use alloy_rlp::Decodable;
use dex_storage::{BlockStore, StateStore, StoredBlock};
use jsonrpsee::{
    core::RpcResult,
    proc_macros::rpc,
    server::{ServerBuilder, ServerHandle},
};
use tower_http::cors::{Any, CorsLayer};
use reth_ethereum_primitives::TransactionSigned;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, RwLock},
};
use tokio::sync::mpsc;

/// Transaction request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionRequest {
    pub from: Option<Address>,
    pub to: Option<Address>,
    pub gas: Option<U64>,
    #[serde(rename = "gasPrice")]
    pub gas_price: Option<U256>,
    pub value: Option<U256>,
    pub data: Option<Bytes>,
    pub nonce: Option<U64>,
}

/// Transaction receipt
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionReceipt {
    pub transaction_hash: B256,
    pub transaction_index: U64,
    pub block_hash: B256,
    pub block_number: U64,
    pub from: Address,
    pub to: Option<Address>,
    pub cumulative_gas_used: U64,
    pub gas_used: U64,
    pub contract_address: Option<Address>,
    pub logs: Vec<Log>,
    pub logs_bloom: Bytes,
    pub status: U64,
    #[serde(rename = "type")]
    pub tx_type: U64,
}

/// Log entry
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Log {
    pub address: Address,
    pub topics: Vec<B256>,
    pub data: Bytes,
    pub block_hash: B256,
    pub block_number: U64,
    pub transaction_hash: B256,
    pub transaction_index: U64,
    pub log_index: U64,
}

/// Block info - compatible with Ethereum RPC format
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockInfo {
    pub number: U64,
    pub hash: B256,
    pub parent_hash: B256,
    pub sha3_uncles: B256,
    pub logs_bloom: Bytes,
    pub transactions_root: B256,
    pub state_root: B256,
    pub receipts_root: B256,
    pub miner: Address,
    pub difficulty: U256,
    pub total_difficulty: U256,
    pub extra_data: Bytes,
    pub size: U64,
    pub gas_limit: U64,
    pub gas_used: U64,
    pub timestamp: U64,
    pub transactions: Vec<B256>,
    pub uncles: Vec<B256>,
    pub nonce: B64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub base_fee_per_gas: Option<U256>,
}

/// Empty uncles hash (keccak256 of RLP empty list)
const EMPTY_OMMER_ROOT: B256 = B256::new([
    0x1d, 0xcc, 0x4d, 0xe8, 0xde, 0xc7, 0x5d, 0x7a, 0xab, 0x85, 0xb5, 0x67, 0xb6, 0xcc, 0xd4, 0x1a,
    0xd3, 0x12, 0x45, 0x1b, 0x94, 0x8a, 0x74, 0x13, 0xf0, 0xa1, 0x42, 0xfd, 0x40, 0xd4, 0x93, 0x47,
]);

/// Empty transactions root (keccak256 of RLP empty list)
const EMPTY_TX_ROOT: B256 = B256::new([
    0x56, 0xe8, 0x1f, 0x17, 0x1b, 0xcc, 0x55, 0xa6, 0xff, 0x83, 0x45, 0xe6, 0x92, 0xc0, 0xf8, 0x6e,
    0x5b, 0x48, 0xe0, 0x1b, 0x99, 0x6c, 0xad, 0xc0, 0x01, 0x62, 0x2f, 0xb5, 0xe3, 0x63, 0xb4, 0x21,
]);

/// Empty receipts root (keccak256 of RLP empty list)
const EMPTY_RECEIPTS_ROOT: B256 = B256::new([
    0x56, 0xe8, 0x1f, 0x17, 0x1b, 0xcc, 0x55, 0xa6, 0xff, 0x83, 0x45, 0xe6, 0x92, 0xc0, 0xf8, 0x6e,
    0x5b, 0x48, 0xe0, 0x1b, 0x99, 0x6c, 0xad, 0xc0, 0x01, 0x62, 0x2f, 0xb5, 0xe3, 0x63, 0xb4, 0x21,
]);

impl From<StoredBlock> for BlockInfo {
    fn from(block: StoredBlock) -> Self {
        Self {
            number: U64::from(block.number),
            hash: block.hash,
            parent_hash: block.parent_hash,
            sha3_uncles: EMPTY_OMMER_ROOT,
            logs_bloom: Bytes::from(vec![0u8; 256]),
            transactions_root: if block.transaction_hashes.is_empty() {
                EMPTY_TX_ROOT
            } else {
                // Simplified: just use the state root as a placeholder
                block.combined_state_root
            },
            state_root: block.combined_state_root,
            receipts_root: EMPTY_RECEIPTS_ROOT,
            miner: block.miner,
            difficulty: U256::from(1),
            total_difficulty: U256::from(block.number + 1),
            extra_data: Bytes::default(),
            size: U64::from(1000), // Placeholder size
            gas_limit: U64::from(block.gas_limit),
            gas_used: U64::from(block.gas_used),
            timestamp: U64::from(block.timestamp),
            transactions: block.transaction_hashes,
            uncles: vec![],
            nonce: B64::ZERO,
            base_fee_per_gas: Some(U256::from(1_000_000_000u64)), // 1 gwei
        }
    }
}

/// EVM JSON-RPC interface
#[rpc(server, namespace = "eth")]
pub trait EthApi {
    #[method(name = "chainId")]
    async fn chain_id(&self) -> RpcResult<U64>;

    #[method(name = "blockNumber")]
    async fn block_number(&self) -> RpcResult<U64>;

    #[method(name = "getBalance")]
    async fn get_balance(&self, address: Address, block: Option<String>) -> RpcResult<U256>;

    #[method(name = "getTransactionCount")]
    async fn get_transaction_count(
        &self,
        address: Address,
        block: Option<String>,
    ) -> RpcResult<U64>;

    #[method(name = "getCode")]
    async fn get_code(&self, address: Address, block: Option<String>) -> RpcResult<Bytes>;

    #[method(name = "getStorageAt")]
    async fn get_storage_at(
        &self,
        address: Address,
        slot: U256,
        block: Option<String>,
    ) -> RpcResult<B256>;

    #[method(name = "sendRawTransaction")]
    async fn send_raw_transaction(&self, data: Bytes) -> RpcResult<B256>;

    #[method(name = "call")]
    async fn call(&self, request: TransactionRequest, block: Option<String>) -> RpcResult<Bytes>;

    #[method(name = "estimateGas")]
    async fn estimate_gas(
        &self,
        request: TransactionRequest,
        block: Option<String>,
    ) -> RpcResult<U64>;

    #[method(name = "gasPrice")]
    async fn gas_price(&self) -> RpcResult<U256>;

    #[method(name = "getBlockByNumber")]
    async fn get_block_by_number(
        &self,
        number: String,
        full_tx: bool,
    ) -> RpcResult<Option<BlockInfo>>;

    #[method(name = "getBlockByHash")]
    async fn get_block_by_hash(&self, hash: B256, full_tx: bool) -> RpcResult<Option<BlockInfo>>;

    #[method(name = "getTransactionReceipt")]
    async fn get_transaction_receipt(&self, hash: B256) -> RpcResult<Option<TransactionReceipt>>;

    #[method(name = "accounts")]
    async fn accounts(&self) -> RpcResult<Vec<Address>>;

    #[method(name = "net_version")]
    async fn net_version(&self) -> RpcResult<String>;
}

/// Web3 JSON-RPC interface
#[rpc(server, namespace = "web3")]
pub trait Web3Api {
    #[method(name = "clientVersion")]
    async fn client_version(&self) -> RpcResult<String>;
}

/// Net JSON-RPC interface
#[rpc(server, namespace = "net")]
pub trait NetApi {
    #[method(name = "version")]
    async fn version(&self) -> RpcResult<String>;

    #[method(name = "listening")]
    async fn listening(&self) -> RpcResult<bool>;

    #[method(name = "peerCount")]
    async fn peer_count(&self) -> RpcResult<U64>;
}

/// Pending transaction
#[derive(Debug, Clone)]
pub struct PendingTransaction {
    pub tx: TransactionSigned,
    pub hash: B256,
    pub from: Address,
}

/// EVM RPC server implementation
pub struct EvmRpcServer {
    chain_id: u64,
    state_store: Arc<StateStore>,
    block_store: Arc<BlockStore>,
    pending_txs: Arc<RwLock<Vec<PendingTransaction>>>,
    receipts: Arc<RwLock<HashMap<B256, TransactionReceipt>>>,
    /// Optional channel for broadcasting transactions via P2P
    tx_broadcast_sender: Arc<RwLock<Option<mpsc::Sender<Vec<u8>>>>>,
}

impl EvmRpcServer {
    pub fn new(chain_id: u64, state_store: Arc<StateStore>, block_store: Arc<BlockStore>) -> Self {
        Self {
            chain_id,
            state_store,
            block_store,
            pending_txs: Arc::new(RwLock::new(Vec::new())),
            receipts: Arc::new(RwLock::new(HashMap::new())),
            tx_broadcast_sender: Arc::new(RwLock::new(None)),
        }
    }

    /// Set the transaction broadcast channel for P2P propagation
    pub fn set_tx_broadcast_sender(&self, sender: mpsc::Sender<Vec<u8>>) {
        *self.tx_broadcast_sender.write().unwrap() = Some(sender);
    }

    /// Broadcast a transaction via P2P (if sender is configured)
    fn broadcast_transaction(&self, tx_rlp: Vec<u8>) {
        if let Some(sender) = self.tx_broadcast_sender.read().unwrap().as_ref() {
            // Use try_send to avoid blocking - if the channel is full, we'll skip
            let _ = sender.try_send(tx_rlp);
        }
    }

    pub fn get_pending_transactions(&self) -> Vec<PendingTransaction> {
        self.pending_txs.read().unwrap().clone()
    }

    pub fn clear_pending_transactions(&self) {
        self.pending_txs.write().unwrap().clear();
    }

    pub fn add_receipt(&self, hash: B256, receipt: TransactionReceipt) {
        self.receipts.write().unwrap().insert(hash, receipt);
    }

    /// Add a pending transaction from P2P (without validation)
    /// Returns true if the transaction was added, false if it already exists
    pub fn add_pending_transaction_from_p2p(&self, tx: TransactionSigned) -> bool {
        let hash = *tx.tx_hash();
        let mut pending = self.pending_txs.write().unwrap();

        // Check if transaction already exists
        if pending.iter().any(|p| p.hash == hash) {
            return false;
        }

        // Recover sender address
        let from = match tx.recover_signer() {
            Ok(addr) => addr,
            Err(_) => return false,
        };

        pending.push(PendingTransaction { tx, hash, from });
        true
    }
}

#[async_trait::async_trait]
impl EthApiServer for EvmRpcServer {
    async fn chain_id(&self) -> RpcResult<U64> {
        Ok(U64::from(self.chain_id))
    }

    async fn block_number(&self) -> RpcResult<U64> {
        Ok(U64::from(self.block_store.latest_block_number()))
    }

    async fn get_balance(&self, address: Address, _block: Option<String>) -> RpcResult<U256> {
        Ok(self.state_store.get_balance(&address))
    }

    async fn get_transaction_count(
        &self,
        address: Address,
        _block: Option<String>,
    ) -> RpcResult<U64> {
        Ok(U64::from(self.state_store.get_nonce(&address)))
    }

    async fn get_code(&self, address: Address, _block: Option<String>) -> RpcResult<Bytes> {
        Ok(self.state_store.get_code(&address).unwrap_or_default())
    }

    async fn get_storage_at(
        &self,
        address: Address,
        slot: U256,
        _block: Option<String>,
    ) -> RpcResult<B256> {
        let value = self.state_store.get_storage(&address, slot);
        Ok(B256::from(value.to_be_bytes()))
    }

    async fn send_raw_transaction(&self, data: Bytes) -> RpcResult<B256> {
        let tx = TransactionSigned::decode(&mut data.as_ref()).map_err(|e| {
            jsonrpsee::types::ErrorObjectOwned::owned(
                -32000,
                format!("Failed to decode transaction: {}", e),
                None::<()>,
            )
        })?;

        let tx_hash = *tx.tx_hash();

        let caller = tx.recover_signer().map_err(|e| {
            jsonrpsee::types::ErrorObjectOwned::owned(
                -32000,
                format!("Failed to recover signer: {}", e),
                None::<()>,
            )
        })?;

        // Basic validation (don't execute yet - execution happens during block production)
        let caller_balance = self.state_store.get_balance(&caller);
        let caller_nonce = self.state_store.get_nonce(&caller);

        tracing::info!(
            "Received transaction {} from {}: nonce={}, balance={}, tx_nonce={}, value={}, gas_limit={}, gas_price={}",
            tx_hash, caller, caller_nonce, caller_balance, tx.nonce(), tx.value(), tx.gas_limit(), tx.effective_gas_price(None)
        );

        // Check nonce
        if tx.nonce() < caller_nonce {
            return Err(jsonrpsee::types::ErrorObjectOwned::owned(
                -32000,
                format!("Nonce too low: expected {}, got {}", caller_nonce, tx.nonce()),
                None::<()>,
            ));
        }

        // Check balance (rough estimate)
        let tx_value = tx.value();
        let gas_price = U256::from(tx.effective_gas_price(None));
        let gas_limit = tx.gas_limit();
        let max_gas_cost = gas_price * U256::from(gas_limit);
        let total_cost = tx_value + max_gas_cost;

        if caller_balance < total_cost {
            return Err(jsonrpsee::types::ErrorObjectOwned::owned(
                -32000,
                format!("Insufficient balance: have {}, need {}", caller_balance, total_cost),
                None::<()>,
            ));
        }

        // Add to pending transactions (will be executed during block production)
        self.pending_txs.write().unwrap().push(PendingTransaction { tx, hash: tx_hash, from: caller });

        // Broadcast transaction to P2P network (for fullnode mode)
        self.broadcast_transaction(data.to_vec());

        tracing::info!(
            "Transaction {} added to mempool from {}",
            tx_hash,
            caller
        );

        Ok(tx_hash)
    }

    async fn call(&self, _request: TransactionRequest, _block: Option<String>) -> RpcResult<Bytes> {
        Ok(Bytes::default())
    }

    async fn estimate_gas(
        &self,
        request: TransactionRequest,
        _block: Option<String>,
    ) -> RpcResult<U64> {
        let mut gas = 21000u64;
        if let Some(data) = &request.data {
            gas += data.len() as u64 * 16;
        }
        if request.to.is_none() {
            gas += 32000;
            if let Some(data) = &request.data {
                gas += data.len() as u64 * 200;
            }
        }
        Ok(U64::from((gas as f64 * 1.2) as u64))
    }

    async fn gas_price(&self) -> RpcResult<U256> {
        Ok(U256::from(1_000_000_000u64))
    }

    async fn get_block_by_number(
        &self,
        number: String,
        _full_tx: bool,
    ) -> RpcResult<Option<BlockInfo>> {
        let block_num = if number == "latest" || number == "pending" {
            self.block_store.latest_block_number()
        } else if number == "earliest" {
            0
        } else {
            let num_str = number.strip_prefix("0x").unwrap_or(&number);
            u64::from_str_radix(num_str, 16).unwrap_or(0)
        };

        Ok(self.block_store.get_block_by_number(block_num).map(BlockInfo::from))
    }

    async fn get_block_by_hash(&self, hash: B256, _full_tx: bool) -> RpcResult<Option<BlockInfo>> {
        Ok(self.block_store.get_block_by_hash(hash).map(BlockInfo::from))
    }

    async fn get_transaction_receipt(&self, hash: B256) -> RpcResult<Option<TransactionReceipt>> {
        Ok(self.receipts.read().unwrap().get(&hash).cloned())
    }

    async fn accounts(&self) -> RpcResult<Vec<Address>> {
        let accounts = self.state_store.all_accounts();
        Ok(accounts.keys().cloned().collect())
    }

    async fn net_version(&self) -> RpcResult<String> {
        Ok(self.chain_id.to_string())
    }
}

#[async_trait::async_trait]
impl Web3ApiServer for EvmRpcServer {
    async fn client_version(&self) -> RpcResult<String> {
        Ok("DualVM/v0.1.0".to_string())
    }
}

#[async_trait::async_trait]
impl NetApiServer for EvmRpcServer {
    async fn version(&self) -> RpcResult<String> {
        Ok(self.chain_id.to_string())
    }

    async fn listening(&self) -> RpcResult<bool> {
        Ok(true)
    }

    async fn peer_count(&self) -> RpcResult<U64> {
        Ok(U64::from(0))
    }
}

/// Start EVM RPC server
pub async fn start_evm_rpc_server(
    chain_id: u64,
    state_store: Arc<StateStore>,
    block_store: Arc<BlockStore>,
    port: u16,
) -> eyre::Result<(ServerHandle, Arc<EvmRpcServer>)> {
    let server = EvmRpcServer::new(chain_id, state_store, block_store);
    let server = Arc::new(server);

    let addr: SocketAddr = format!("0.0.0.0:{}", port).parse()?;

    // Configure CORS to allow any origin (for browser wallet compatibility)
    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let server_builder = ServerBuilder::default()
        .set_http_middleware(tower::ServiceBuilder::new().layer(cors))
        .build(addr)
        .await?;

    let server_clone = Arc::clone(&server);
    let rpc_module = {
        let mut module = jsonrpsee::RpcModule::new(());
        module.merge(EthApiServer::into_rpc(server_clone.as_ref().clone()))?;
        module.merge(Web3ApiServer::into_rpc(server_clone.as_ref().clone()))?;
        module.merge(NetApiServer::into_rpc(server_clone.as_ref().clone()))?;
        module
    };

    let handle = server_builder.start(rpc_module);

    tracing::info!("EVM JSON-RPC server listening on {}", addr);

    Ok((handle, server))
}

impl Clone for EvmRpcServer {
    fn clone(&self) -> Self {
        Self {
            chain_id: self.chain_id,
            state_store: Arc::clone(&self.state_store),
            block_store: Arc::clone(&self.block_store),
            pending_txs: Arc::clone(&self.pending_txs),
            receipts: Arc::clone(&self.receipts),
            tx_broadcast_sender: Arc::clone(&self.tx_broadcast_sender),
        }
    }
}
