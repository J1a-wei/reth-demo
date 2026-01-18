# P2P Block Sync Implementation Summary

## Overview

Implemented complete ETH devp2p protocol support for block synchronization between validator and fullnode in the dex-reth dual-VM blockchain.

## Architecture

```
┌─────────────────┐                    ┌─────────────────┐
│    Validator    │                    │    Fullnode     │
│                 │                    │                 │
│  ┌───────────┐  │   ECIES + P2P +    │  ┌───────────┐  │
│  │ Consensus │  │   ETH Status       │  │   Sync    │  │
│  │  Engine   │  │◄──────────────────►│  │  Manager  │  │
│  └───────────┘  │                    │  └───────────┘  │
│       │         │                    │       │         │
│       ▼         │                    │       ▼         │
│  ┌───────────┐  │  NewBlockHashes    │  ┌───────────┐  │
│  │   Block   │  │ ─────────────────► │  │   Block   │  │
│  │   Store   │  │                    │  │   Store   │  │
│  └───────────┘  │  GetBlockHeaders   │  └───────────┘  │
│       │         │ ◄───────────────── │       ▲         │
│       │         │                    │       │         │
│       │         │  BlockHeaders      │       │         │
│       └─────────┼──────────────────► │───────┘         │
│                 │                    │                 │
│                 │  GetBlockBodies    │                 │
│                 │ ◄───────────────── │                 │
│                 │                    │                 │
│                 │  BlockBodies       │                 │
│                 │ ─────────────────► │                 │
└─────────────────┘                    └─────────────────┘
```

## Implementation Details

### 1. P2P Connection Handshake

Connection establishment follows the Ethereum devp2p protocol:

1. **ECIES Handshake**: Encrypted channel setup using secp256k1 keys
2. **P2P Hello**: Exchange client info and capabilities (eth/68)
3. **ETH Status**: Exchange chain info (chain_id, genesis_hash, total_difficulty)

```rust
// Session establishment flow
async fn connect_outbound(addr, remote_id, config) -> Session {
    // 1. TCP connect
    let stream = TcpStream::connect(addr).await?;

    // 2. ECIES handshake
    let ecies_stream = ECIESStream::connect(stream, secret_key, remote_id).await?;

    // 3. P2P handshake
    let (p2p_stream, their_hello) = UnauthedP2PStream::new(ecies_stream)
        .handshake(our_hello).await?;

    // 4. ETH Status handshake
    let their_status = eth_status_handshake(&mut p2p_stream, our_status).await?;

    Ok(Session { stream: p2p_stream, peer_id })
}
```

### 2. Block Broadcasting (Validator)

When validator produces a new block:

```rust
// After block finalization
let cmd = SessionCommand::BroadcastBlock { hash: block_hash, number: block_number };
p2p_handle.send_command(cmd).await?;

// Broadcasts NewBlockHashes to all connected peers
EthMessage::NewBlockHashes(vec![BlockHashNumber { hash, number }])
```

### 3. Block Sync (Fullnode)

The `BlockSyncManager` handles synchronization:

```rust
struct BlockSyncManager {
    p2p_handle: P2pHandle,
    block_store: Arc<BlockStore>,
    pending_header_requests: HashSet<u64>,
    pending_body_requests: HashMap<u64, ConsensusHeader>,
}

impl BlockSyncManager {
    // On NewBlockHash event
    async fn handle_new_block_hash(&mut self, peer_id, hash, number) {
        // Check if we need this block
        if self.block_store.get_block_by_number(number).is_some() {
            return; // Already have it
        }

        // Request headers for missing blocks
        let our_latest = self.block_store.latest_block_number();
        let cmd = SessionCommand::GetBlockHeaders {
            peer_id,
            start: our_latest + 1,
            count: number - our_latest
        };
        self.p2p_handle.send_command(cmd).await?;
    }

    // On BlockHeaders response
    async fn handle_block_headers(&mut self, peer_id, headers) {
        // Store headers and request bodies
        let hashes: Vec<B256> = headers.iter()
            .map(|h| keccak256(alloy_rlp::encode(h)))
            .collect();

        let cmd = SessionCommand::GetBlockBodies { peer_id, hashes };
        self.p2p_handle.send_command(cmd).await?;
    }

    // On BlockBodies response
    fn handle_block_bodies(&mut self, bodies) {
        // Create StoredBlock from header + body and save
        for (header, body) in headers.zip(bodies) {
            let block = StoredBlock {
                number: header.number,
                hash: keccak256(alloy_rlp::encode(&header)),
                parent_hash: header.parent_hash,
                // ... other fields
            };
            self.block_store.store_block(block)?;
        }
    }
}
```

### 4. Request Handling (Validator)

Validator responds to block requests from fullnodes:

```rust
// Handle GetBlockHeadersRequest
P2pEvent::GetBlockHeadersRequest { peer_id, request_id, start, limit } => {
    let mut headers = Vec::new();
    for i in 0..limit {
        if let Some(block) = block_store.get_block_by_number(start - i) {
            let header = ConsensusHeader {
                number: block.number,
                parent_hash: block.parent_hash,
                state_root: block.combined_state_root,
                // ... convert StoredBlock to ConsensusHeader
            };
            headers.push(header);
        }
    }

    let cmd = SessionCommand::SendBlockHeaders { peer_id, request_id, headers };
    p2p_handle.send_command(cmd).await?;
}

// Handle GetBlockBodiesRequest
P2pEvent::GetBlockBodiesRequest { peer_id, request_id, hashes } => {
    // Send empty bodies (transactions not stored in current implementation)
    let bodies: Vec<BlockBody> = hashes.iter()
        .map(|_| BlockBody::default())
        .collect();

    let cmd = SessionCommand::SendBlockBodies { peer_id, request_id, bodies };
    p2p_handle.send_command(cmd).await?;
}
```

## ETH Protocol Messages

| Message | Direction | Description |
|---------|-----------|-------------|
| `NewBlockHashes` | Validator → Fullnode | Announce new blocks |
| `GetBlockHeaders` | Fullnode → Validator | Request block headers |
| `BlockHeaders` | Validator → Fullnode | Response with headers |
| `GetBlockBodies` | Fullnode → Validator | Request block bodies |
| `BlockBodies` | Validator → Fullnode | Response with bodies |

## Files Modified

### crates/p2p/

| File | Changes |
|------|---------|
| `src/service.rs` | Added `P2pEvent::{BlockHeaders, BlockBodies, GetBlockHeadersRequest, GetBlockBodiesRequest}`, `SessionCommand::{SendBlockHeaders, SendBlockBodies}` |
| `src/eth_handler.rs` | Added request/response event types and command handlers |
| `src/session.rs` | ECIES + P2P + ETH Status handshake implementation |
| `src/lib.rs` | Updated exports |
| `Cargo.toml` | Added reth-eth-wire, reth-ecies dependencies |

### bin/dex-reth/

| File | Changes |
|------|---------|
| `src/main.rs` | Added `BlockSyncManager`, `run_fullnode_sync()`, `run_validator_p2p_handler()` |
| `Cargo.toml` | Added alloy-consensus, reth-ethereum-primitives dependencies |

## Test Results

```
# Start validator (block interval: 2s)
./target/release/dex-reth --datadir /tmp/validator --enable-consensus --p2p-port 30303

# Start fullnode with bootnode
./target/release/dex-reth --datadir /tmp/fullnode --p2p-port 30304 \
    --bootnodes "enode://<validator_pubkey>@127.0.0.1:30303"

# Observed behavior:
# 1. P2P connection established (ECIES + P2P Hello + ETH Status)
# 2. Fullnode receives NewBlockHash from validator
# 3. Fullnode requests headers: "Requesting 39 block headers from peer..."
# 4. Validator responds: "Sending 39 headers to peer..."
# 5. Fullnode requests bodies: "Requesting 39 block bodies from peer..."
# 6. Fullnode syncs: "Synced block 1...2...3..."
# 7. Fullnode catches up to validator's block height
```

## Usage

### Running Validator Node

```bash
cargo run --release --bin dex-reth -- \
    --datadir ./data/validator \
    --enable-consensus \
    --validator 0x0000000000000000000000000000000000000001 \
    --block-interval-ms 2000 \
    --p2p-port 30303
```

### Running Fullnode

```bash
cargo run --release --bin dex-reth -- \
    --datadir ./data/fullnode \
    --p2p-port 30304 \
    --bootnodes "enode://<validator_pubkey>@<validator_ip>:30303"
```

## Limitations & Future Work

1. **Transaction sync**: Currently sends empty block bodies (transactions not stored)
2. **Batch optimization**: Could request larger batches of headers/bodies
3. **Parallel requests**: Could pipeline header and body requests
4. **State validation**: Fullnode trusts validator's state roots without re-execution
5. **Peer scoring**: No peer reputation or ban list implemented
6. **Discovery**: No peer discovery protocol (requires explicit bootnodes)

## Dependencies

```toml
# P2P crate dependencies
reth-eth-wire = { git = "https://github.com/paradigmxyz/reth.git", tag = "v1.5.1" }
reth-eth-wire-types = { git = "https://github.com/paradigmxyz/reth.git", tag = "v1.5.1" }
reth-ecies = { git = "https://github.com/paradigmxyz/reth.git", tag = "v1.5.1" }
reth-network-peers = { git = "https://github.com/paradigmxyz/reth.git", tag = "v1.5.1" }
alloy-consensus = "1.0"
alloy-rlp = "0.3"
```
