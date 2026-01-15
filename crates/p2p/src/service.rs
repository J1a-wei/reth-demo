//! P2P network service

use crate::{
    config::P2pConfig,
    peer::{PeerManager, PeerState, SharedPeerManager},
};
use alloy_primitives::{B256, B512};
use reth_network_peers::{pk2id, PeerId, TrustedPeer};
use secp256k1::{PublicKey, SECP256K1};
use std::{
    net::SocketAddr,
    sync::Arc,
    time::Duration,
};
use tokio::{
    net::{TcpListener, TcpStream},
    sync::{broadcast, mpsc},
    time::interval,
};

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
    /// Shutdown sender (kept for handle)
    shutdown_tx: mpsc::Sender<()>,
}

impl P2pService {
    /// Create new P2P service
    pub fn new(config: P2pConfig) -> Self {
        let peers = Arc::new(PeerManager::new(config.max_peers));
        let (event_tx, _) = broadcast::channel(1024);
        let (shutdown_tx, shutdown_rx) = mpsc::channel(1);

        // Derive local peer ID from secret key
        let public_key = PublicKey::from_secret_key(SECP256K1, &config.secret_key);
        let local_id = pk2id(&public_key);

        Self {
            config,
            peers,
            event_tx,
            local_id,
            shutdown_rx: Some(shutdown_rx),
            shutdown_tx,
        }
    }

    /// Get service handle
    pub fn handle(&self) -> P2pHandle {
        P2pHandle {
            event_tx: self.event_tx.clone(),
            peers: Arc::clone(&self.peers),
            local_id: self.local_id,
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

        // Spawn the main service loop
        tokio::spawn(async move {
            if let Err(e) = Self::run_service(
                config,
                peers,
                event_tx,
                local_id,
                &mut shutdown_rx,
            ).await {
                tracing::error!("P2P service error: {}", e);
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
    ) -> eyre::Result<()> {
        tracing::info!(
            "Starting P2P service on {}, local_id={:?}",
            config.listen_addr,
            local_id
        );

        // Bind TCP listener
        let listener = TcpListener::bind(config.listen_addr).await?;
        tracing::info!("P2P listening on {}", config.listen_addr);

        // Connect to boot nodes
        let boot_nodes = config.boot_nodes.clone();
        let peers_clone = Arc::clone(&peers);
        let event_tx_clone = event_tx.clone();

        tokio::spawn(async move {
            for boot_node in boot_nodes {
                Self::connect_to_peer(
                    boot_node,
                    Arc::clone(&peers_clone),
                    event_tx_clone.clone(),
                ).await;
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
                            tracing::debug!("Incoming connection from {}", addr);
                            Self::handle_incoming(
                                stream,
                                addr,
                                Arc::clone(&peers),
                                event_tx.clone(),
                            ).await;
                        }
                        Err(e) => {
                            tracing::warn!("Failed to accept connection: {}", e);
                        }
                    }
                }

                // Periodic maintenance
                _ = maintenance_interval.tick() => {
                    let connected = peers.connected_count();
                    let total = peers.peer_count();
                    tracing::debug!(
                        "P2P status: {}/{} peers connected, max={}",
                        connected,
                        total,
                        config.max_peers
                    );
                }

                // Shutdown signal
                _ = shutdown_rx.recv() => {
                    tracing::info!("P2P service shutting down");
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
    ) {
        // Resolve the peer to get the node record with IP address
        let node_record = match peer.resolve().await {
            Ok(record) => record,
            Err(e) => {
                tracing::warn!("Failed to resolve peer {}: {}", peer, e);
                return;
            }
        };

        let addr = SocketAddr::new(node_record.address, node_record.tcp_port);
        tracing::info!("Connecting to boot node: {}", addr);

        match TcpStream::connect(addr).await {
            Ok(_stream) => {
                // In a full implementation, we would:
                // 1. Perform ECIES handshake
                // 2. Exchange Hello messages
                // 3. Exchange Status messages

                let peer_id = peer.id;
                if peers.add_peer(peer_id, addr) {
                    peers.update_peer_state(&peer_id, PeerState::Connected);
                    let _ = event_tx.send(P2pEvent::PeerConnected { peer_id, addr });
                    tracing::info!("Connected to peer {:?} at {}", peer_id, addr);
                }
            }
            Err(e) => {
                tracing::warn!("Failed to connect to {}: {}", addr, e);
            }
        }
    }

    async fn handle_incoming(
        _stream: TcpStream,
        addr: SocketAddr,
        peers: SharedPeerManager,
        event_tx: broadcast::Sender<P2pEvent>,
    ) {
        if !peers.can_accept_peer() {
            tracing::debug!("Rejecting peer from {}: max peers reached", addr);
            return;
        }

        // In a full implementation, we would:
        // 1. Perform ECIES handshake to get peer ID
        // 2. Exchange Hello messages
        // 3. Exchange Status messages

        // For now, generate a placeholder peer ID
        let peer_id = PeerId::from(B512::random());

        if peers.add_peer(peer_id, addr) {
            peers.update_peer_state(&peer_id, PeerState::Connected);
            let _ = event_tx.send(P2pEvent::PeerConnected { peer_id, addr });
            tracing::info!("Accepted peer {:?} from {}", peer_id, addr);
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
