#!/bin/bash
# Start Full Node (Read-Only) with P2P connection to validator
# 启动全节点（只读）并通过P2P连接验证者

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# Default configuration
DATADIR="${DATADIR:-$PROJECT_DIR/data/fullnode}"
GENESIS="${GENESIS:-$PROJECT_DIR/genesis.json}"
EVM_RPC_PORT="${EVM_RPC_PORT:-8546}"
DEXVM_PORT="${DEXVM_PORT:-9846}"
P2P_PORT="${P2P_PORT:-30304}"
BOOTNODE="${BOOTNODE:-}"

echo "=========================================="
echo "Starting Full Node (Read-Only)"
echo "=========================================="
echo "Data Directory: $DATADIR"
echo "Genesis File: $GENESIS"
echo "EVM RPC Port: $EVM_RPC_PORT"
echo "DexVM Port: $DEXVM_PORT"
echo "P2P Port: $P2P_PORT"
if [ -n "$BOOTNODE" ]; then
    echo "Bootnode: $BOOTNODE"
else
    echo "Bootnode: (not specified)"
fi
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

# Construct bootnode argument if provided
BOOTNODE_ARG=""
if [ -n "$BOOTNODE" ]; then
    BOOTNODE_ARG="--bootnodes $BOOTNODE"
fi

# Start the node (without consensus - read only)
exec cargo run --release --bin dex-reth -- \
    --datadir "$DATADIR" \
    --genesis "$GENESIS" \
    --evm-rpc-port "$EVM_RPC_PORT" \
    --dexvm-port "$DEXVM_PORT" \
    --enable-p2p \
    --p2p-port "$P2P_PORT" \
    $BOOTNODE_ARG
