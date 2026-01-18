//! ETH protocol message handling for block synchronization

use alloy_consensus::Header as ConsensusHeader;
use alloy_primitives::B256;
use futures::{SinkExt, StreamExt};
use reth_ecies::stream::ECIESStream;
use reth_eth_wire::{EthVersion, P2PStream};
use reth_eth_wire_types::{
    BlockHashNumber, EthMessage, EthNetworkPrimitives, GetBlockBodies, GetBlockHeaders,
    HashOrNumber, HeadersDirection, NewBlockHashes, ProtocolMessage,
};
use reth_eth_wire::message::RequestPair;
use reth_network_peers::PeerId;
use tokio::{
    net::TcpStream,
    sync::mpsc,
};
use tracing::{debug, info, trace, warn};

/// Events emitted by the ETH message handler
#[derive(Debug, Clone)]
pub enum EthHandlerEvent {
    /// Received new block hash announcement
    NewBlockHashes {
        peer_id: PeerId,
        hashes: Vec<(B256, u64)>, // (hash, number)
    },
    /// Received block headers response
    BlockHeaders {
        peer_id: PeerId,
        request_id: u64,
        headers: Vec<ConsensusHeader>,
    },
    /// Received block bodies response
    BlockBodies {
        peer_id: PeerId,
        request_id: u64,
        bodies: Vec<reth_ethereum_primitives::BlockBody>,
    },
    /// Session disconnected
    Disconnected { peer_id: PeerId },
    /// Received request for block headers (validator should respond)
    GetBlockHeadersRequest {
        peer_id: PeerId,
        request_id: u64,
        start: HashOrNumber,
        limit: u64,
    },
    /// Received request for block bodies (validator should respond)
    GetBlockBodiesRequest {
        peer_id: PeerId,
        request_id: u64,
        hashes: Vec<B256>,
    },
    /// Received transactions from peer (validator should add to mempool)
    Transactions {
        peer_id: PeerId,
        transactions: Vec<Vec<u8>>, // RLP-encoded transactions
    },
}

/// Commands that can be sent to the ETH handler
#[derive(Debug)]
pub enum EthHandlerCommand {
    /// Request block headers from peer
    GetBlockHeaders {
        start: BlockHashOrNumber,
        limit: u64,
        request_id: u64,
    },
    /// Request block bodies from peer
    GetBlockBodies {
        hashes: Vec<B256>,
        request_id: u64,
    },
    /// Announce new block hashes to peer
    AnnounceBlocks {
        blocks: Vec<(B256, u64)>, // (hash, number)
    },
    /// Send block headers response
    SendBlockHeaders {
        request_id: u64,
        headers: Vec<ConsensusHeader>,
    },
    /// Send block bodies response
    SendBlockBodies {
        request_id: u64,
        bodies: Vec<reth_ethereum_primitives::BlockBody>,
    },
    /// Broadcast transactions to peer
    BroadcastTransactions {
        transactions: Vec<Vec<u8>>, // RLP-encoded transactions
    },
}

/// Block hash or number for header requests
#[derive(Debug, Clone)]
pub enum BlockHashOrNumber {
    Hash(B256),
    Number(u64),
}

/// Run the ETH message handler for a peer session
pub async fn run_eth_handler(
    peer_id: PeerId,
    mut stream: P2PStream<ECIESStream<TcpStream>>,
    mut command_rx: mpsc::Receiver<EthHandlerCommand>,
    event_tx: mpsc::Sender<EthHandlerEvent>,
) {
    info!("ETH handler started for peer {}", peer_id);

    loop {
        tokio::select! {
            // Handle incoming messages from peer
            msg_result = stream.next() => {
                match msg_result {
                    Some(Ok(bytes)) => {
                        if let Err(e) = handle_incoming_message(
                            peer_id,
                            &bytes,
                            &event_tx,
                        ).await {
                            warn!("Error handling message from peer {}: {}", peer_id, e);
                        }
                    }
                    Some(Err(e)) => {
                        warn!("Stream error from peer {}: {}", peer_id, e);
                        let _ = event_tx.send(EthHandlerEvent::Disconnected { peer_id }).await;
                        break;
                    }
                    None => {
                        info!("Stream closed for peer {}", peer_id);
                        let _ = event_tx.send(EthHandlerEvent::Disconnected { peer_id }).await;
                        break;
                    }
                }
            }

            // Handle outgoing commands
            Some(cmd) = command_rx.recv() => {
                if let Err(e) = handle_command(
                    &mut stream,
                    cmd,
                ).await {
                    warn!("Error sending command to peer {}: {}", peer_id, e);
                    let _ = event_tx.send(EthHandlerEvent::Disconnected { peer_id }).await;
                    break;
                }
            }
        }
    }

    info!("ETH handler stopped for peer {}", peer_id);
}

async fn handle_incoming_message(
    peer_id: PeerId,
    bytes: &[u8],
    event_tx: &mpsc::Sender<EthHandlerEvent>,
) -> eyre::Result<()> {
    let msg = ProtocolMessage::<EthNetworkPrimitives>::decode_message(
        EthVersion::Eth68,
        &mut &bytes[..],
    )?;

    match msg.message {
        EthMessage::NewBlockHashes(hashes) => {
            trace!("Received NewBlockHashes from peer {}: {} hashes", peer_id, hashes.0.len());
            let blocks: Vec<_> = hashes.0.iter()
                .map(|h| (h.hash, h.number))
                .collect();
            event_tx.send(EthHandlerEvent::NewBlockHashes { peer_id, hashes: blocks }).await?;
        }

        EthMessage::BlockHeaders(response) => {
            debug!(
                "Received BlockHeaders from peer {}: request_id={}, {} headers",
                peer_id, response.request_id, response.message.0.len()
            );
            event_tx.send(EthHandlerEvent::BlockHeaders {
                peer_id,
                request_id: response.request_id,
                headers: response.message.0,
            }).await?;
        }

        EthMessage::BlockBodies(response) => {
            debug!(
                "Received BlockBodies from peer {}: request_id={}, {} bodies",
                peer_id, response.request_id, response.message.0.len()
            );
            event_tx.send(EthHandlerEvent::BlockBodies {
                peer_id,
                request_id: response.request_id,
                bodies: response.message.0,
            }).await?;
        }

        EthMessage::GetBlockHeaders(request) => {
            debug!(
                "Received GetBlockHeaders from peer {}: request_id={}, start={:?}, limit={}",
                peer_id, request.request_id, request.message.start_block, request.message.limit
            );
            event_tx.send(EthHandlerEvent::GetBlockHeadersRequest {
                peer_id,
                request_id: request.request_id,
                start: request.message.start_block,
                limit: request.message.limit,
            }).await?;
        }

        EthMessage::GetBlockBodies(request) => {
            debug!(
                "Received GetBlockBodies from peer {}: request_id={}, hashes={}",
                peer_id, request.request_id, request.message.0.len()
            );
            event_tx.send(EthHandlerEvent::GetBlockBodiesRequest {
                peer_id,
                request_id: request.request_id,
                hashes: request.message.0,
            }).await?;
        }

        EthMessage::NewBlock(_block) => {
            debug!("Received NewBlock from peer {}", peer_id);
            // TODO: Handle full block announcements
        }

        EthMessage::Transactions(txs) => {
            debug!("Received {} transactions from peer {}", txs.0.len(), peer_id);
            // Forward transactions to be processed
            let rlp_txs: Vec<Vec<u8>> = txs.0.iter()
                .map(|tx| alloy_rlp::encode(tx))
                .collect();
            event_tx.send(EthHandlerEvent::Transactions { peer_id, transactions: rlp_txs }).await?;
        }

        EthMessage::NewPooledTransactionHashes66(_) | EthMessage::NewPooledTransactionHashes68(_) => {
            trace!("Received NewPooledTransactionHashes from peer {} (ignoring)", peer_id);
            // We don't need to handle transaction hashes for now
        }

        _ => {
            trace!("Received unhandled message type {:?} from peer {}", msg.message_type, peer_id);
        }
    }

    Ok(())
}

async fn handle_command(
    stream: &mut P2PStream<ECIESStream<TcpStream>>,
    cmd: EthHandlerCommand,
) -> eyre::Result<()> {
    match cmd {
        EthHandlerCommand::GetBlockHeaders { start, limit, request_id } => {
            let start_block = match start {
                BlockHashOrNumber::Hash(hash) => HashOrNumber::Hash(hash),
                BlockHashOrNumber::Number(num) => HashOrNumber::Number(num),
            };

            let request = GetBlockHeaders {
                start_block,
                limit,
                skip: 0,
                direction: HeadersDirection::Falling,
            };

            let msg = ProtocolMessage::<EthNetworkPrimitives>::from(
                EthMessage::GetBlockHeaders(RequestPair {
                    request_id,
                    message: request,
                })
            );

            let encoded = alloy_rlp::encode(&msg);
            stream.send(encoded.into()).await?;
            trace!("Sent GetBlockHeaders request_id={}", request_id);
        }

        EthHandlerCommand::GetBlockBodies { hashes, request_id } => {
            let msg = ProtocolMessage::<EthNetworkPrimitives>::from(
                EthMessage::GetBlockBodies(RequestPair {
                    request_id,
                    message: GetBlockBodies(hashes),
                })
            );

            let encoded = alloy_rlp::encode(&msg);
            stream.send(encoded.into()).await?;
            trace!("Sent GetBlockBodies request_id={}", request_id);
        }

        EthHandlerCommand::AnnounceBlocks { blocks } => {
            let hashes: Vec<_> = blocks.into_iter()
                .map(|(hash, number)| BlockHashNumber { hash, number })
                .collect();

            let msg = ProtocolMessage::<EthNetworkPrimitives>::from(
                EthMessage::NewBlockHashes(NewBlockHashes(hashes))
            );

            let encoded = alloy_rlp::encode(&msg);
            stream.send(encoded.into()).await?;
            trace!("Sent NewBlockHashes announcement");
        }

        EthHandlerCommand::SendBlockHeaders { request_id, headers } => {
            use reth_eth_wire_types::BlockHeaders;
            let msg = ProtocolMessage::<EthNetworkPrimitives>::from(
                EthMessage::BlockHeaders(RequestPair {
                    request_id,
                    message: BlockHeaders(headers),
                })
            );

            let encoded = alloy_rlp::encode(&msg);
            stream.send(encoded.into()).await?;
            trace!("Sent BlockHeaders response request_id={}", request_id);
        }

        EthHandlerCommand::SendBlockBodies { request_id, bodies } => {
            use reth_eth_wire_types::BlockBodies;
            let msg = ProtocolMessage::<EthNetworkPrimitives>::from(
                EthMessage::BlockBodies(RequestPair {
                    request_id,
                    message: BlockBodies(bodies),
                })
            );

            let encoded = alloy_rlp::encode(&msg);
            stream.send(encoded.into()).await?;
            trace!("Sent BlockBodies response request_id={}", request_id);
        }

        EthHandlerCommand::BroadcastTransactions { transactions } => {
            use alloy_rlp::Decodable;
            use reth_ethereum_primitives::TransactionSigned;
            use reth_eth_wire_types::Transactions;

            // Decode RLP transactions
            let decoded_txs: Vec<TransactionSigned> = transactions.iter()
                .filter_map(|rlp| TransactionSigned::decode(&mut rlp.as_slice()).ok())
                .collect();

            if decoded_txs.is_empty() {
                trace!("No valid transactions to broadcast");
            } else {
                let msg = ProtocolMessage::<EthNetworkPrimitives>::from(
                    EthMessage::Transactions(Transactions(decoded_txs))
                );

                let encoded = alloy_rlp::encode(&msg);
                stream.send(encoded.into()).await?;
                trace!("Broadcasted {} transactions", transactions.len());
            }
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_block_hash_or_number() {
        let by_hash = BlockHashOrNumber::Hash(B256::ZERO);
        let by_number = BlockHashOrNumber::Number(100);

        match by_hash {
            BlockHashOrNumber::Hash(h) => assert_eq!(h, B256::ZERO),
            _ => panic!("Expected Hash variant"),
        }

        match by_number {
            BlockHashOrNumber::Number(n) => assert_eq!(n, 100),
            _ => panic!("Expected Number variant"),
        }
    }
}
