//! Peer management

use alloy_primitives::B256;
use parking_lot::RwLock;
use reth_network_peers::PeerId;
use std::{
    collections::HashMap,
    net::SocketAddr,
    sync::Arc,
    time::Instant,
};

/// Peer connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PeerState {
    /// Connecting to peer
    Connecting,
    /// Connected and handshaking
    Handshaking,
    /// Fully connected
    Connected,
    /// Disconnected
    Disconnected,
}

/// Information about a connected peer
#[derive(Debug, Clone)]
pub struct PeerInfo {
    /// Peer ID (public key)
    pub id: PeerId,
    /// Remote address
    pub addr: SocketAddr,
    /// Connection state
    pub state: PeerState,
    /// Client version string
    pub client_version: Option<String>,
    /// Peer's chain head
    pub head_hash: Option<B256>,
    /// Peer's total difficulty
    pub total_difficulty: Option<u128>,
    /// Time of last message
    pub last_seen: Instant,
    /// Connected at
    pub connected_at: Instant,
}

impl PeerInfo {
    /// Create new peer info
    pub fn new(id: PeerId, addr: SocketAddr) -> Self {
        let now = Instant::now();
        Self {
            id,
            addr,
            state: PeerState::Connecting,
            client_version: None,
            head_hash: None,
            total_difficulty: None,
            last_seen: now,
            connected_at: now,
        }
    }

    /// Update last seen time
    pub fn touch(&mut self) {
        self.last_seen = Instant::now();
    }

    /// Check if peer is connected
    pub fn is_connected(&self) -> bool {
        self.state == PeerState::Connected
    }
}

/// Manages connected peers
#[derive(Debug)]
pub struct PeerManager {
    /// Connected peers
    peers: RwLock<HashMap<PeerId, PeerInfo>>,
    /// Maximum number of peers
    max_peers: usize,
}

impl PeerManager {
    /// Create new peer manager
    pub fn new(max_peers: usize) -> Self {
        Self {
            peers: RwLock::new(HashMap::new()),
            max_peers,
        }
    }

    /// Add a new peer
    pub fn add_peer(&self, id: PeerId, addr: SocketAddr) -> bool {
        let mut peers = self.peers.write();
        if peers.len() >= self.max_peers {
            return false;
        }
        peers.insert(id, PeerInfo::new(id, addr));
        true
    }

    /// Remove a peer
    pub fn remove_peer(&self, id: &PeerId) -> Option<PeerInfo> {
        self.peers.write().remove(id)
    }

    /// Get peer info
    pub fn get_peer(&self, id: &PeerId) -> Option<PeerInfo> {
        self.peers.read().get(id).cloned()
    }

    /// Update peer state
    pub fn update_peer_state(&self, id: &PeerId, state: PeerState) {
        if let Some(peer) = self.peers.write().get_mut(id) {
            peer.state = state;
            peer.touch();
        }
    }

    /// Update peer head
    pub fn update_peer_head(&self, id: &PeerId, head_hash: B256, td: u128) {
        if let Some(peer) = self.peers.write().get_mut(id) {
            peer.head_hash = Some(head_hash);
            peer.total_difficulty = Some(td);
            peer.touch();
        }
    }

    /// Set peer client version
    pub fn set_client_version(&self, id: &PeerId, version: String) {
        if let Some(peer) = self.peers.write().get_mut(id) {
            peer.client_version = Some(version);
        }
    }

    /// Get all connected peers
    pub fn connected_peers(&self) -> Vec<PeerInfo> {
        self.peers
            .read()
            .values()
            .filter(|p| p.is_connected())
            .cloned()
            .collect()
    }

    /// Get peer count
    pub fn peer_count(&self) -> usize {
        self.peers.read().len()
    }

    /// Get connected peer count
    pub fn connected_count(&self) -> usize {
        self.peers
            .read()
            .values()
            .filter(|p| p.is_connected())
            .count()
    }

    /// Check if we can accept more peers
    pub fn can_accept_peer(&self) -> bool {
        self.peer_count() < self.max_peers
    }

    /// Get all peer IDs
    pub fn peer_ids(&self) -> Vec<PeerId> {
        self.peers.read().keys().cloned().collect()
    }
}

impl Default for PeerManager {
    fn default() -> Self {
        Self::new(50)
    }
}

/// Shared peer manager
pub type SharedPeerManager = Arc<PeerManager>;

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::B512;

    fn test_peer_id() -> PeerId {
        PeerId::from(B512::repeat_byte(1))
    }

    #[test]
    fn test_peer_manager() {
        let manager = PeerManager::new(10);
        let id = test_peer_id();
        let addr: SocketAddr = "127.0.0.1:30303".parse().unwrap();

        assert!(manager.add_peer(id, addr));
        assert_eq!(manager.peer_count(), 1);

        manager.update_peer_state(&id, PeerState::Connected);
        assert_eq!(manager.connected_count(), 1);

        manager.remove_peer(&id);
        assert_eq!(manager.peer_count(), 0);
    }

    #[test]
    fn test_max_peers() {
        let manager = PeerManager::new(2);
        let addr: SocketAddr = "127.0.0.1:30303".parse().unwrap();

        let id1 = PeerId::from(B512::repeat_byte(1));
        let id2 = PeerId::from(B512::repeat_byte(2));
        let id3 = PeerId::from(B512::repeat_byte(3));

        assert!(manager.add_peer(id1, addr));
        assert!(manager.add_peer(id2, addr));
        assert!(!manager.add_peer(id3, addr)); // Should fail - max reached
    }
}
