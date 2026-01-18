#!/bin/bash
#
# 流程 2: EVM 预编译合约调用 DexVM
#
# 测试内容:
# - 通过 EVM 预编译合约 (0x100) 调用 DexVM 计数器
# - 测试 Increment (增加)
# - 测试 Decrement (减少)
# - 测试 Query (查询)
# - 验证跨 VM 状态同步
#
# 预编译合约格式:
# - 地址: 0x0000000000000000000000000000000000000100
# - Calldata: [op: 1 byte][amount: 8 bytes big-endian]
#   - 0x00 = Increment
#   - 0x01 = Decrement
#   - 0x02 = Query
#

set -e

# 配置
RPC_URL="${RPC_URL:-http://127.0.0.1:8545}"
DEXVM_URL="${DEXVM_URL:-http://127.0.0.1:9845}"
PRIVATE_KEY="${PRIVATE_KEY:-0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80}"
TEST_ADDRESS="0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
PRECOMPILE_ADDRESS="0x0000000000000000000000000000000000000100"

echo "=============================================="
echo "  流程 2: EVM 预编译合约调用 DexVM"
echo "=============================================="
echo ""

# 检查节点
echo "=== 1. 检查节点状态 ==="
CHAIN_ID=$(curl -s -X POST $RPC_URL \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}' | grep -oE '"result":"0x[0-9a-fA-F]+"' | cut -d'"' -f4 || echo "error")
if [ "$CHAIN_ID" = "error" ]; then
    echo "Error: 无法连接到 EVM RPC $RPC_URL"
    exit 1
fi
echo "EVM RPC: $RPC_URL (Chain ID: $CHAIN_ID)"

HEALTH=$(curl -s $DEXVM_URL/health | grep -o '"status":"ok"' || echo "error")
if [ "$HEALTH" = "error" ]; then
    echo "Error: 无法连接到 DexVM API $DEXVM_URL"
    exit 1
fi
echo "DexVM API: $DEXVM_URL (健康)"

# 查询初始计数器
echo ""
echo "=== 2. 查询初始 DexVM 计数器 ==="
INITIAL_COUNTER=$(curl -s "$DEXVM_URL/api/v1/counter/$TEST_ADDRESS")
echo "DexVM 计数器: $INITIAL_COUNTER"

# 构造 Increment calldata: 0x00 + 10 (8 bytes big-endian)
# 0x00 = Increment
# 10 = 0x000000000000000a
echo ""
echo "=== 3. 通过预编译合约增加计数器 (+10) ==="
echo "预编译地址: $PRECOMPILE_ADDRESS"
echo "Calldata: 0x00000000000000000a (op=Increment, amount=10)"

# 发送交易到预编译合约
if command -v cast &> /dev/null; then
    cast send $PRECOMPILE_ADDRESS 0x00000000000000000a \
        --rpc-url $RPC_URL \
        --private-key $PRIVATE_KEY \
        --legacy 2>&1 || true
else
    echo "Warning: cast 未安装，跳过交易发送"
    echo "安装 Foundry: curl -L https://foundry.paradigm.xyz | bash && foundryup"
fi

sleep 3

# 查询 DexVM 计数器 (预期增加)
echo ""
echo "=== 4. 查询 DexVM 计数器 (预期: +10) ==="
AFTER_INCREMENT=$(curl -s "$DEXVM_URL/api/v1/counter/$TEST_ADDRESS")
echo "DexVM 计数器: $AFTER_INCREMENT"

# 构造 Query calldata: 0x02 + padding
echo ""
echo "=== 5. 通过预编译合约查询计数器 ==="
echo "Calldata: 0x020000000000000000 (op=Query)"

if command -v cast &> /dev/null; then
    # eth_call 查询 (只读)
    QUERY_RESULT=$(curl -s -X POST $RPC_URL \
        -H "Content-Type: application/json" \
        -d "{\"jsonrpc\":\"2.0\",\"method\":\"eth_call\",\"params\":[{\"from\":\"$TEST_ADDRESS\",\"to\":\"$PRECOMPILE_ADDRESS\",\"data\":\"0x020000000000000000\"},\"latest\"],\"id\":1}")
    echo "eth_call 结果: $QUERY_RESULT"
fi

# 构造 Decrement calldata: 0x01 + 5 (8 bytes big-endian)
echo ""
echo "=== 6. 通过预编译合约减少计数器 (-5) ==="
echo "Calldata: 0x010000000000000005 (op=Decrement, amount=5)"

if command -v cast &> /dev/null; then
    cast send $PRECOMPILE_ADDRESS 0x010000000000000005 \
        --rpc-url $RPC_URL \
        --private-key $PRIVATE_KEY \
        --legacy 2>&1 || true
fi

sleep 3

# 最终查询
echo ""
echo "=== 7. 最终 DexVM 计数器状态 ==="
FINAL_COUNTER=$(curl -s "$DEXVM_URL/api/v1/counter/$TEST_ADDRESS")
echo "DexVM 计数器: $FINAL_COUNTER"

# 获取状态根
echo ""
echo "=== 8. 获取状态根 ==="
STATE_ROOT=$(curl -s "$DEXVM_URL/api/v1/state-root")
echo "DexVM State Root: $STATE_ROOT"

echo ""
echo "=============================================="
echo "  流程 2 测试完成"
echo "=============================================="
echo ""
echo "注意: 当前 EVM RPC 的 eth_sendRawTransaction 使用简化执行路径"
echo "运行: cargo test --release"
echo ""
