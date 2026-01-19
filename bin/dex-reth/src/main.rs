//! dex-reth node binary
//!
//! A dual virtual machine blockchain node with EVM and DexVM support.

use alloy_consensus::Header as ConsensusHeader;
use alloy_primitives::{hex, keccak256, Address, Bloom, B256, B64, U256};
use alloy_rlp::Decodable;
use clap::Parser;
use dex_node::{DualVmNode, PoaConfig};
use dex_p2p::{P2pConfig, P2pEvent, P2pHandle, P2pService, HashOrNumber, PeerId, SessionCommand};
use dex_rpc::EvmRpcServer;
use dex_storage::{BlockStore, StoredBlock};
use reth_ethereum_primitives::{BlockBody, TransactionSigned};
use reth_network_peers::TrustedPeer;
use serde::Deserialize;
use std::{collections::{HashMap, HashSet}, path::PathBuf, sync::Arc, time::Duration};
use tokio::sync::RwLock;

/// dex-reth node command line arguments
#[derive(Debug, Parser)]
#[clap(name = "dex-reth", about = "dex-reth - Dual Virtual Machine Node")]
struct Cli {
    /// EVM JSON-RPC port
    #[clap(long, default_value = "8545")]
    evm_rpc_port: u16,

    /// DexVM REST API port
    #[clap(long, default_value = "9845")]
    dexvm_port: u16,

    /// P2P listen port
    #[clap(long, default_value = "30303")]
    p2p_port: u16,

    /// Disable P2P networking (P2P is enabled by default)
    #[clap(long, default_value = "false")]
    disable_p2p: bool,

    /// Boot nodes (enode URLs)
    #[clap(long)]
    bootnodes: Vec<String>,

    /// Log level
    #[clap(long, default_value = "info")]
    log_level: String,

    /// Genesis file path
    #[clap(long)]
    genesis: Option<PathBuf>,

    /// Enable POA consensus
    #[clap(long)]
    enable_consensus: bool,

    /// Validator private key (hex string, with or without 0x prefix)
    /// Default is Hardhat's first test account key (0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266)
    #[clap(long, default_value = "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80")]
    validator_key: String,

    /// Block interval (milliseconds)
    #[clap(long, default_value = "500")]
    block_interval_ms: u64,

    /// Data directory
    #[clap(long, default_value = "./data")]
    datadir: PathBuf,

    /// Maximum number of P2P peers
    #[clap(long, default_value = "50")]
    max_peers: usize,
}

/// Genesis file format
#[derive(Debug, Deserialize)]
struct GenesisFile {
    config: GenesisConfig,
    alloc: HashMap<Address, AccountAlloc>,
}

#[derive(Debug, Deserialize)]
struct GenesisConfig {
    #[serde(rename = "chainId")]
    chain_id: u64,
}

#[derive(Debug, Deserialize)]
struct AccountAlloc {
    balance: String,
}

/// Block sync manager for fullnode mode
struct BlockSyncManager {
    /// P2P handle for sending requests
    p2p_handle: P2pHandle,
    /// Block store for checking/storing blocks
    block_store: Arc<BlockStore>,
    /// Blocks we're currently requesting headers for
    pending_header_requests: HashSet<u64>,
    /// Headers received, waiting for bodies (block_number -> header)
    pending_body_requests: HashMap<u64, ConsensusHeader>,
    /// Track which peer we requested from (for bodies)
    request_peer: Option<PeerId>,
    /// Track known peer head heights for active sync
    peer_heads: HashMap<PeerId, u64>,
}

impl BlockSyncManager {
    fn new(p2p_handle: P2pHandle, block_store: Arc<BlockStore>) -> Self {
        Self {
            p2p_handle,
            block_store,
            pending_header_requests: HashSet::new(),
            pending_body_requests: HashMap::new(),
            request_peer: None,
            peer_heads: HashMap::new(),
        }
    }

    /// Request initial sync from a peer when connected
    async fn request_initial_sync(&mut self, peer_id: PeerId) {
        let our_latest = self.block_store.latest_block_number();

        // Request headers starting from our latest block + 1
        // Use a larger batch size for initial sync (up to 512 headers)
        let start_block = our_latest + 1;
        let count = 512u64; // Request more headers at once

        // Only request if we don't have pending requests
        if !self.pending_header_requests.is_empty() {
            tracing::debug!("Skipping initial sync request, already have pending requests");
            return;
        }

        tracing::info!(
            "Requesting initial sync from peer {}: starting at block {}",
            peer_id, start_block
        );

        // Mark blocks as pending
        for block_num in start_block..start_block + count {
            self.pending_header_requests.insert(block_num);
        }
        self.request_peer = Some(peer_id);

        let cmd = SessionCommand::GetBlockHeaders {
            peer_id,
            start: start_block,
            count,
        };
        if let Err(e) = self.p2p_handle.send_command(cmd).await {
            tracing::warn!("Failed to send initial sync request: {}", e);
            // Clear pending on error
            self.pending_header_requests.clear();
        }
    }

    /// Handle NewBlockHash event - request headers if we don't have the block
    async fn handle_new_block_hash(&mut self, peer_id: PeerId, _hash: B256, number: u64) {
        // Track the peer's head height
        self.peer_heads.insert(peer_id, number);

        // Check if we already have this block
        if self.block_store.get_block_by_number(number).is_some() {
            tracing::debug!("Already have block {}, skipping sync", number);
            return;
        }

        // Check if we're already requesting this block
        if self.pending_header_requests.contains(&number) || self.pending_body_requests.contains_key(&number) {
            tracing::debug!("Already requesting block {}, skipping", number);
            return;
        }

        // Request headers for missing blocks (up to 512 at a time)
        let our_latest = self.block_store.latest_block_number();
        let start_block = our_latest + 1;
        let mut count = number - our_latest;

        // Limit batch size
        if count > 512 {
            count = 512;
        }

        if count > 0 {
            tracing::info!(
                "Requesting {} block headers from peer {} (blocks {} to {})",
                count, peer_id, start_block, start_block + count - 1
            );

            // Track pending requests
            for block_num in start_block..start_block + count {
                self.pending_header_requests.insert(block_num);
            }
            self.request_peer = Some(peer_id);

            // Send request
            let cmd = SessionCommand::GetBlockHeaders {
                peer_id,
                start: start_block,
                count,
            };
            if let Err(e) = self.p2p_handle.send_command(cmd).await {
                tracing::warn!("Failed to send GetBlockHeaders: {}", e);
                // Clear pending on error
                for block_num in start_block..start_block + count {
                    self.pending_header_requests.remove(&block_num);
                }
            }
        }
    }

    /// Handle BlockHeaders response - store headers and request bodies
    async fn handle_block_headers(&mut self, peer_id: PeerId, headers: Vec<ConsensusHeader>) {
        if headers.is_empty() {
            tracing::debug!("Received empty headers response from {}", peer_id);
            // Clear pending requests since we got an empty response
            self.pending_header_requests.clear();
            return;
        }

        tracing::info!("Received {} block headers from peer {}", headers.len(), peer_id);

        // Collect hashes for body requests
        let mut hashes_to_request: Vec<B256> = Vec::new();

        for header in headers {
            let block_num = header.number;

            // Remove from pending header requests
            self.pending_header_requests.remove(&block_num);

            // Compute header hash
            let header_hash = keccak256(alloy_rlp::encode(&header));

            tracing::debug!(
                "Received header for block {}: hash={:?}, parent={:?}",
                block_num, header_hash, header.parent_hash
            );

            // Store header and add to body request queue
            hashes_to_request.push(header_hash);
            self.pending_body_requests.insert(block_num, header);
        }

        // Clear any remaining pending header requests (for blocks we didn't receive)
        self.pending_header_requests.clear();

        // Request bodies for all headers
        if !hashes_to_request.is_empty() {
            tracing::info!("Requesting {} block bodies from peer {}", hashes_to_request.len(), peer_id);

            let cmd = SessionCommand::GetBlockBodies {
                peer_id,
                hashes: hashes_to_request,
            };
            if let Err(e) = self.p2p_handle.send_command(cmd).await {
                tracing::warn!("Failed to send GetBlockBodies: {}", e);
            }
        }
    }

    /// Handle BlockBodies response - create and store complete blocks
    async fn handle_block_bodies(&mut self, peer_id: PeerId, bodies: Vec<BlockBody>) {
        if bodies.is_empty() {
            tracing::debug!("Received empty bodies response");
            return;
        }

        tracing::info!("Received {} block bodies", bodies.len());

        // Match bodies with pending headers
        // Bodies come in the same order as requested hashes
        let mut pending_numbers: Vec<u64> = self.pending_body_requests.keys().copied().collect();
        pending_numbers.sort();

        for (i, body) in bodies.into_iter().enumerate() {
            if i >= pending_numbers.len() {
                tracing::warn!("Received more bodies than pending headers");
                break;
            }

            let block_num = pending_numbers[i];

            if let Some(header) = self.pending_body_requests.remove(&block_num) {
                // Create StoredBlock from header and body
                let header_hash = keccak256(alloy_rlp::encode(&header));

                // Extract transaction hashes and prepare for storage
                let tx_hashes: Vec<B256> = body.transactions.iter()
                    .map(|tx| *tx.tx_hash())
                    .collect();

                // Store full transactions
                let tx_data: Vec<(B256, Vec<u8>)> = body.transactions.iter()
                    .map(|tx| (*tx.tx_hash(), alloy_rlp::encode(tx)))
                    .collect();

                if !tx_data.is_empty() {
                    if let Err(e) = self.block_store.store_transactions(&tx_data) {
                        tracing::error!("Failed to store transactions for block {}: {}", block_num, e);
                    }
                }

                // Extract signature from extra_data if present (65 bytes)
                let signature = if header.extra_data.len() >= 65 {
                    let mut sig = [0u8; 65];
                    sig.copy_from_slice(&header.extra_data[header.extra_data.len() - 65..]);
                    sig
                } else {
                    [0u8; 65]
                };

                let stored_block = StoredBlock {
                    number: header.number,
                    hash: header_hash,
                    parent_hash: header.parent_hash,
                    timestamp: header.timestamp,
                    gas_limit: header.gas_limit,
                    gas_used: header.gas_used,
                    miner: header.beneficiary,
                    // For sync, use header's state_root as combined (we don't have separate roots)
                    evm_state_root: header.state_root,
                    dexvm_state_root: B256::ZERO,
                    combined_state_root: header.state_root,
                    transaction_hashes: tx_hashes.clone(),
                    transaction_count: tx_data.len() as u64,
                    signature,
                };

                // Store the block
                match self.block_store.store_block(stored_block) {
                    Ok(_) => {
                        tracing::info!(
                            "Synced block {}: hash={:?}, txs={}",
                            block_num, header_hash, tx_hashes.len()
                        );
                    }
                    Err(e) => {
                        tracing::error!("Failed to store synced block {}: {}", block_num, e);
                    }
                }
            } else {
                tracing::warn!("Received body for unknown block {}", block_num);
            }
        }

        // Log sync progress
        let latest = self.block_store.latest_block_number();
        tracing::info!("Sync progress: latest block = {}", latest);

        // Continue sync if peer has more blocks
        if let Some(&peer_head) = self.peer_heads.get(&peer_id) {
            if latest < peer_head && self.pending_header_requests.is_empty() && self.pending_body_requests.is_empty() {
                tracing::info!(
                    "Continuing sync: our latest={}, peer head={}",
                    latest, peer_head
                );
                self.handle_new_block_hash(peer_id, B256::ZERO, peer_head).await;
            }
        }
    }
}

/// Run fullnode sync loop
async fn run_fullnode_sync(
    p2p_handle: P2pHandle,
    block_store: Arc<BlockStore>,
) -> eyre::Result<()> {
    let mut sync_manager = BlockSyncManager::new(p2p_handle.clone(), block_store);
    let mut events = p2p_handle.subscribe();

    tracing::info!("Starting fullnode sync handler");

    loop {
        match events.recv().await {
            Ok(event) => match event {
                P2pEvent::PeerConnected { peer_id, addr } => {
                    tracing::info!("Peer connected: {} from {}", peer_id, addr);
                    // Request initial sync from the connected peer
                    sync_manager.request_initial_sync(peer_id).await;
                }
                P2pEvent::PeerDisconnected { peer_id } => {
                    tracing::info!("Peer disconnected: {}", peer_id);
                    sync_manager.peer_heads.remove(&peer_id);
                }
                P2pEvent::NewBlockHash { peer_id, hash, number } => {
                    tracing::info!(
                        "Received NewBlockHash from {}: block {} hash {:?}",
                        peer_id, number, hash
                    );
                    sync_manager.handle_new_block_hash(peer_id, hash, number).await;
                }
                P2pEvent::BlockHeaders { peer_id, request_id: _, headers } => {
                    sync_manager.handle_block_headers(peer_id, headers).await;
                }
                P2pEvent::BlockBodies { peer_id, request_id: _, bodies } => {
                    sync_manager.handle_block_bodies(peer_id, bodies).await;
                }
                _ => {}
            },
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("P2P event receiver lagged {} events", n);
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                tracing::info!("P2P event channel closed");
                break;
            }
        }
    }

    Ok(())
}

/// Run validator P2P event handler - responds to block header/body requests
async fn run_validator_p2p_handler(
    p2p_handle: P2pHandle,
    block_store: Arc<BlockStore>,
    evm_rpc_server: Option<Arc<EvmRpcServer>>,
) -> eyre::Result<()> {
    let mut events = p2p_handle.subscribe();

    tracing::info!("Starting validator P2P event handler");

    loop {
        match events.recv().await {
            Ok(event) => match event {
                P2pEvent::PeerConnected { peer_id, addr } => {
                    tracing::info!("Peer connected: {} from {}", peer_id, addr);
                }
                P2pEvent::PeerDisconnected { peer_id } => {
                    tracing::info!("Peer disconnected: {}", peer_id);
                }
                P2pEvent::NewBlockHash { peer_id, hash, number } => {
                    tracing::debug!(
                        "Received NewBlockHash from {}: block {} hash {:?}",
                        peer_id, number, hash
                    );
                    // Validator doesn't need to sync - it produces blocks
                }
                P2pEvent::GetBlockHeadersRequest { peer_id, request_id, start, limit } => {
                    tracing::info!(
                        "Peer {} requesting {} headers starting from {:?}",
                        peer_id, limit, start
                    );

                    // Look up requested headers from our block store
                    let mut headers: Vec<ConsensusHeader> = Vec::new();

                    let start_num = match start {
                        HashOrNumber::Number(n) => n,
                        HashOrNumber::Hash(hash) => {
                            // Try to find block by hash
                            // For now, we don't support hash-based lookup
                            tracing::warn!("Hash-based header lookup not implemented, hash={:?}", hash);
                            continue;
                        }
                    };

                    // Collect headers (going backwards from start, as per ETH protocol)
                    for i in 0..limit {
                        let block_num = if start_num >= i { start_num - i } else { break };

                        if let Some(block) = block_store.get_block_by_number(block_num) {
                            // Include signature in extra_data (65 bytes at the end)
                            let extra_data = alloy_primitives::Bytes::copy_from_slice(&block.signature);

                            // Convert StoredBlock to ConsensusHeader
                            let header = ConsensusHeader {
                                parent_hash: block.parent_hash,
                                ommers_hash: keccak256([0x80]), // RLP empty list
                                beneficiary: block.miner,
                                state_root: block.combined_state_root,
                                transactions_root: keccak256([0x80]), // Empty trie root
                                receipts_root: keccak256([0x80]),
                                logs_bloom: Bloom::ZERO,
                                difficulty: U256::ZERO,
                                number: block.number,
                                gas_limit: block.gas_limit,
                                gas_used: block.gas_used,
                                timestamp: block.timestamp,
                                extra_data,
                                mix_hash: B256::ZERO,
                                nonce: B64::ZERO,
                                base_fee_per_gas: Some(0),
                                withdrawals_root: None,
                                blob_gas_used: None,
                                excess_blob_gas: None,
                                parent_beacon_block_root: None,
                                requests_hash: None,
                            };
                            headers.push(header);
                        } else {
                            // No more blocks
                            break;
                        }
                    }

                    if !headers.is_empty() {
                        tracing::info!("Sending {} headers to peer {}", headers.len(), peer_id);
                        let cmd = SessionCommand::SendBlockHeaders {
                            peer_id,
                            request_id,
                            headers,
                        };
                        if let Err(e) = p2p_handle.send_command(cmd).await {
                            tracing::warn!("Failed to send headers to peer {}: {}", peer_id, e);
                        }
                    } else {
                        tracing::debug!("No headers found for request from peer {}", peer_id);
                    }
                }
                P2pEvent::GetBlockBodiesRequest { peer_id, request_id, hashes } => {
                    tracing::info!(
                        "Peer {} requesting {} block bodies",
                        peer_id, hashes.len()
                    );

                    // Look up transactions for each requested block hash
                    let mut bodies: Vec<BlockBody> = Vec::with_capacity(hashes.len());

                    for block_hash in &hashes {
                        // Find the block by hash
                        if let Some(block) = block_store.get_block_by_hash(*block_hash) {
                            // Get full transactions from storage
                            let mut transactions = Vec::new();
                            for tx_hash in &block.transaction_hashes {
                                if let Some(tx_rlp) = block_store.get_transaction(*tx_hash) {
                                    // Decode the transaction
                                    if let Ok(tx) = TransactionSigned::decode(&mut tx_rlp.as_slice()) {
                                        transactions.push(tx);
                                    } else {
                                        tracing::warn!("Failed to decode transaction {:?}", tx_hash);
                                    }
                                }
                            }

                            tracing::debug!(
                                "Block {} has {} transactions",
                                block.number, transactions.len()
                            );

                            bodies.push(BlockBody {
                                transactions,
                                ommers: vec![],
                                withdrawals: None,
                            });
                        } else {
                            // Block not found, send empty body
                            tracing::debug!("Block {:?} not found", block_hash);
                            bodies.push(BlockBody {
                                transactions: vec![],
                                ommers: vec![],
                                withdrawals: None,
                            });
                        }
                    }

                    let total_txs: usize = bodies.iter().map(|b| b.transactions.len()).sum();
                    tracing::info!(
                        "Sending {} bodies with {} total transactions to peer {}",
                        bodies.len(), total_txs, peer_id
                    );
                    let cmd = SessionCommand::SendBlockBodies {
                        peer_id,
                        request_id,
                        bodies,
                    };
                    if let Err(e) = p2p_handle.send_command(cmd).await {
                        tracing::warn!("Failed to send bodies to peer {}: {}", peer_id, e);
                    }
                }
                P2pEvent::Transactions { peer_id, transactions } => {
                    tracing::info!(
                        "Received {} transactions from peer {}",
                        transactions.len(), peer_id
                    );

                    // Add transactions to the pending pool
                    if let Some(ref rpc_server) = evm_rpc_server {
                        let mut added = 0;
                        for tx_rlp in transactions {
                            let decode_result: Result<TransactionSigned, _> = TransactionSigned::decode(&mut tx_rlp.as_slice());
                            if let Ok(tx) = decode_result {
                                if rpc_server.add_pending_transaction_from_p2p(tx) {
                                    added += 1;
                                }
                            }
                        }
                        if added > 0 {
                            tracing::info!("Added {} transactions to mempool from peer {}", added, peer_id);
                        }
                    }
                }
                _ => {}
            },
            Err(tokio::sync::broadcast::error::RecvError::Lagged(n)) => {
                tracing::warn!("P2P event receiver lagged {} events", n);
            }
            Err(tokio::sync::broadcast::error::RecvError::Closed) => {
                tracing::info!("P2P event channel closed");
                break;
            }
        }
    }

    Ok(())
}

/// Run consensus loop with P2P block broadcasting
async fn run_consensus_loop_with_p2p(
    mut node: DualVmNode,
    p2p_handle: Option<P2pHandle>,
    last_broadcast_block: Arc<RwLock<u64>>,
) -> eyre::Result<()> {
    // Verify consensus is configured
    if node.consensus().is_none() {
        return Err(eyre::eyre!("No consensus engine configured"));
    }

    tracing::info!("Starting consensus loop with P2P integration");

    loop {
        // Get proposal from consensus (short borrow)
        let proposal = node.consensus().and_then(|c| c.recv_proposal());

        if let Some(proposal) = proposal {
            tracing::info!(
                "Received block proposal: block_number={}, tx_count={}",
                proposal.number,
                proposal.transactions.len()
            );

            let pending_txs = if let Some(rpc_server) = node.evm_rpc_server() {
                let txs = rpc_server.get_pending_transactions();
                rpc_server.clear_pending_transactions();
                txs
            } else {
                vec![]
            };

            let mut all_transactions = proposal.transactions.clone();
            for pending in &pending_txs {
                all_transactions.push(pending.tx.clone());
            }

            if !all_transactions.is_empty() {
                tracing::info!(
                    "Processing block {} with {} transactions ({} from mempool)",
                    proposal.number,
                    all_transactions.len(),
                    pending_txs.len()
                );
            }

            match node.executor_mut().execute_transactions(all_transactions.clone()) {
                Ok(result) => {
                    tracing::info!(
                        "Block executed successfully: gas_used={}, state_root={:?}",
                        result.total_gas_used,
                        result.combined_state_root
                    );

                    // Build a proper Ethereum header for hashing
                    let block_header = ConsensusHeader {
                        parent_hash: proposal.parent_hash,
                        ommers_hash: keccak256([0x80]), // RLP empty list
                        beneficiary: proposal.proposer,
                        state_root: result.combined_state_root,
                        transactions_root: keccak256([0x80]), // Empty trie root
                        receipts_root: keccak256([0x80]),
                        logs_bloom: Bloom::ZERO,
                        difficulty: U256::ZERO,
                        number: proposal.number,
                        gas_limit: 30_000_000,
                        gas_used: result.total_gas_used,
                        timestamp: proposal.timestamp,
                        extra_data: alloy_primitives::Bytes::copy_from_slice(&proposal.signature.to_bytes()),
                        mix_hash: B256::ZERO,
                        nonce: B64::ZERO,
                        base_fee_per_gas: Some(0),
                        withdrawals_root: None,
                        blob_gas_used: None,
                        excess_blob_gas: None,
                        parent_beacon_block_root: None,
                        requests_hash: None,
                    };
                    let block_hash = keccak256(alloy_rlp::encode(&block_header));

                    let tx_hashes: Vec<B256> =
                        all_transactions.iter().map(|tx| *tx.tx_hash()).collect();

                    // Store transaction receipts
                    if let Some(rpc_server) = node.evm_rpc_server() {
                        use alloy_consensus::transaction::SignerRecoverable;
                        use alloy_consensus::Transaction;

                        for (idx, (tx, receipt)) in all_transactions.iter().zip(result.evm_receipts.iter()).enumerate() {
                            let tx_hash = *tx.tx_hash();
                            let from = tx.recover_signer().unwrap_or_default();
                            let to = tx.to();

                            // Calculate contract address for contract creation txs
                            let contract_address = if to.is_none() && receipt.status.coerce_status() {
                                // Contract creation: address = keccak256(rlp([sender, nonce]))[12:]
                                let nonce = tx.nonce();
                                let mut data = Vec::new();
                                data.extend_from_slice(from.as_slice());
                                data.extend_from_slice(&nonce.to_be_bytes());
                                Some(Address::from_slice(&keccak256(&data)[12..]))
                            } else {
                                None
                            };

                            let rpc_receipt = dex_rpc::TransactionReceipt {
                                transaction_hash: tx_hash,
                                transaction_index: alloy_primitives::U64::from(idx),
                                block_hash,
                                block_number: alloy_primitives::U64::from(proposal.number),
                                from,
                                to,
                                cumulative_gas_used: alloy_primitives::U64::from(receipt.cumulative_gas_used),
                                gas_used: alloy_primitives::U64::from(21000u64), // Base gas for now
                                contract_address,
                                logs: vec![],
                                logs_bloom: alloy_primitives::Bytes::from(vec![0u8; 256]), // 256 bytes bloom filter
                                status: alloy_primitives::U64::from(if receipt.status.coerce_status() { 1u64 } else { 0u64 }),
                                tx_type: alloy_primitives::U64::from(0u64), // Legacy tx
                            };

                            rpc_server.add_receipt(tx_hash, rpc_receipt);
                        }
                    }

                    let stored_block = dex_storage::StoredBlock {
                        number: proposal.number,
                        hash: block_hash,
                        parent_hash: proposal.parent_hash,
                        timestamp: proposal.timestamp,
                        gas_limit: 30_000_000,
                        gas_used: result.total_gas_used,
                        miner: proposal.proposer,
                        evm_state_root: result.evm_state_root,
                        dexvm_state_root: result.dexvm_state_root,
                        combined_state_root: result.combined_state_root,
                        transaction_hashes: tx_hashes,
                        transaction_count: all_transactions.len() as u64,
                        signature: proposal.signature.to_bytes(),
                    };

                    if let Err(e) = node.block_store().store_block(stored_block) {
                        tracing::error!("Failed to store block: {}", e);
                    }

                    // Store full transaction data for block body sync
                    let tx_data: Vec<(B256, Vec<u8>)> = all_transactions.iter()
                        .map(|tx| (*tx.tx_hash(), alloy_rlp::encode(tx)))
                        .collect();
                    if let Err(e) = node.block_store().store_transactions(&tx_data) {
                        tracing::error!("Failed to store transactions: {}", e);
                    }

                    // Persist DexVM counter state to database
                    if let Ok(dexvm_exec) = node.executor().dexvm_executor().read() {
                        for (address, &value) in dexvm_exec.state().all_accounts() {
                            if let Err(e) = node.state_store().set_counter(*address, value) {
                                tracing::error!("Failed to persist DexVM counter for {}: {}", address, e);
                            }
                        }
                    }

                    // Finalize block (short borrow)
                    if let Some(consensus) = node.consensus() {
                        consensus.finalize_block(result.combined_state_root);
                    }

                    tracing::info!(
                        "Block {} finalized and stored, hash={:?}",
                        proposal.number,
                        block_hash
                    );

                    // Broadcast new block to all connected peers via P2P
                    if let Some(ref handle) = p2p_handle {
                        let last_block = *last_broadcast_block.read().await;
                        if proposal.number > last_block {
                            let cmd = SessionCommand::BroadcastBlock {
                                hash: block_hash,
                                number: proposal.number,
                            };
                            if let Err(e) = handle.send_command(cmd).await {
                                tracing::warn!("Failed to broadcast block via P2P: {}", e);
                            } else {
                                *last_broadcast_block.write().await = proposal.number;
                                tracing::debug!(
                                    "Broadcasted block {} to {} peers",
                                    proposal.number,
                                    handle.connected_count()
                                );
                            }
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Block execution failed: {}", e);
                }
            }
        }

        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let cli = Cli::parse();

    init_tracing(&cli.log_level)?;

    tracing::info!("====================================");
    tracing::info!("  Starting dex-reth Node v0.1.0");
    tracing::info!("====================================");
    tracing::info!("EVM JSON-RPC port: {}", cli.evm_rpc_port);
    tracing::info!("DexVM REST API port: {}", cli.dexvm_port);
    tracing::info!("Data directory: {}", cli.datadir.display());

    // Load genesis file
    let (chain_id, genesis_alloc, genesis_hash) = if let Some(genesis_path) = &cli.genesis {
        tracing::info!("Loading genesis file from: {}", genesis_path.display());
        let genesis_data = std::fs::read_to_string(genesis_path)?;
        let genesis: GenesisFile = serde_json::from_str(&genesis_data)?;

        let chain_id = genesis.config.chain_id;
        tracing::info!("Chain ID: {}", chain_id);

        let mut alloc = HashMap::new();
        for (address, account) in genesis.alloc {
            let balance = if account.balance.starts_with("0x") {
                U256::from_str_radix(&account.balance[2..], 16)?
            } else {
                U256::from_str_radix(&account.balance, 10)?
            };

            tracing::info!("Genesis account: {} with balance {} wei", address, balance);

            alloc.insert(address, balance);
        }

        // Compute genesis hash from genesis data
        let genesis_hash = keccak256(genesis_data.as_bytes());

        (chain_id, Some(alloc), genesis_hash)
    } else {
        tracing::info!("No genesis file specified, using default chain ID 1");
        (1, None, B256::ZERO)
    };

    // Create node
    let mut node = DualVmNode::with_full_config(
        chain_id,
        genesis_alloc.clone().unwrap_or_default(),
        cli.datadir.clone(),
        None,
    );

    // Start P2P service if enabled
    let _p2p_handle = if !cli.disable_p2p {
        tracing::info!("P2P networking enabled on port {}", cli.p2p_port);

        // Load or create persistent P2P secret key
        let key_path = cli.datadir.join("p2p_key");
        let secret_key = match P2pConfig::load_or_create_secret_key(&key_path) {
            Ok(key) => {
                tracing::info!("P2P key loaded from: {}", key_path.display());
                key
            }
            Err(e) => {
                tracing::warn!("Failed to load P2P key: {}, generating new key", e);
                P2pConfig::random_secret_key()
            }
        };
        let mut p2p_config = P2pConfig::new(secret_key, chain_id, genesis_hash)
            .with_port(cli.p2p_port)
            .with_max_peers(cli.max_peers);

        // Add boot nodes from CLI
        for bootnode in &cli.bootnodes {
            match bootnode.parse::<TrustedPeer>() {
                Ok(peer) => {
                    tracing::info!("Adding bootnode: {}", bootnode);
                    p2p_config = p2p_config.with_boot_node(peer);
                }
                Err(e) => {
                    tracing::warn!("Invalid bootnode URL '{}': {}", bootnode, e);
                }
            }
        }

        let p2p_service = P2pService::new(p2p_config);
        let handle = p2p_service.start().await?;

        // Display enode URL for other nodes to connect
        let local_id = handle.local_id();
        tracing::info!("P2P service started");
        tracing::info!("Local peer ID: {:?}", local_id);
        tracing::info!(
            "Enode URL: enode://{}@127.0.0.1:{}",
            hex::encode(local_id.as_slice()),
            cli.p2p_port
        );

        Some(handle)
    } else {
        tracing::info!("P2P networking disabled");
        None
    };

    // Configure POA consensus
    if cli.enable_consensus {
        let mut poa_config = PoaConfig::from_hex_key(
            &cli.validator_key,
            Duration::from_millis(cli.block_interval_ms),
        )
        .map_err(|e| eyre::eyre!("Invalid validator key: {}", e))?;

        let latest_block = node.block_store().latest_block_number();
        let last_block_hash = node
            .block_store()
            .get_block_by_number(latest_block)
            .map(|b| b.hash)
            .unwrap_or_default();

        poa_config.starting_block = latest_block;

        tracing::info!("POA consensus enabled");
        tracing::info!("Validator address: {:?}", poa_config.validator);
        tracing::info!("Block interval: {}ms", cli.block_interval_ms);
        tracing::info!("Continuing from block {} (hash: {:?})", latest_block, last_block_hash);

        node.set_consensus(poa_config, last_block_hash);
    } else {
        tracing::info!("POA consensus not enabled (RPC-only mode)");
    }

    // Start EVM JSON-RPC service
    let evm_rpc_handle = node.start_evm_rpc(cli.evm_rpc_port).await?;
    tracing::info!("EVM JSON-RPC available at: http://127.0.0.1:{}", cli.evm_rpc_port);

    // Start DexVM REST API service
    let dexvm_rpc_handle = node.start_dexvm_rpc(cli.dexvm_port).await?;
    tracing::info!("DexVM REST API available at: http://127.0.0.1:{}", cli.dexvm_port);

    tracing::info!("====================================");
    tracing::info!("  dex-reth Node started successfully");
    tracing::info!("====================================");
    tracing::info!("");
    tracing::info!("Endpoints:");
    tracing::info!("  - EVM RPC:    http://127.0.0.1:{}", cli.evm_rpc_port);
    tracing::info!("  - DexVM API:  http://127.0.0.1:{}", cli.dexvm_port);
    tracing::info!("  - Health:     http://127.0.0.1:{}/health", cli.dexvm_port);
    if !cli.disable_p2p {
        tracing::info!("  - P2P:        0.0.0.0:{}", cli.p2p_port);
    }
    tracing::info!("");
    tracing::info!("Data stored in: {}", cli.datadir.display());

    if cli.enable_consensus {
        let consensus_handle =
            node.start_consensus().ok_or_else(|| eyre::eyre!("Failed to start consensus"))?;

        tracing::info!("POA consensus engine started, auto block production enabled");

        // Clone P2P handle for block broadcasting
        let p2p_for_broadcast = _p2p_handle.clone();

        // Shared last block number for P2P broadcasting
        let last_broadcast_block = Arc::new(RwLock::new(0u64));
        let last_broadcast_block_for_loop = Arc::clone(&last_broadcast_block);

        // Start P2P event handler if P2P is enabled (responds to block requests)
        let p2p_event_handle = if let Some(p2p_handle) = _p2p_handle.clone() {
            let block_store = Arc::clone(&node.storage().blocks);
            let evm_rpc_server = node.evm_rpc_server().cloned();
            Some(tokio::spawn(async move {
                if let Err(e) = run_validator_p2p_handler(p2p_handle, block_store, evm_rpc_server).await {
                    tracing::error!("Validator P2P handler error: {}", e);
                }
            }))
        } else {
            None
        };

        let consensus_loop = tokio::spawn(async move {
            if let Err(e) = run_consensus_loop_with_p2p(
                node,
                p2p_for_broadcast,
                last_broadcast_block_for_loop,
            ).await {
                tracing::error!("Consensus loop error: {}", e);
            }
        });

        tracing::info!("");
        tracing::info!("Press Ctrl+C to stop");

        tokio::signal::ctrl_c().await?;

        tracing::info!("");
        tracing::info!("Shutting down dex-reth Node...");

        consensus_handle.abort();
        consensus_loop.abort();
        if let Some(h) = p2p_event_handle {
            h.abort();
        }
        dexvm_rpc_handle.abort();
        evm_rpc_handle.stop()?;
    } else {
        // Full node mode with block sync
        tracing::info!("Running in fullnode mode (sync only, no block production)");

        // Create transaction broadcast channel for fullnode to forward transactions
        let (tx_broadcast_tx, mut tx_broadcast_rx) = tokio::sync::mpsc::channel::<Vec<u8>>(256);

        // Set the broadcast channel on the RPC server if available
        if let Some(rpc_server) = node.evm_rpc_server() {
            rpc_server.set_tx_broadcast_sender(tx_broadcast_tx);
            tracing::info!("Transaction forwarding enabled for fullnode");
        }

        // Start fullnode sync handler if P2P is enabled
        let sync_handle = if let Some(p2p_handle) = _p2p_handle.clone() {
            let block_store = Arc::clone(&node.storage().blocks);
            Some(tokio::spawn(async move {
                if let Err(e) = run_fullnode_sync(p2p_handle, block_store).await {
                    tracing::error!("Fullnode sync error: {}", e);
                }
            }))
        } else {
            None
        };

        // Start transaction broadcast handler if P2P is enabled
        let tx_broadcast_handle = _p2p_handle.clone().map(|p2p_handle| tokio::spawn(async move {
                tracing::info!("Starting transaction broadcast handler");
                while let Some(tx_rlp) = tx_broadcast_rx.recv().await {
                    tracing::debug!("Broadcasting transaction to peers");
                    let cmd = SessionCommand::BroadcastTransactions {
                        transactions: vec![tx_rlp],
                    };
                    if let Err(e) = p2p_handle.send_command(cmd).await {
                        tracing::warn!("Failed to broadcast transaction: {}", e);
                    }
                }
            }));

        tracing::info!("");
        tracing::info!("Press Ctrl+C to stop");

        tokio::signal::ctrl_c().await?;

        tracing::info!("");
        tracing::info!("Shutting down dex-reth Node...");

        if let Some(h) = sync_handle {
            h.abort();
        }
        if let Some(h) = tx_broadcast_handle {
            h.abort();
        }
        dexvm_rpc_handle.abort();
        evm_rpc_handle.stop()?;
    }

    tracing::info!("dex-reth Node stopped.");
    Ok(())
}

fn init_tracing(level: &str) -> eyre::Result<()> {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .try_init()
        .map_err(|e| eyre::eyre!("Failed to initialize tracing: {}", e))?;

    Ok(())
}
