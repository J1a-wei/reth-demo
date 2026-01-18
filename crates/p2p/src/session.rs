//! Peer session handling with ECIES encryption and P2P protocol

use alloy_chains::Chain;
use alloy_hardforks::{ForkHash, ForkId};
use alloy_primitives::{B256, U256};
use futures::{SinkExt, StreamExt};
use reth_ecies::stream::ECIESStream;
use reth_eth_wire::{
    Capability, EthVersion, HelloMessageWithProtocols, P2PStream, ProtocolVersion,
    UnauthedP2PStream,
};
use reth_eth_wire_types::{EthMessage, EthNetworkPrimitives, ProtocolMessage, Status, StatusMessage};
use reth_network_peers::PeerId;
use secp256k1::SecretKey;
use std::net::SocketAddr;
use tokio::net::TcpStream;
use tracing::{debug, info, trace};

/// Client version string
pub const CLIENT_VERSION: &str = "dex-reth/0.1.0";

/// Session configuration
#[derive(Debug, Clone)]
pub struct SessionConfig {
    /// Local secret key
    pub secret_key: SecretKey,
    /// Chain ID
    pub chain_id: u64,
    /// Genesis hash
    pub genesis_hash: B256,
    /// Client version
    pub client_version: String,
}

impl SessionConfig {
    /// Create new session config
    pub fn new(secret_key: SecretKey, chain_id: u64, genesis_hash: B256) -> Self {
        Self {
            secret_key,
            chain_id,
            genesis_hash,
            client_version: CLIENT_VERSION.to_string(),
        }
    }
}

/// Result of establishing a peer session
pub struct EstablishedSession {
    /// Remote peer ID
    pub peer_id: PeerId,
    /// P2P stream for communication
    pub stream: P2PStream<ECIESStream<TcpStream>>,
    /// Shared capabilities
    pub capabilities: Vec<Capability>,
    /// Remote peer's status
    pub their_status: Status,
}

/// Create a Status message for ETH protocol handshake
fn create_status_message(config: &SessionConfig) -> Status {
    // Create a simple fork ID based on genesis hash only (no forks)
    let fork_hash = ForkHash::from(config.genesis_hash);
    let fork_id = ForkId { hash: fork_hash, next: 0 };

    Status {
        version: EthVersion::Eth68,
        chain: Chain::from_id(config.chain_id),
        total_difficulty: U256::ZERO, // POA doesn't use total difficulty
        blockhash: config.genesis_hash, // Will be updated with actual head
        genesis: config.genesis_hash,
        forkid: fork_id,
    }
}

/// Perform ETH Status handshake
async fn eth_status_handshake(
    stream: &mut P2PStream<ECIESStream<TcpStream>>,
    our_status: Status,
) -> eyre::Result<Status> {
    // Send our status
    let status_msg = ProtocolMessage::<EthNetworkPrimitives>::from(
        EthMessage::Status(StatusMessage::Legacy(our_status.clone()))
    );
    let encoded = alloy_rlp::encode(&status_msg);
    stream.send(encoded.into()).await?;
    trace!("Sent ETH Status message: {:?}", our_status);

    // Receive their status
    let their_msg = stream.next().await
        .ok_or_else(|| eyre::eyre!("Connection closed during status handshake"))??;

    // Decode the status message
    let protocol_msg = ProtocolMessage::<EthNetworkPrimitives>::decode_message(
        EthVersion::Eth68,
        &mut their_msg.as_ref(),
    ).map_err(|e| eyre::eyre!("Failed to decode status message: {}", e))?;

    match protocol_msg.message {
        EthMessage::Status(StatusMessage::Legacy(status)) => {
            trace!("Received ETH Status: {:?}", status);

            // Validate genesis hash matches
            if status.genesis != our_status.genesis {
                return Err(eyre::eyre!(
                    "Genesis hash mismatch: expected {:?}, got {:?}",
                    our_status.genesis,
                    status.genesis
                ));
            }

            // Validate chain ID matches
            if status.chain.id() != our_status.chain.id() {
                return Err(eyre::eyre!(
                    "Chain ID mismatch: expected {:?}, got {:?}",
                    our_status.chain,
                    status.chain
                ));
            }

            Ok(status)
        }
        EthMessage::Status(StatusMessage::Eth69(_)) => {
            Err(eyre::eyre!("Unexpected Eth69 status message"))
        }
        _ => Err(eyre::eyre!("Expected Status message, got {:?}", protocol_msg.message_type)),
    }
}

/// Establish an outbound session to a peer
pub async fn connect_outbound(
    addr: SocketAddr,
    remote_id: PeerId,
    config: &SessionConfig,
) -> eyre::Result<EstablishedSession> {
    info!("Connecting to peer {} at {}", remote_id, addr);

    // Connect TCP
    let tcp_stream = TcpStream::connect(addr).await?;

    // ECIES handshake
    trace!("Starting ECIES handshake with {}", remote_id);
    let ecies_stream = ECIESStream::connect(tcp_stream, config.secret_key, remote_id).await?;
    let actual_remote_id = ecies_stream.remote_id();
    debug!("ECIES handshake completed with peer {}", actual_remote_id);

    // P2P handshake
    let hello = create_hello_message(config);
    let unauth_p2p = UnauthedP2PStream::new(ecies_stream);

    trace!("Starting P2P handshake with {}", actual_remote_id);
    let (mut p2p_stream, their_hello) = unauth_p2p.handshake(hello).await?;
    info!(
        "P2P handshake completed with {}, client: {}, caps: {:?}",
        actual_remote_id, their_hello.client_version, their_hello.capabilities
    );

    // ETH Status handshake
    let our_status = create_status_message(config);
    trace!("Starting ETH Status handshake with {}", actual_remote_id);
    let their_status = eth_status_handshake(&mut p2p_stream, our_status).await?;
    info!(
        "ETH Status handshake completed with {}, chain: {}, genesis: {:?}",
        actual_remote_id, their_status.chain, their_status.genesis
    );

    Ok(EstablishedSession {
        peer_id: actual_remote_id,
        stream: p2p_stream,
        capabilities: their_hello.capabilities,
        their_status,
    })
}

/// Accept an inbound session from a peer
pub async fn accept_inbound(
    tcp_stream: TcpStream,
    addr: SocketAddr,
    config: &SessionConfig,
) -> eyre::Result<EstablishedSession> {
    info!("Accepting connection from {}", addr);

    // ECIES handshake (server side)
    trace!("Starting ECIES handshake (inbound) from {}", addr);
    let ecies_stream = ECIESStream::incoming(tcp_stream, config.secret_key).await?;
    let remote_id = ecies_stream.remote_id();
    debug!("ECIES handshake completed with peer {}", remote_id);

    // P2P handshake
    let hello = create_hello_message(config);
    let unauth_p2p = UnauthedP2PStream::new(ecies_stream);

    trace!("Starting P2P handshake with {}", remote_id);
    let (mut p2p_stream, their_hello) = unauth_p2p.handshake(hello).await?;
    info!(
        "P2P handshake completed with {}, client: {}, caps: {:?}",
        remote_id, their_hello.client_version, their_hello.capabilities
    );

    // ETH Status handshake
    let our_status = create_status_message(config);
    trace!("Starting ETH Status handshake with {}", remote_id);
    let their_status = eth_status_handshake(&mut p2p_stream, our_status).await?;
    info!(
        "ETH Status handshake completed with {}, chain: {}, genesis: {:?}",
        remote_id, their_status.chain, their_status.genesis
    );

    Ok(EstablishedSession {
        peer_id: remote_id,
        stream: p2p_stream,
        capabilities: their_hello.capabilities,
        their_status,
    })
}

/// Create hello message for P2P handshake
fn create_hello_message(config: &SessionConfig) -> HelloMessageWithProtocols {
    let local_id = reth_network_peers::pk2id(&config.secret_key.public_key(secp256k1::SECP256K1));

    HelloMessageWithProtocols::builder(local_id)
        .client_version(&config.client_version)
        .protocol_version(ProtocolVersion::V5)
        // Add eth68 capability (we're compatible with standard eth protocol for block sync)
        .protocol(EthVersion::Eth68)
        .build()
}

#[cfg(test)]
mod tests {
    use super::*;
    use secp256k1::SECP256K1;
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn test_session_handshake() {
        // Server setup
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server_key = SecretKey::new(&mut rand::thread_rng());
        let server_config = SessionConfig::new(server_key, 1, B256::ZERO);

        let client_key = SecretKey::new(&mut rand::thread_rng());
        let client_config = SessionConfig::new(client_key, 1, B256::ZERO);

        let server_id = reth_network_peers::pk2id(&server_key.public_key(SECP256K1));

        // Server task
        let server_config_clone = server_config.clone();
        let server_handle = tokio::spawn(async move {
            let (stream, peer_addr) = listener.accept().await.unwrap();
            accept_inbound(stream, peer_addr, &server_config_clone).await
        });

        // Client connect
        let client_result = connect_outbound(addr, server_id, &client_config).await;
        assert!(client_result.is_ok(), "Client connection failed: {:?}", client_result.err());

        let server_result = server_handle.await.unwrap();
        assert!(server_result.is_ok(), "Server accept failed: {:?}", server_result.err());
    }
}
