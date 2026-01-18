//! P2P network service

use crate::{
    config::P2pConfig,
    eth_handler::{run_eth_handler, EthHandlerCommand, EthHandlerEvent},
    peer::{PeerManager, PeerState, SharedPeerManager},
    session::{accept_inbound, connect_outbound, SessionConfig},
};
use alloy_consensus::Header as ConsensusHeader;
use alloy_primitives::B256;
use reth_network_peers::{pk2id, PeerId, TrustedPeer};
use secp256k1::{PublicKey, SECP256K1};
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{broadcast, mpsc, RwLock},
    time::interval,
};
use tracing::{debug, error, info, warn};

/// P2P network events
#[derive(Debug, Clone)]
pub enum P2pEvent {
    /// New peer connected
    PeerConnected { peer_id: PeerId, addr: SocketAddr },
    /// Peer disconnected
    PeerDisconnected { peer_id: PeerId },
    /// Received new transaction hashes
    NewPooledTransactionHashes { peer_id: PeerId, hashes: Vec<B256> },
    /// Received new block hash announcement
    NewBlockHash { peer_id: PeerId, hash: B256, number: u64 },
    /// Received new block
    NewBlock { peer_id: PeerId, hash: B256, number: u64 },
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
    /// Peer requesting block headers (validator should respond)
    GetBlockHeadersRequest {
        peer_id: PeerId,
        request_id: u64,
        start: reth_eth_wire_types::HashOrNumber,
        limit: u64,
    },
    /// Peer requesting block bodies (validator should respond)
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

/// P2P service handle
#[derive(Clone)]
pub struct P2pHandle {
    /// Event receiver
    event_tx: broadcast::Sender<P2pEvent>,
    /// Peer manager
    peers: SharedPeerManager,
    /// Local peer ID
    local_id: PeerId,
    /// Shutdown sender (kept alive to prevent service from stopping)
    _shutdown_tx: Arc<mpsc::Sender<()>>,
    /// Session sender for sending messages to peers
    session_tx: mpsc::Sender<SessionCommand>,
}

/// Commands to send to active sessions
#[derive(Debug)]
pub enum SessionCommand {
    /// Broadcast a new block to all peers
    BroadcastBlock { hash: B256, number: u64 },
    /// Request block headers from a peer
    GetBlockHeaders { peer_id: PeerId, start: u64, count: u64 },
    /// Request block bodies from a peer
    GetBlockBodies { peer_id: PeerId, hashes: Vec<B256> },
    /// Send block headers response to a peer
    SendBlockHeaders { peer_id: PeerId, request_id: u64, headers: Vec<ConsensusHeader> },
    /// Send block bodies response to a peer
    SendBlockBodies { peer_id: PeerId, request_id: u64, bodies: Vec<reth_ethereum_primitives::BlockBody> },
    /// Broadcast transactions to all peers
    BroadcastTransactions { transactions: Vec<Vec<u8>> },
}

impl P2pHandle {
    /// Get local peer ID
    pub fn local_id(&self) -> PeerId {
        self.local_id
    }

    /// Get peer count
    pub fn peer_count(&self) -> usize {
        self.peers.peer_count()
    }

    /// Get connected peer count
    pub fn connected_count(&self) -> usize {
        self.peers.connected_count()
    }

    /// Subscribe to P2P events
    pub fn subscribe(&self) -> broadcast::Receiver<P2pEvent> {
        self.event_tx.subscribe()
    }

    /// Get all connected peer IDs
    pub fn connected_peers(&self) -> Vec<PeerId> {
        self.peers
            .connected_peers()
            .into_iter()
            .map(|p| p.id)
            .collect()
    }

    /// Send a command to sessions
    pub async fn send_command(&self, cmd: SessionCommand) -> Result<(), mpsc::error::SendError<SessionCommand>> {
        self.session_tx.send(cmd).await
    }
}

/// P2P network service
pub struct P2pService {
    /// Configuration
    config: P2pConfig,
    /// Peer manager
    peers: SharedPeerManager,
    /// Event sender
    event_tx: broadcast::Sender<P2pEvent>,
    /// Local peer ID
    local_id: PeerId,
    /// Shutdown signal
    shutdown_rx: Option<mpsc::Receiver<()>>,
    /// Shutdown sender (wrapped in Arc to keep alive in handle)
    shutdown_tx: Arc<mpsc::Sender<()>>,
    /// Session command sender
    session_tx: mpsc::Sender<SessionCommand>,
    /// Session command receiver
    session_rx: Option<mpsc::Receiver<SessionCommand>>,
}

impl P2pService {
    /// Create new P2P service
    pub fn new(config: P2pConfig) -> Self {
        let peers = Arc::new(PeerManager::new(config.max_peers));
        let (event_tx, _) = broadcast::channel(1024);
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);
        let (session_tx, session_rx) = mpsc::channel(256);

        // Derive local peer ID from secret key
        let public_key = PublicKey::from_secret_key(SECP256K1, &config.secret_key);
        let local_id = pk2id(&public_key);

        Self {
            config,
            peers,
            event_tx,
            local_id,
            shutdown_rx: Some(shutdown_rx),
            shutdown_tx: Arc::new(shutdown_tx),
            session_tx,
            session_rx: Some(session_rx),
        }
    }

    /// Get service handle
    pub fn handle(&self) -> P2pHandle {
        P2pHandle {
            event_tx: self.event_tx.clone(),
            peers: Arc::clone(&self.peers),
            local_id: self.local_id,
            _shutdown_tx: Arc::clone(&self.shutdown_tx),
            session_tx: self.session_tx.clone(),
        }
    }

    /// Start the P2P service
    pub async fn start(mut self) -> eyre::Result<P2pHandle> {
        let handle = self.handle();
        let config = self.config.clone();
        let peers = Arc::clone(&self.peers);
        let event_tx = self.event_tx.clone();
        let local_id = self.local_id;
        let mut shutdown_rx = self.shutdown_rx.take().unwrap();
        let mut session_rx = self.session_rx.take().unwrap();

        // Spawn the main service loop
        tokio::spawn(async move {
            if let Err(e) = Self::run_service(
                config,
                peers,
                event_tx,
                local_id,
                &mut shutdown_rx,
                &mut session_rx,
            )
            .await
            {
                error!("P2P service error: {}", e);
            }
        });

        Ok(handle)
    }

    async fn run_service(
        config: P2pConfig,
        peers: SharedPeerManager,
        event_tx: broadcast::Sender<P2pEvent>,
        local_id: PeerId,
        shutdown_rx: &mut mpsc::Receiver<()>,
        session_rx: &mut mpsc::Receiver<SessionCommand>,
    ) -> eyre::Result<()> {
        info!(
            "Starting P2P service on {}, local_id={:?}",
            config.listen_addr, local_id
        );

        // Create session config
        let session_config = SessionConfig::new(config.secret_key, config.chain_id, config.genesis_hash);

        // Bind TCP listener
        let listener = TcpListener::bind(config.listen_addr).await?;
        info!("P2P listening on {}", config.listen_addr);

        // Active sessions storage - now stores command sender per peer
        let peer_commands: Arc<RwLock<HashMap<PeerId, mpsc::Sender<EthHandlerCommand>>>> =
            Arc::new(RwLock::new(HashMap::new()));

        // Channel for receiving events from all ETH handlers
        let (eth_event_tx, mut eth_event_rx) = mpsc::channel::<EthHandlerEvent>(1024);

        // Connect to boot nodes
        let boot_nodes = config.boot_nodes.clone();
        let session_config_clone = session_config.clone();
        let peers_clone = Arc::clone(&peers);
        let event_tx_clone = event_tx.clone();
        let peer_commands_clone = Arc::clone(&peer_commands);
        let eth_event_tx_clone = eth_event_tx.clone();

        tokio::spawn(async move {
            for boot_node in boot_nodes {
                Self::connect_to_peer(
                    boot_node,
                    Arc::clone(&peers_clone),
                    event_tx_clone.clone(),
                    session_config_clone.clone(),
                    Arc::clone(&peer_commands_clone),
                    eth_event_tx_clone.clone(),
                )
                .await;
            }
        });

        // Periodic peer maintenance
        let mut maintenance_interval = interval(Duration::from_secs(30));

        loop {
            tokio::select! {
                // Accept incoming connections
                accept_result = listener.accept() => {
                    match accept_result {
                        Ok((stream, addr)) => {
                            debug!("Incoming connection from {}", addr);
                            let session_config = session_config.clone();
                            let peers = Arc::clone(&peers);
                            let event_tx = event_tx.clone();
                            let peer_commands = Arc::clone(&peer_commands);
                            let eth_event_tx = eth_event_tx.clone();

                            tokio::spawn(async move {
                                Self::handle_incoming(
                                    stream,
                                    addr,
                                    peers,
                                    event_tx,
                                    session_config,
                                    peer_commands,
                                    eth_event_tx,
                                ).await;
                            });
                        }
                        Err(e) => {
                            warn!("Failed to accept connection: {}", e);
                        }
                    }
                }

                // Handle session commands from external callers
                Some(cmd) = session_rx.recv() => {
                    match cmd {
                        SessionCommand::BroadcastBlock { hash, number } => {
                            debug!("Broadcasting block {} to all peers", number);
                            let commands = peer_commands.read().await;
                            for (peer_id, sender) in commands.iter() {
                                let cmd = EthHandlerCommand::AnnounceBlocks {
                                    blocks: vec![(hash, number)],
                                };
                                if let Err(e) = sender.send(cmd).await {
                                    warn!("Failed to send block announcement to peer {}: {}", peer_id, e);
                                }
                            }
                        }
                        SessionCommand::GetBlockHeaders { peer_id, start, count } => {
                            let commands = peer_commands.read().await;
                            if let Some(sender) = commands.get(&peer_id) {
                                let cmd = EthHandlerCommand::GetBlockHeaders {
                                    start: crate::BlockHashOrNumber::Number(start),
                                    limit: count,
                                    request_id: rand::random(),
                                };
                                if let Err(e) = sender.send(cmd).await {
                                    warn!("Failed to send GetBlockHeaders to peer {}: {}", peer_id, e);
                                }
                            }
                        }
                        SessionCommand::GetBlockBodies { peer_id, hashes } => {
                            let commands = peer_commands.read().await;
                            if let Some(sender) = commands.get(&peer_id) {
                                let cmd = EthHandlerCommand::GetBlockBodies {
                                    hashes,
                                    request_id: rand::random(),
                                };
                                if let Err(e) = sender.send(cmd).await {
                                    warn!("Failed to send GetBlockBodies to peer {}: {}", peer_id, e);
                                }
                            }
                        }
                        SessionCommand::SendBlockHeaders { peer_id, request_id, headers } => {
                            let commands = peer_commands.read().await;
                            if let Some(sender) = commands.get(&peer_id) {
                                let cmd = EthHandlerCommand::SendBlockHeaders {
                                    request_id,
                                    headers,
                                };
                                if let Err(e) = sender.send(cmd).await {
                                    warn!("Failed to send BlockHeaders to peer {}: {}", peer_id, e);
                                }
                            }
                        }
                        SessionCommand::SendBlockBodies { peer_id, request_id, bodies } => {
                            let commands = peer_commands.read().await;
                            if let Some(sender) = commands.get(&peer_id) {
                                let cmd = EthHandlerCommand::SendBlockBodies {
                                    request_id,
                                    bodies,
                                };
                                if let Err(e) = sender.send(cmd).await {
                                    warn!("Failed to send BlockBodies to peer {}: {}", peer_id, e);
                                }
                            }
                        }
                        SessionCommand::BroadcastTransactions { transactions } => {
                            debug!("Broadcasting {} transactions to all peers", transactions.len());
                            let commands = peer_commands.read().await;
                            for (peer_id, sender) in commands.iter() {
                                let cmd = EthHandlerCommand::BroadcastTransactions {
                                    transactions: transactions.clone(),
                                };
                                if let Err(e) = sender.send(cmd).await {
                                    warn!("Failed to send transactions to peer {}: {}", peer_id, e);
                                }
                            }
                        }
                    }
                }

                // Handle events from ETH handlers
                Some(eth_event) = eth_event_rx.recv() => {
                    match eth_event {
                        EthHandlerEvent::NewBlockHashes { peer_id, hashes } => {
                            for (hash, number) in hashes {
                                debug!("Received NewBlockHash from peer {}: {} at {}", peer_id, hash, number);
                                let _ = event_tx.send(P2pEvent::NewBlockHash { peer_id, hash, number });
                            }
                        }
                        EthHandlerEvent::BlockHeaders { peer_id, request_id, headers } => {
                            debug!("Received {} block headers from peer {} (request_id={})", headers.len(), peer_id, request_id);
                            let _ = event_tx.send(P2pEvent::BlockHeaders { peer_id, request_id, headers });
                        }
                        EthHandlerEvent::BlockBodies { peer_id, request_id, bodies } => {
                            debug!("Received {} block bodies from peer {} (request_id={})", bodies.len(), peer_id, request_id);
                            let _ = event_tx.send(P2pEvent::BlockBodies { peer_id, request_id, bodies });
                        }
                        EthHandlerEvent::Disconnected { peer_id } => {
                            info!("Peer {} disconnected", peer_id);
                            peers.update_peer_state(&peer_id, PeerState::Disconnected);
                            peer_commands.write().await.remove(&peer_id);
                            let _ = event_tx.send(P2pEvent::PeerDisconnected { peer_id });
                        }
                        EthHandlerEvent::GetBlockHeadersRequest { peer_id, request_id, start, limit } => {
                            debug!("Peer {} requesting {} headers starting from {:?}", peer_id, limit, start);
                            let _ = event_tx.send(P2pEvent::GetBlockHeadersRequest { peer_id, request_id, start, limit });
                        }
                        EthHandlerEvent::GetBlockBodiesRequest { peer_id, request_id, hashes } => {
                            debug!("Peer {} requesting {} block bodies", peer_id, hashes.len());
                            let _ = event_tx.send(P2pEvent::GetBlockBodiesRequest { peer_id, request_id, hashes });
                        }
                        EthHandlerEvent::Transactions { peer_id, transactions } => {
                            debug!("Received {} transactions from peer {}", transactions.len(), peer_id);
                            let _ = event_tx.send(P2pEvent::Transactions { peer_id, transactions });
                        }
                    }
                }

                // Periodic maintenance
                _ = maintenance_interval.tick() => {
                    let connected = peers.connected_count();
                    let total = peers.peer_count();
                    debug!(
                        "P2P status: {}/{} peers connected, max={}",
                        connected,
                        total,
                        config.max_peers
                    );
                }

                // Shutdown signal
                _ = shutdown_rx.recv() => {
                    info!("P2P service shutting down");
                    break;
                }
            }
        }

        Ok(())
    }

    async fn connect_to_peer(
        peer: TrustedPeer,
        peers: SharedPeerManager,
        event_tx: broadcast::Sender<P2pEvent>,
        session_config: SessionConfig,
        peer_commands: Arc<RwLock<HashMap<PeerId, mpsc::Sender<EthHandlerCommand>>>>,
        eth_event_tx: mpsc::Sender<EthHandlerEvent>,
    ) {
        // Resolve the peer to get the node record with IP address
        let node_record = match peer.resolve().await {
            Ok(record) => record,
            Err(e) => {
                warn!("Failed to resolve peer {}: {}", peer, e);
                return;
            }
        };

        let remote_id = peer.id;
        let addr = SocketAddr::new(node_record.address, node_record.tcp_port);
        info!("Connecting to boot node: {} at {}", remote_id, addr);

        // Establish session with ECIES + P2P + ETH Status handshake
        match connect_outbound(addr, remote_id, &session_config).await {
            Ok(session) => {
                let peer_id = session.peer_id;

                if peers.add_peer(peer_id, addr) {
                    peers.update_peer_state(&peer_id, PeerState::Connected);
                    let _ = event_tx.send(P2pEvent::PeerConnected { peer_id, addr });
                    info!("Connected to peer {} at {}", peer_id, addr);

                    // Create command channel for this peer
                    let (cmd_tx, cmd_rx) = mpsc::channel(256);
                    peer_commands.write().await.insert(peer_id, cmd_tx);

                    // Spawn ETH handler for this session
                    tokio::spawn(async move {
                        run_eth_handler(peer_id, session.stream, cmd_rx, eth_event_tx).await;
                    });
                }
            }
            Err(e) => {
                warn!("Failed to connect to {}: {}", addr, e);
            }
        }
    }

    async fn handle_incoming(
        stream: TcpStream,
        addr: SocketAddr,
        peers: SharedPeerManager,
        event_tx: broadcast::Sender<P2pEvent>,
        session_config: SessionConfig,
        peer_commands: Arc<RwLock<HashMap<PeerId, mpsc::Sender<EthHandlerCommand>>>>,
        eth_event_tx: mpsc::Sender<EthHandlerEvent>,
    ) {
        if !peers.can_accept_peer() {
            debug!("Rejecting peer from {}: max peers reached", addr);
            return;
        }

        // Establish session with ECIES + P2P + ETH Status handshake
        match accept_inbound(stream, addr, &session_config).await {
            Ok(session) => {
                let peer_id = session.peer_id;

                if peers.add_peer(peer_id, addr) {
                    peers.update_peer_state(&peer_id, PeerState::Connected);
                    let _ = event_tx.send(P2pEvent::PeerConnected { peer_id, addr });
                    info!("Accepted peer {} from {}", peer_id, addr);

                    // Create command channel for this peer
                    let (cmd_tx, cmd_rx) = mpsc::channel(256);
                    peer_commands.write().await.insert(peer_id, cmd_tx);

                    // Spawn ETH handler for this session
                    tokio::spawn(async move {
                        run_eth_handler(peer_id, session.stream, cmd_rx, eth_event_tx).await;
                    });
                }
            }
            Err(e) => {
                warn!("Failed to accept peer from {}: {}", addr, e);
            }
        }
    }
}

/// Builder for P2P service
pub struct P2pServiceBuilder {
    config: P2pConfig,
}

impl P2pServiceBuilder {
    /// Create new builder
    pub fn new(config: P2pConfig) -> Self {
        Self { config }
    }

    /// Set listen port
    pub fn port(mut self, port: u16) -> Self {
        self.config = self.config.with_port(port);
        self
    }

    /// Add boot node
    pub fn boot_node(mut self, node: TrustedPeer) -> Self {
        self.config = self.config.with_boot_node(node);
        self
    }

    /// Set max peers
    pub fn max_peers(mut self, max: usize) -> Self {
        self.config = self.config.with_max_peers(max);
        self
    }

    /// Build the service
    pub fn build(self) -> P2pService {
        P2pService::new(self.config)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_service_creation() {
        let config = P2pConfig::default().with_port(0); // Use random port
        let service = P2pService::new(config);
        let handle = service.handle();

        assert_eq!(handle.peer_count(), 0);
    }

    #[tokio::test]
    async fn test_service_start() {
        let config = P2pConfig::default().with_port(0);
        let service = P2pService::new(config);

        let handle = service.start().await.unwrap();

        // Give the service time to start
        tokio::time::sleep(Duration::from_millis(100)).await;

        assert_eq!(handle.peer_count(), 0);
    }
}
