#!/bin/bash
# Start Full Node with P2P connection to validator
# 启动全节点并通过P2P连接验证者
#
# 默认连接到验证者节点: 15.235.230.59:30303
#

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# Validator's fixed peer ID (derived from validator_p2p.key)
VALIDATOR_PEER_ID="c6ecdf9e2d5c7838b2787f71e533e0f97ed4d6dde57286884e16683603c4266bb800c67b4bc40cf68c44e0d750b1238306e16af0e16edc80dd24430eaf3d1253"
VALIDATOR_IP="${VALIDATOR_IP:-15.235.230.59}"
VALIDATOR_P2P_PORT="${VALIDATOR_P2P_PORT:-30303}"

# Default configuration
DATADIR="${DATADIR:-$PROJECT_DIR/data-fullnode}"
GENESIS="${GENESIS:-$PROJECT_DIR/genesis.json}"
EVM_RPC_PORT="${EVM_RPC_PORT:-8546}"
DEXVM_PORT="${DEXVM_PORT:-9846}"
P2P_PORT="${P2P_PORT:-30304}"
LOG_LEVEL="${LOG_LEVEL:-info}"

# Build bootnode URL
BOOTNODE="${BOOTNODE:-enode://${VALIDATOR_PEER_ID}@${VALIDATOR_IP}:${VALIDATOR_P2P_PORT}}"

echo "=========================================="
echo "Starting Full Node"
echo "=========================================="
echo "Data Directory: $DATADIR"
echo "Genesis File: $GENESIS"
echo "EVM RPC Port: $EVM_RPC_PORT"
echo "DexVM Port: $DEXVM_PORT"
echo "P2P Port: $P2P_PORT"
echo "Log Level: $LOG_LEVEL"
echo ""
echo "Connecting to Validator:"
echo "  IP: $VALIDATOR_IP"
echo "  P2P Port: $VALIDATOR_P2P_PORT"
echo "  Enode: $BOOTNODE"
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

echo ""
echo "Starting node..."
echo ""

# Start the node (without consensus - sync only, P2P enabled by default)
exec ./target/release/dex-reth \
    --datadir "$DATADIR" \
    --genesis "$GENESIS" \
    --evm-rpc-port "$EVM_RPC_PORT" \
    --dexvm-port "$DEXVM_PORT" \
    --p2p-port "$P2P_PORT" \
    --bootnodes "$BOOTNODE" \
    --log-level "$LOG_LEVEL"
