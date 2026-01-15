#!/bin/bash
# End-to-End Test Script
# 端到端测试脚本

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

EVM_RPC_URL="${EVM_RPC_URL:-http://localhost:8545}"
DEXVM_URL="${DEXVM_URL:-http://localhost:9845}"

echo "=========================================="
echo "End-to-End Test Suite"
echo "=========================================="
echo "EVM RPC URL: $EVM_RPC_URL"
echo "DexVM URL: $DEXVM_URL"
echo "=========================================="
echo ""

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

pass_count=0
fail_count=0

# Helper function to check test result
check_result() {
    local test_name=$1
    local result=$2
    local expected=$3

    if [ "$result" = "$expected" ]; then
        echo -e "  ${GREEN}PASS${NC}: $test_name"
        ((pass_count++))
    else
        echo -e "  ${RED}FAIL${NC}: $test_name"
        echo "       Expected: $expected"
        echo "       Got: $result"
        ((fail_count++))
    fi
}

# Helper function for JSON-RPC calls
rpc_call() {
    local method=$1
    local params=$2
    curl -s -X POST "$EVM_RPC_URL" \
        -H "Content-Type: application/json" \
        -d "{\"jsonrpc\":\"2.0\",\"method\":\"$method\",\"params\":$params,\"id\":1}" 2>/dev/null
}

echo "=== Phase 1: Connection Tests ==="
echo ""

# Test 1: EVM RPC Connection
echo "Testing EVM RPC connection..."
EVM_RESULT=$(rpc_call "web3_clientVersion" "[]" 2>/dev/null)
if echo "$EVM_RESULT" | grep -q "result"; then
    echo -e "  ${GREEN}PASS${NC}: EVM RPC is accessible"
    ((pass_count++))
else
    echo -e "  ${RED}FAIL${NC}: EVM RPC is not accessible"
    echo "       Please start the node first"
    ((fail_count++))
fi

# Test 2: DexVM Connection
echo "Testing DexVM connection..."
DEXVM_RESULT=$(curl -s "$DEXVM_URL/health" 2>/dev/null)
if [ -n "$DEXVM_RESULT" ]; then
    echo -e "  ${GREEN}PASS${NC}: DexVM API is accessible"
    ((pass_count++))
else
    echo -e "  ${RED}FAIL${NC}: DexVM API is not accessible"
    ((fail_count++))
fi

echo ""
echo "=== Phase 2: EVM Basic Tests ==="
echo ""

# Test 3: Chain ID
echo "Testing chain ID..."
CHAIN_ID=$(rpc_call "eth_chainId" "[]" | jq -r '.result // ""')
if [ -n "$CHAIN_ID" ]; then
    echo -e "  ${GREEN}PASS${NC}: Chain ID returned: $CHAIN_ID"
    ((pass_count++))
else
    echo -e "  ${RED}FAIL${NC}: Chain ID not returned"
    ((fail_count++))
fi

# Test 4: Block Number
echo "Testing block number..."
BLOCK_NUM=$(rpc_call "eth_blockNumber" "[]" | jq -r '.result // ""')
if [ -n "$BLOCK_NUM" ]; then
    echo -e "  ${GREEN}PASS${NC}: Block number returned: $BLOCK_NUM"
    ((pass_count++))
else
    echo -e "  ${RED}FAIL${NC}: Block number not returned"
    ((fail_count++))
fi

# Test 5: Account Balance
echo "Testing account balance..."
ADDR="0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
BALANCE=$(rpc_call "eth_getBalance" "[\"$ADDR\", \"latest\"]" | jq -r '.result // ""')
if [ -n "$BALANCE" ]; then
    echo -e "  ${GREEN}PASS${NC}: Balance returned: $BALANCE"
    ((pass_count++))
else
    echo -e "  ${RED}FAIL${NC}: Balance not returned"
    ((fail_count++))
fi

echo ""
echo "=== Phase 3: DexVM Counter Tests ==="
echo ""

# Test 6: Counter Query
echo "Testing counter query..."
COUNTER=$(curl -s "$DEXVM_URL/api/v1/counter/$ADDR" 2>/dev/null)
if [ -n "$COUNTER" ]; then
    echo -e "  ${GREEN}PASS${NC}: Counter query successful"
    ((pass_count++))
else
    echo -e "  ${RED}FAIL${NC}: Counter query failed"
    ((fail_count++))
fi

# Test 7: Counter Increment
echo "Testing counter increment..."
INC_RESULT=$(curl -s -X POST "$DEXVM_URL/api/v1/counter/$ADDR/increment" \
    -H "Content-Type: application/json" \
    -d '{"amount": 5}' 2>/dev/null)
if [ -n "$INC_RESULT" ]; then
    echo -e "  ${GREEN}PASS${NC}: Counter increment successful"
    ((pass_count++))
else
    echo -e "  ${RED}FAIL${NC}: Counter increment failed"
    ((fail_count++))
fi

# Test 8: Counter Decrement
echo "Testing counter decrement..."
DEC_RESULT=$(curl -s -X POST "$DEXVM_URL/api/v1/counter/$ADDR/decrement" \
    -H "Content-Type: application/json" \
    -d '{"amount": 2}' 2>/dev/null)
if [ -n "$DEC_RESULT" ]; then
    echo -e "  ${GREEN}PASS${NC}: Counter decrement successful"
    ((pass_count++))
else
    echo -e "  ${RED}FAIL${NC}: Counter decrement failed"
    ((fail_count++))
fi

# Test 9: State Root
echo "Testing state root..."
STATE_ROOT=$(curl -s "$DEXVM_URL/api/v1/state-root" 2>/dev/null)
if [ -n "$STATE_ROOT" ]; then
    echo -e "  ${GREEN}PASS${NC}: State root query successful"
    ((pass_count++))
else
    echo -e "  ${RED}FAIL${NC}: State root query failed"
    ((fail_count++))
fi

echo ""
echo "=========================================="
echo "Test Summary"
echo "=========================================="
echo -e "  ${GREEN}Passed${NC}: $pass_count"
echo -e "  ${RED}Failed${NC}: $fail_count"
echo "  Total: $((pass_count + fail_count))"
echo "=========================================="

if [ $fail_count -gt 0 ]; then
    exit 1
fi
