//! EVM JSON-RPC service

use alloy_consensus::{transaction::SignerRecoverable, Transaction};
use alloy_primitives::{Address, Bytes, B256, U256, U64};
use alloy_rlp::Decodable;
use dex_storage::{BlockStore, StateStore, StoredBlock};
use jsonrpsee::{
    core::RpcResult,
    proc_macros::rpc,
    server::{ServerBuilder, ServerHandle},
};
use reth_ethereum_primitives::TransactionSigned;
use serde::{Deserialize, Serialize};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::{Arc, RwLock},
};

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
    pub status: U64,
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

/// Block info
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BlockInfo {
    pub number: U64,
    pub hash: B256,
    pub parent_hash: B256,
    pub timestamp: U64,
    pub gas_limit: U64,
    pub gas_used: U64,
    pub miner: Address,
    pub state_root: B256,
    pub transactions: Vec<B256>,
}

impl From<StoredBlock> for BlockInfo {
    fn from(block: StoredBlock) -> Self {
        Self {
            number: U64::from(block.number),
            hash: block.hash,
            parent_hash: block.parent_hash,
            timestamp: U64::from(block.timestamp),
            gas_limit: U64::from(block.gas_limit),
            gas_used: U64::from(block.gas_used),
            miner: block.miner,
            state_root: block.combined_state_root,
            transactions: block.transaction_hashes,
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
}

impl EvmRpcServer {
    pub fn new(chain_id: u64, state_store: Arc<StateStore>, block_store: Arc<BlockStore>) -> Self {
        Self {
            chain_id,
            state_store,
            block_store,
            pending_txs: Arc::new(RwLock::new(Vec::new())),
            receipts: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    fn execute_simple_tx(
        &self,
        tx: &TransactionSigned,
        caller: Address,
    ) -> eyre::Result<(bool, u64, Option<Address>)> {
        let caller_balance = self.state_store.get_balance(&caller);
        let caller_nonce = self.state_store.get_nonce(&caller);

        if tx.nonce() < caller_nonce {
            return Err(eyre::eyre!("nonce too low"));
        }

        let tx_value = tx.value();
        let gas_price = U256::from(tx.effective_gas_price(None));
        let gas_limit = tx.gas_limit();
        let max_gas_cost = gas_price * U256::from(gas_limit);
        let total_cost = tx_value + max_gas_cost;

        if caller_balance < total_cost {
            return Err(eyre::eyre!("insufficient balance"));
        }

        let base_gas = 21000u64;
        let data_gas = tx.input().len() as u64 * 16;
        let mut gas_used = base_gas + data_gas;

        let contract_address = match tx.to() {
            Some(to) => {
                let to_balance = self.state_store.get_balance(&to);
                let _ = self.state_store.set_balance(to, to_balance + tx_value);

                if self.state_store.get_code(&to).is_some() {
                    gas_used += 10000;
                }
                None
            }
            None => {
                use alloy_primitives::keccak256;

                let mut data = Vec::new();
                data.extend_from_slice(caller.as_slice());
                data.extend_from_slice(&caller_nonce.to_be_bytes());
                let contract_addr = Address::from_slice(&keccak256(&data)[12..]);

                let code = tx.input().clone();
                gas_used += code.len() as u64 * 200;
                let _ = self.state_store.set_code(contract_addr, code);
                let _ = self.state_store.set_balance(contract_addr, tx_value);

                Some(contract_addr)
            }
        };

        let gas_cost = gas_price * U256::from(gas_used);
        let _ = self.state_store.set_balance(caller, caller_balance - tx_value - gas_cost);
        let _ = self.state_store.increment_nonce(caller);

        Ok((true, gas_used, contract_address))
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

        tracing::info!("Received transaction {} from {}", tx_hash, caller);

        let (success, gas_used, contract_address) =
            self.execute_simple_tx(&tx, caller).map_err(|e| {
                jsonrpsee::types::ErrorObjectOwned::owned(
                    -32000,
                    format!("Transaction execution failed: {}", e),
                    None::<()>,
                )
            })?;

        let receipt = TransactionReceipt {
            transaction_hash: tx_hash,
            transaction_index: U64::from(0),
            block_hash: B256::ZERO,
            block_number: U64::from(self.block_store.latest_block_number() + 1),
            from: caller,
            to: tx.to(),
            cumulative_gas_used: U64::from(gas_used),
            gas_used: U64::from(gas_used),
            contract_address,
            logs: vec![],
            status: if success { U64::from(1) } else { U64::from(0) },
        };

        self.add_receipt(tx_hash, receipt);

        self.pending_txs.write().unwrap().push(PendingTransaction { tx, hash: tx_hash, from: caller });

        tracing::info!(
            "Transaction {} executed: success={}, gas_used={}",
            tx_hash,
            success,
            gas_used
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

    let server_builder = ServerBuilder::default().build(addr).await?;

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
        }
    }
}
