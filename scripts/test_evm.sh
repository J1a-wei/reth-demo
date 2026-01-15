#!/bin/bash
# EVM JSON-RPC Test Script
# 测试 EVM JSON-RPC 功能

set -e

RPC_URL="${EVM_RPC_URL:-http://localhost:8545}"
ADDR="0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
ADDR2="0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
PRIVATE_KEY="0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"

echo "=========================================="
echo "EVM JSON-RPC Test"
echo "=========================================="
echo "RPC URL: $RPC_URL"
echo "Test Address: $ADDR"
echo ""

# Helper function for JSON-RPC calls
rpc_call() {
    local method=$1
    local params=$2
    curl -s -X POST "$RPC_URL" \
        -H "Content-Type: application/json" \
        -d "{\"jsonrpc\":\"2.0\",\"method\":\"$method\",\"params\":$params,\"id\":1}" 2>/dev/null
}

# Check if server is running
echo "1. Checking server connection..."
RESULT=$(rpc_call "web3_clientVersion" "[]")
if echo "$RESULT" | grep -q "error"; then
    echo "   ERROR: EVM RPC server is not running at $RPC_URL"
    echo "   Please start the node first with: cargo run --release --bin dex-reth"
    exit 1
fi
echo "   Client Version: $(echo $RESULT | jq -r '.result // "N/A"')"
echo ""

# Get chain ID
echo "2. Querying chain ID..."
CHAIN_ID=$(rpc_call "eth_chainId" "[]")
echo "   Chain ID: $(echo $CHAIN_ID | jq -r '.result // "N/A"')"
echo ""

# Get block number
echo "3. Querying block number..."
BLOCK_NUM=$(rpc_call "eth_blockNumber" "[]")
echo "   Block Number: $(echo $BLOCK_NUM | jq -r '.result // "N/A"')"
echo ""

# Get account balance
echo "4. Querying account balance..."
BALANCE=$(rpc_call "eth_getBalance" "[\"$ADDR\", \"latest\"]")
echo "   Balance of $ADDR:"
echo "   $(echo $BALANCE | jq -r '.result // "N/A"') wei"
echo ""

# Get transaction count (nonce)
echo "5. Querying transaction count (nonce)..."
NONCE=$(rpc_call "eth_getTransactionCount" "[\"$ADDR\", \"latest\"]")
echo "   Nonce: $(echo $NONCE | jq -r '.result // "N/A"')"
echo ""

# Get gas price
echo "6. Querying gas price..."
GAS_PRICE=$(rpc_call "eth_gasPrice" "[]")
echo "   Gas Price: $(echo $GAS_PRICE | jq -r '.result // "N/A"') wei"
echo ""

# Get net version
echo "7. Querying net version..."
NET_VERSION=$(rpc_call "net_version" "[]")
echo "   Net Version: $(echo $NET_VERSION | jq -r '.result // "N/A"')"
echo ""

# Get latest block
echo "8. Querying latest block..."
BLOCK=$(rpc_call "eth_getBlockByNumber" "[\"latest\", false]")
echo "   Latest Block Hash: $(echo $BLOCK | jq -r '.result.hash // "N/A"')"
echo "   Block Number: $(echo $BLOCK | jq -r '.result.number // "N/A"')"
echo "   Timestamp: $(echo $BLOCK | jq -r '.result.timestamp // "N/A"')"
echo ""

echo "=========================================="
echo "EVM JSON-RPC Test Complete"
echo "=========================================="
