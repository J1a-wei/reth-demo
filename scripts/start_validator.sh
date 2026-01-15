#!/bin/bash
# Start Validator Node with P2P
# 启动验证者节点（带P2P）

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# Default configuration
DATADIR="${DATADIR:-$PROJECT_DIR/data/validator}"
GENESIS="${GENESIS:-$PROJECT_DIR/genesis.json}"
EVM_RPC_PORT="${EVM_RPC_PORT:-8545}"
DEXVM_PORT="${DEXVM_PORT:-9845}"
P2P_PORT="${P2P_PORT:-30303}"
BLOCK_INTERVAL="${BLOCK_INTERVAL:-500}"
VALIDATOR="${VALIDATOR:-0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266}"

echo "=========================================="
echo "Starting Validator Node"
echo "=========================================="
echo "Data Directory: $DATADIR"
echo "Genesis File: $GENESIS"
echo "EVM RPC Port: $EVM_RPC_PORT"
echo "DexVM Port: $DEXVM_PORT"
echo "P2P Port: $P2P_PORT"
echo "Block Interval: ${BLOCK_INTERVAL}ms"
echo "Validator Address: $VALIDATOR"
echo "=========================================="
echo ""

# Create data directory if it doesn't exist
mkdir -p "$DATADIR"

# Build the project if needed
echo "Building project..."
cd "$PROJECT_DIR"
cargo build --release

echo ""
echo "Starting node..."
echo ""
echo "Note: Copy the 'Enode URL' from the logs below to connect full nodes"
echo ""

# Start the node with P2P enabled
exec cargo run --release --bin dex-reth -- \
    --datadir "$DATADIR" \
    --genesis "$GENESIS" \
    --enable-consensus \
    --validator "$VALIDATOR" \
    --block-interval-ms "$BLOCK_INTERVAL" \
    --evm-rpc-port "$EVM_RPC_PORT" \
    --dexvm-port "$DEXVM_PORT" \
    --enable-p2p \
    --p2p-port "$P2P_PORT"
