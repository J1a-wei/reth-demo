#!/bin/bash
# Start Validator Node with P2P
# 启动验证者节点（带P2P）
#
# P2P密钥固化在 validator_p2p.key 文件中
# 其他节点可以使用固定的enode URL连接
#

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# Default configuration
DATADIR="${DATADIR:-$PROJECT_DIR/data}"
GENESIS="${GENESIS:-$PROJECT_DIR/genesis.json}"
P2P_KEY_FILE="${P2P_KEY_FILE:-$PROJECT_DIR/validator_p2p.key}"
EVM_RPC_PORT="${EVM_RPC_PORT:-8545}"
DEXVM_PORT="${DEXVM_PORT:-9845}"
P2P_PORT="${P2P_PORT:-30303}"
BLOCK_INTERVAL="${BLOCK_INTERVAL:-2000}"
LOG_LEVEL="${LOG_LEVEL:-info}"
# Default: Hardhat test account #1 private key
VALIDATOR_KEY="${VALIDATOR_KEY:-ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80}"

echo "=========================================="
echo "Starting Validator Node"
echo "=========================================="
echo "Data Directory: $DATADIR"
echo "Genesis File: $GENESIS"
echo "P2P Key File: $P2P_KEY_FILE"
echo "EVM RPC Port: $EVM_RPC_PORT"
echo "DexVM Port: $DEXVM_PORT"
echo "P2P Port: $P2P_PORT"
echo "Block Interval: ${BLOCK_INTERVAL}ms"
echo "Log Level: $LOG_LEVEL"
echo "=========================================="
echo ""

cd "$PROJECT_DIR"

# Build the project if needed
if [ ! -f "./target/release/dex-reth" ]; then
    echo "Building project..."
    cargo build --release
fi

# Create data directory if it doesn't exist
mkdir -p "$DATADIR"

# Copy P2P key to data directory if exists
if [ -f "$P2P_KEY_FILE" ]; then
    cp "$P2P_KEY_FILE" "$DATADIR/p2p_key"
    chmod 600 "$DATADIR/p2p_key"
    echo "Using fixed P2P key from: $P2P_KEY_FILE"
else
    echo "Warning: P2P key file not found at $P2P_KEY_FILE"
    echo "A new P2P key will be generated."
    echo ""
    echo "To fix the P2P key, save it after first run:"
    echo "  cp $DATADIR/p2p_key $P2P_KEY_FILE"
fi

echo ""
echo "Starting node..."
echo ""

# Start the node
exec ./target/release/dex-reth \
    --datadir "$DATADIR" \
    --genesis "$GENESIS" \
    --enable-consensus \
    --validator-key "$VALIDATOR_KEY" \
    --block-interval-ms "$BLOCK_INTERVAL" \
    --evm-rpc-port "$EVM_RPC_PORT" \
    --dexvm-port "$DEXVM_PORT" \
    --p2p-port "$P2P_PORT" \
    --log-level "$LOG_LEVEL"
