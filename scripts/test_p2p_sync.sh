#!/bin/bash
# Test P2P Sync between Validator and Fullnode
# 测试验证者和全节点之间的P2P同步

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Test configuration
VALIDATOR_DATADIR="/tmp/dex-p2p-test/validator"
FULLNODE_DATADIR="/tmp/dex-p2p-test/fullnode"
VALIDATOR_P2P_PORT=30303
FULLNODE_P2P_PORT=30304
VALIDATOR_RPC_PORT=8545
FULLNODE_RPC_PORT=8546
BLOCK_INTERVAL=2000
TEST_DURATION=20

# Hardhat test account #1
VALIDATOR_KEY="ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"

echo -e "${YELLOW}=========================================="
echo "P2P Sync Test: Validator <-> Fullnode"
echo -e "==========================================${NC}"
echo ""

# Cleanup function
cleanup() {
    echo ""
    echo -e "${YELLOW}Cleaning up...${NC}"

    # Kill background processes
    if [ -n "$VALIDATOR_PID" ]; then
        kill $VALIDATOR_PID 2>/dev/null || true
        wait $VALIDATOR_PID 2>/dev/null || true
    fi
    if [ -n "$FULLNODE_PID" ]; then
        kill $FULLNODE_PID 2>/dev/null || true
        wait $FULLNODE_PID 2>/dev/null || true
    fi

    # Clean up data directories
    rm -rf /tmp/dex-p2p-test

    echo -e "${GREEN}Cleanup complete${NC}"
}

# Set trap for cleanup
trap cleanup EXIT INT TERM

# Kill any existing dex-reth processes
echo "Killing any existing dex-reth processes..."
pkill -f "dex-reth" 2>/dev/null || true
sleep 1

# Create directories
echo "Creating test directories..."
rm -rf /tmp/dex-p2p-test
mkdir -p "$VALIDATOR_DATADIR" "$FULLNODE_DATADIR"

# Build the project
echo "Building project..."
cd "$PROJECT_DIR"
cargo build --release 2>&1 | tail -3

BINARY="$PROJECT_DIR/target/release/dex-reth"

echo ""
echo -e "${GREEN}Step 1: Starting Validator Node${NC}"
echo "  - P2P Port: $VALIDATOR_P2P_PORT"
echo "  - RPC Port: $VALIDATOR_RPC_PORT"
echo "  - Block Interval: ${BLOCK_INTERVAL}ms"
echo ""

# Start validator node and capture output
VALIDATOR_LOG="/tmp/dex-p2p-test/validator.log"
$BINARY \
    --datadir "$VALIDATOR_DATADIR" \
    --enable-consensus \
    --validator-key "$VALIDATOR_KEY" \
    --block-interval-ms "$BLOCK_INTERVAL" \
    --evm-rpc-port "$VALIDATOR_RPC_PORT" \
    --dexvm-port 9845 \
    --p2p-port "$VALIDATOR_P2P_PORT" \
    --log-level info \
    > "$VALIDATOR_LOG" 2>&1 &
VALIDATOR_PID=$!

echo "Validator started with PID: $VALIDATOR_PID"

# Wait for validator to start and extract enode URL
echo "Waiting for validator to start..."
sleep 3

# Extract enode URL from logs
ENODE_URL=""
for i in {1..10}; do
    ENODE_URL=$(grep -o "enode://[a-f0-9]*@127.0.0.1:$VALIDATOR_P2P_PORT" "$VALIDATOR_LOG" 2>/dev/null || true)
    if [ -n "$ENODE_URL" ]; then
        break
    fi
    sleep 1
done

if [ -z "$ENODE_URL" ]; then
    echo -e "${RED}ERROR: Could not extract enode URL from validator logs${NC}"
    echo "Validator log:"
    cat "$VALIDATOR_LOG"
    exit 1
fi

echo -e "${GREEN}Validator enode URL: $ENODE_URL${NC}"
echo ""

# Wait for a few blocks to be produced
echo "Waiting for validator to produce some blocks..."
sleep 5

# Check validator block number
VALIDATOR_BLOCK=$(curl -s -X POST -H "Content-Type: application/json" \
    --data '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' \
    http://127.0.0.1:$VALIDATOR_RPC_PORT 2>/dev/null | grep -o '"result":"0x[0-9a-f]*"' | cut -d'"' -f4 || echo "0x0")
VALIDATOR_BLOCK_DEC=$((VALIDATOR_BLOCK))

echo "Validator current block: $VALIDATOR_BLOCK_DEC"
echo ""

echo -e "${GREEN}Step 2: Starting Fullnode${NC}"
echo "  - P2P Port: $FULLNODE_P2P_PORT"
echo "  - RPC Port: $FULLNODE_RPC_PORT"
echo "  - Bootnode: $ENODE_URL"
echo ""

# Start fullnode with bootnode
FULLNODE_LOG="/tmp/dex-p2p-test/fullnode.log"
$BINARY \
    --datadir "$FULLNODE_DATADIR" \
    --evm-rpc-port "$FULLNODE_RPC_PORT" \
    --dexvm-port 9846 \
    --p2p-port "$FULLNODE_P2P_PORT" \
    --bootnodes "$ENODE_URL" \
    --log-level info \
    > "$FULLNODE_LOG" 2>&1 &
FULLNODE_PID=$!

echo "Fullnode started with PID: $FULLNODE_PID"

# Wait for connection and sync
echo ""
echo -e "${GREEN}Step 3: Waiting for P2P connection and block sync${NC}"
echo ""

# Monitor sync progress
SYNC_SUCCESS=false
for i in $(seq 1 $TEST_DURATION); do
    sleep 1

    # Get current block numbers
    VALIDATOR_BLOCK=$(curl -s -X POST -H "Content-Type: application/json" \
        --data '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' \
        http://127.0.0.1:$VALIDATOR_RPC_PORT 2>/dev/null | grep -o '"result":"0x[0-9a-f]*"' | cut -d'"' -f4 || echo "0x0")

    FULLNODE_BLOCK=$(curl -s -X POST -H "Content-Type: application/json" \
        --data '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' \
        http://127.0.0.1:$FULLNODE_RPC_PORT 2>/dev/null | grep -o '"result":"0x[0-9a-f]*"' | cut -d'"' -f4 || echo "0x0")

    VALIDATOR_BLOCK_DEC=$((VALIDATOR_BLOCK))
    FULLNODE_BLOCK_DEC=$((FULLNODE_BLOCK))

    echo "  [$i/${TEST_DURATION}s] Validator: block $VALIDATOR_BLOCK_DEC | Fullnode: block $FULLNODE_BLOCK_DEC"

    # Check if fullnode has synced (within 4 blocks of validator is acceptable)
    if [ "$FULLNODE_BLOCK_DEC" -gt 0 ] && [ "$FULLNODE_BLOCK_DEC" -ge $((VALIDATOR_BLOCK_DEC - 4)) ]; then
        SYNC_SUCCESS=true
        echo ""
        echo -e "${GREEN}Sync successful! Fullnode is syncing with validator.${NC}"
        break
    fi
done

echo ""
echo "=========================================="
echo "Test Results"
echo "=========================================="

# Check peer connection
echo ""
echo "Checking P2P connection logs..."

PEER_CONNECTED_VALIDATOR=$(grep "Peer connected" "$VALIDATOR_LOG" 2>/dev/null | wc -l)
PEER_CONNECTED_FULLNODE=$(grep "Peer connected" "$FULLNODE_LOG" 2>/dev/null | wc -l)

echo "  Validator peer connections: $PEER_CONNECTED_VALIDATOR"
echo "  Fullnode peer connections: $PEER_CONNECTED_FULLNODE"

# Check block sync logs
BLOCKS_SYNCED=$(grep "Synced block" "$FULLNODE_LOG" 2>/dev/null | wc -l)
NEW_BLOCK_HASHES=$(grep "NewBlockHash" "$FULLNODE_LOG" 2>/dev/null | wc -l)
echo "  Blocks synced by fullnode: $BLOCKS_SYNCED"
echo "  NewBlockHash events received: $NEW_BLOCK_HASHES"

echo ""
if [ "$SYNC_SUCCESS" = true ]; then
    echo -e "${GREEN}=========================================="
    echo "TEST PASSED: P2P sync working correctly!"
    echo -e "==========================================${NC}"

    # Show final state
    echo ""
    echo "Final State:"
    echo "  - Validator block: $VALIDATOR_BLOCK_DEC"
    echo "  - Fullnode block: $FULLNODE_BLOCK_DEC"

    exit 0
else
    echo -e "${RED}=========================================="
    echo "TEST FAILED: P2P sync did not complete"
    echo -e "==========================================${NC}"

    echo ""
    echo "Validator log (last 30 lines):"
    tail -30 "$VALIDATOR_LOG"

    echo ""
    echo "Fullnode log (last 30 lines):"
    tail -30 "$FULLNODE_LOG"

    exit 1
fi
