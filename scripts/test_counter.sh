#!/bin/bash
# DexVM Counter Test Script
# 测试 DexVM 计数器功能

set -e

ADDR="0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
BASE_URL="${DEXVM_URL:-http://localhost:9845}"

echo "=========================================="
echo "DexVM Counter Test"
echo "=========================================="
echo "Target Address: $ADDR"
echo "DexVM URL: $BASE_URL"
echo ""

# Check if server is running
echo "1. Checking server health..."
if ! curl -s -f "$BASE_URL/health" > /dev/null 2>&1; then
    echo "   ERROR: DexVM server is not running at $BASE_URL"
    echo "   Please start the node first with: cargo run --release --bin dex-reth"
    exit 1
fi
echo "   OK: Server is healthy"
echo ""

# Query initial counter value
echo "2. Querying initial counter value..."
INITIAL=$(curl -s "$BASE_URL/api/v1/counter/$ADDR" 2>/dev/null || echo '{"error": "failed"}')
echo "   Response: $INITIAL"
echo ""

# Increment counter by 10
echo "3. Incrementing counter by 10..."
INC_RESULT=$(curl -s -X POST "$BASE_URL/api/v1/counter/$ADDR/increment" \
    -H "Content-Type: application/json" \
    -d '{"amount": 10}' 2>/dev/null || echo '{"error": "failed"}')
echo "   Response: $INC_RESULT"
echo ""

# Query updated value
echo "4. Querying updated counter value..."
UPDATED=$(curl -s "$BASE_URL/api/v1/counter/$ADDR" 2>/dev/null || echo '{"error": "failed"}')
echo "   Response: $UPDATED"
echo ""

# Decrement counter by 3
echo "5. Decrementing counter by 3..."
DEC_RESULT=$(curl -s -X POST "$BASE_URL/api/v1/counter/$ADDR/decrement" \
    -H "Content-Type: application/json" \
    -d '{"amount": 3}' 2>/dev/null || echo '{"error": "failed"}')
echo "   Response: $DEC_RESULT"
echo ""

# Query final value
echo "6. Querying final counter value..."
FINAL=$(curl -s "$BASE_URL/api/v1/counter/$ADDR" 2>/dev/null || echo '{"error": "failed"}')
echo "   Response: $FINAL"
echo ""

# Query state root
echo "7. Querying DexVM state root..."
STATE_ROOT=$(curl -s "$BASE_URL/api/v1/state-root" 2>/dev/null || echo '{"error": "failed"}')
echo "   Response: $STATE_ROOT"
echo ""

echo "=========================================="
echo "DexVM Counter Test Complete"
echo "=========================================="
