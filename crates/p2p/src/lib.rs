//! DexVM P2P networking module
//!
//! This module provides P2P networking functionality using the Ethereum devp2p protocol.
//!
//! # Features
//!
//! - Peer discovery and management
//! - Eth protocol message handling
//! - Transaction propagation
//! - Block announcement
//!
//! # Example
//!
//! ```ignore
//! use dex_p2p::{P2pConfig, P2pService};
//!
//! let config = P2pConfig::default()
//!     .with_port(30303)
//!     .with_max_peers(50);
//!
//! let service = P2pService::new(config);
//! let handle = service.start().await?;
//!
//! // Subscribe to events
//! let mut events = handle.subscribe();
//! while let Ok(event) = events.recv().await {
//!     println!("P2P event: {:?}", event);
//! }
//! ```

pub mod config;
pub mod peer;
pub mod service;

pub use config::{P2pConfig, DEFAULT_P2P_PORT};
pub use peer::{PeerInfo, PeerManager, PeerState, SharedPeerManager};
pub use service::{P2pEvent, P2pHandle, P2pService, P2pServiceBuilder};

/// Re-export reth network peer types
pub use reth_network_peers::{pk2id, PeerId, TrustedPeer};
