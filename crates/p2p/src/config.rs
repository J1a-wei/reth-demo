//! P2P configuration

use alloy_primitives::B256;
use reth_network_peers::TrustedPeer;
use secp256k1::SecretKey;
use std::{
    collections::HashSet,
    net::{IpAddr, Ipv4Addr, SocketAddr},
};

/// Default P2P port
pub const DEFAULT_P2P_PORT: u16 = 30303;

/// P2P network configuration
#[derive(Debug, Clone)]
pub struct P2pConfig {
    /// Node secret key for identity
    pub secret_key: SecretKey,
    /// Address to listen on
    pub listen_addr: SocketAddr,
    /// Chain ID
    pub chain_id: u64,
    /// Genesis block hash
    pub genesis_hash: B256,
    /// Boot nodes to connect to
    pub boot_nodes: HashSet<TrustedPeer>,
    /// Maximum number of peers
    pub max_peers: usize,
    /// Network ID (same as chain ID for custom networks)
    pub network_id: u64,
}

impl P2pConfig {
    /// Create new P2P config with secret key
    pub fn new(secret_key: SecretKey, chain_id: u64, genesis_hash: B256) -> Self {
        Self {
            secret_key,
            listen_addr: SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), DEFAULT_P2P_PORT),
            chain_id,
            genesis_hash,
            boot_nodes: HashSet::new(),
            max_peers: 50,
            network_id: chain_id,
        }
    }

    /// Generate random secret key
    pub fn random_secret_key() -> SecretKey {
        SecretKey::new(&mut rand::thread_rng())
    }

    /// Set listen address
    pub fn with_listen_addr(mut self, addr: SocketAddr) -> Self {
        self.listen_addr = addr;
        self
    }

    /// Set listen port
    pub fn with_port(mut self, port: u16) -> Self {
        self.listen_addr.set_port(port);
        self
    }

    /// Add boot node
    pub fn with_boot_node(mut self, node: TrustedPeer) -> Self {
        self.boot_nodes.insert(node);
        self
    }

    /// Set max peers
    pub fn with_max_peers(mut self, max: usize) -> Self {
        self.max_peers = max;
        self
    }
}

impl Default for P2pConfig {
    fn default() -> Self {
        Self::new(
            Self::random_secret_key(),
            1,
            B256::ZERO,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_creation() {
        let config = P2pConfig::default();
        assert_eq!(config.chain_id, 1);
        assert_eq!(config.listen_addr.port(), DEFAULT_P2P_PORT);
    }

    #[test]
    fn test_config_builder() {
        let config = P2pConfig::default()
            .with_port(30304)
            .with_max_peers(100);

        assert_eq!(config.listen_addr.port(), 30304);
        assert_eq!(config.max_peers, 100);
    }
}
