# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Project Overview

dex-reth is a dual virtual machine blockchain system that runs two VMs in a single node:
- **EVM (Ethereum Virtual Machine)**: Standard smart contract execution
- **DexVM (Custom VM)**: Simple counter state management

The system computes separate state roots for each VM, then combines them: `keccak256(evm_root || dexvm_root)`.

## Build Commands

```bash
# Build the project
cargo build --release

# Run the node
cargo run --release --bin dex-reth -- --help

# Run tests
cargo test

# Format code
cargo +nightly fmt --all

# Lint
cargo clippy --all-features
```

## Running the Node

```bash
# Basic run (RPC only)
cargo run --release --bin dex-reth -- --datadir ./data

# With POA consensus enabled
cargo run --release --bin dex-reth -- \
    --datadir ./data \
    --enable-consensus \
    --validator 0x0000000000000000000000000000000000000001 \
    --block-interval-ms 500

# With P2P networking enabled
cargo run --release --bin dex-reth -- \
    --datadir ./data \
    --enable-consensus \
    --enable-p2p \
    --p2p-port 30303

# With custom genesis file
cargo run --release --bin dex-reth -- \
    --genesis genesis.json \
    --datadir ./data \
    --enable-consensus
```

## Architecture

### Crate Structure

```
crates/
├── primitives/     # Core types (DualVmTransaction, DexVmReceipt)
├── dexvm/          # DexVM implementation (state.rs, executor.rs, precompiles.rs)
├── storage/        # MDBX database tables and stores
├── rpc/            # REST API (Axum) + JSON-RPC (jsonrpsee)
├── p2p/            # P2P networking (eth devp2p protocol)
└── node/           # Node integration (DualVmNode, POA consensus)

bin/dex-reth/
└── main.rs         # CLI entry point
```

### Key Components

| Crate | Purpose |
|-------|---------|
| `dex-primitives` | Transaction routing, receipt types, constants |
| `dex-dexvm` | DexVM state machine and precompile contracts |
| `dex-storage` | MDBX-based block and state storage |
| `dex-rpc` | DexVM REST API + Ethereum JSON-RPC |
| `dex-p2p` | P2P networking with eth protocol support |
| `dex-node` | Node orchestration, POA consensus, dual executor |

### Transaction Routing

Transactions are routed based on the `to` address:
- Address `0xddddddddddddddddddddddddddddddddddddddd1` → DexVM
- All other addresses → EVM

### DexVM Calldata Format

```
[op_type: u8][amount: u64 big-endian]
```
- `0` = Increment
- `1` = Decrement
- `2` = Query

### Precompile Contract

Counter precompile at `0x0000000000000000000000000000000000000100`:

Calldata format: `[op: 1 byte][amount: 8 bytes big-endian]`
- `0x00` + amount = Increment counter
- `0x01` + amount = Decrement counter
- `0x02` + padding = Query counter

### State Root Calculation

- EVM: `keccak256(sorted_account_data)`
- DexVM: `keccak256(sorted_counter_data)`
- Combined: `keccak256(evm_root || dexvm_root)`

## API Endpoints

| Port | Service | Protocol |
|------|---------|----------|
| 8545 | EVM RPC | JSON-RPC |
| 9845 | DexVM API | REST |
| 30303 | P2P | devp2p |

### DexVM REST API

```bash
# Health check
GET /health

# Query counter
GET /api/v1/counter/:address

# Increment counter
POST /api/v1/counter/:address/increment
Body: {"amount": 10}

# Decrement counter
POST /api/v1/counter/:address/decrement
Body: {"amount": 5}

# Get state root
GET /api/v1/state-root
```

### EVM JSON-RPC

Standard Ethereum JSON-RPC methods:
- `eth_chainId`, `eth_blockNumber`
- `eth_getBalance`, `eth_getTransactionCount`
- `eth_sendRawTransaction`
- `eth_getBlockByNumber`, `eth_getBlockByHash`
- `eth_getTransactionReceipt`
- `web3_clientVersion`, `net_version`

## Genesis File Format

```json
{
  "config": {
    "chainId": 13337
  },
  "alloc": {
    "0x1111111111111111111111111111111111111111": {
      "balance": "1000000000000000000000"
    }
  }
}
```

## Database

Uses MDBX with custom tables:
- `DualvmBlocks`: Block headers
- `DualvmAccounts`: EVM account state
- `DualvmCounters`: DexVM counter state
- `DualvmStorage`: Contract storage
- `DualvmTxHashes`: Transaction lookup index

## Development Notes

- POA consensus: single validator, configurable block interval (default 500ms)
- Data persists to `./data` directory by default
- All reth dependencies pinned to `v1.5.1`
- Alloy dependencies use `v1.x` (compatible with reth v1.5.1)
- Rust minimum version: 1.84
- P2P uses Ethereum devp2p protocol for peer discovery and communication
