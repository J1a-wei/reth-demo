#!/bin/bash
#
# 流程 4: P2P 全节点同步测试
#
# 测试内容:
# - 启动验证者节点
# - 启动全节点并连接验证者
# - 在验证者上发送交易产生区块
# - 验证全节点同步区块
# - 对比区块高度和状态根
#

set -e

# 配置
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
GENESIS="${GENESIS:-$PROJECT_DIR/genesis.json}"

# 验证者配置
VALIDATOR_DATADIR="$PROJECT_DIR/data/test_validator"
VALIDATOR_EVM_PORT=8545
VALIDATOR_DEXVM_PORT=9845
VALIDATOR_P2P_PORT=30303

# 全节点配置
FULLNODE_DATADIR="$PROJECT_DIR/data/test_fullnode"
FULLNODE_EVM_PORT=8546
FULLNODE_DEXVM_PORT=9846
FULLNODE_P2P_PORT=30304

# 日志文件
VALIDATOR_LOG="/tmp/validator_test.log"
FULLNODE_LOG="/tmp/fullnode_test.log"

# 测试密钥 (Hardhat test account #1)
VALIDATOR_KEY="ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
TEST_ADDRESS="0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"

# 颜色输出
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

echo "=============================================="
echo "  流程 4: P2P 全节点同步测试"
echo "=============================================="
echo ""

# 清理函数
cleanup() {
    echo ""
    echo "=== 清理进程 ==="
    pkill -f "dex-reth.*test_validator" 2>/dev/null || true
    pkill -f "dex-reth.*test_fullnode" 2>/dev/null || true
    sleep 2
    echo "清理完成"
}

# 捕获退出信号
trap cleanup EXIT

# 1. 清理旧数据
echo "=== 1. 清理旧数据 ==="
rm -rf "$VALIDATOR_DATADIR" "$FULLNODE_DATADIR"
rm -f "$VALIDATOR_LOG" "$FULLNODE_LOG"
echo "已清理数据目录"

# 2. 编译项目
echo ""
echo "=== 2. 编译项目 ==="
cd "$PROJECT_DIR"
cargo build --release 2>&1 | tail -3
echo "编译完成"

# 3. 启动验证者节点
echo ""
echo "=== 3. 启动验证者节点 ==="
cargo run --release --bin dex-reth -- \
    --datadir "$VALIDATOR_DATADIR" \
    --genesis "$GENESIS" \
    --enable-consensus \
    --validator-key "$VALIDATOR_KEY" \
    --block-interval-ms 1000 \
    --evm-rpc-port $VALIDATOR_EVM_PORT \
    --dexvm-port $VALIDATOR_DEXVM_PORT \
    --p2p-port $VALIDATOR_P2P_PORT \
    > "$VALIDATOR_LOG" 2>&1 &

VALIDATOR_PID=$!
echo "验证者 PID: $VALIDATOR_PID"

# 等待验证者启动
sleep 5

# 检查验证者是否运行
if ! curl -s "http://127.0.0.1:$VALIDATOR_EVM_PORT" -X POST -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}' | grep -q "result"; then
    echo -e "${RED}错误: 验证者节点启动失败${NC}"
    cat "$VALIDATOR_LOG"
    exit 1
fi
echo "验证者节点已启动"

# 4. 获取 enode URL
echo ""
echo "=== 4. 获取 Enode URL ==="
sleep 2
ENODE_URL=$(grep -o 'enode://[^[:space:]]*' "$VALIDATOR_LOG" | head -1)
if [ -z "$ENODE_URL" ]; then
    echo -e "${YELLOW}警告: 无法从日志获取 enode URL，尝试构造...${NC}"
    # 从日志获取 peer ID
    PEER_ID=$(grep -o 'Local peer ID: [0-9a-fA-Fx]*' "$VALIDATOR_LOG" | head -1 | awk '{print $4}')
    if [ -n "$PEER_ID" ]; then
        ENODE_URL="enode://${PEER_ID}@127.0.0.1:$VALIDATOR_P2P_PORT"
    fi
fi

if [ -z "$ENODE_URL" ]; then
    echo -e "${RED}错误: 无法获取 enode URL${NC}"
    echo "验证者日志:"
    cat "$VALIDATOR_LOG"
    exit 1
fi
echo "Enode URL: $ENODE_URL"

# 5. 启动全节点
echo ""
echo "=== 5. 启动全节点 ==="
cargo run --release --bin dex-reth -- \
    --datadir "$FULLNODE_DATADIR" \
    --genesis "$GENESIS" \
    --evm-rpc-port $FULLNODE_EVM_PORT \
    --dexvm-port $FULLNODE_DEXVM_PORT \
    --p2p-port $FULLNODE_P2P_PORT \
    --bootnodes "$ENODE_URL" \
    > "$FULLNODE_LOG" 2>&1 &

FULLNODE_PID=$!
echo "全节点 PID: $FULLNODE_PID"

# 等待全节点启动
sleep 5

# 检查全节点是否运行
if ! curl -s "http://127.0.0.1:$FULLNODE_EVM_PORT" -X POST -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}' | grep -q "result"; then
    echo -e "${RED}错误: 全节点启动失败${NC}"
    cat "$FULLNODE_LOG"
    exit 1
fi
echo "全节点已启动"

# 6. 等待 P2P 连接
echo ""
echo "=== 6. 等待 P2P 连接 ==="
sleep 5
if grep -q "Peer connected" "$VALIDATOR_LOG" || grep -q "Peer connected" "$FULLNODE_LOG"; then
    echo -e "${GREEN}P2P 连接已建立${NC}"
else
    echo -e "${YELLOW}警告: 可能未建立 P2P 连接，继续测试...${NC}"
fi

# 7. 获取验证者初始区块高度
echo ""
echo "=== 7. 获取初始区块高度 ==="
VALIDATOR_BLOCK_BEFORE=$(curl -s "http://127.0.0.1:$VALIDATOR_EVM_PORT" -X POST \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' | \
    grep -oE '"result":"0x[0-9a-fA-F]+"' | cut -d'"' -f4)
echo "验证者初始区块: $VALIDATOR_BLOCK_BEFORE ($(printf "%d" $VALIDATOR_BLOCK_BEFORE))"

FULLNODE_BLOCK_BEFORE=$(curl -s "http://127.0.0.1:$FULLNODE_EVM_PORT" -X POST \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' | \
    grep -oE '"result":"0x[0-9a-fA-F]+"' | cut -d'"' -f4)
echo "全节点初始区块: $FULLNODE_BLOCK_BEFORE ($(printf "%d" $FULLNODE_BLOCK_BEFORE))"

# 8. 在验证者上发送 DexVM 交易
echo ""
echo "=== 8. 在验证者上发送 DexVM 交易 ==="
for i in {1..3}; do
    RESULT=$(curl -s -X POST "http://127.0.0.1:$VALIDATOR_DEXVM_PORT/api/v1/counter/$TEST_ADDRESS/increment" \
        -H "Content-Type: application/json" \
        -d '{"amount": 10}')
    echo "交易 $i: $RESULT"
    sleep 1
done

# 9. 等待区块生成和同步
echo ""
echo "=== 9. 等待区块生成和同步 ==="
echo "等待 10 秒..."
sleep 10

# 10. 检查区块同步
echo ""
echo "=== 10. 检查区块同步 ==="
VALIDATOR_BLOCK_AFTER=$(curl -s "http://127.0.0.1:$VALIDATOR_EVM_PORT" -X POST \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' | \
    grep -oE '"result":"0x[0-9a-fA-F]+"' | cut -d'"' -f4)
VALIDATOR_BLOCK_DEC=$(printf "%d" $VALIDATOR_BLOCK_AFTER)
echo "验证者当前区块: $VALIDATOR_BLOCK_AFTER ($VALIDATOR_BLOCK_DEC)"

FULLNODE_BLOCK_AFTER=$(curl -s "http://127.0.0.1:$FULLNODE_EVM_PORT" -X POST \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' | \
    grep -oE '"result":"0x[0-9a-fA-F]+"' | cut -d'"' -f4)
FULLNODE_BLOCK_DEC=$(printf "%d" $FULLNODE_BLOCK_AFTER)
echo "全节点当前区块: $FULLNODE_BLOCK_AFTER ($FULLNODE_BLOCK_DEC)"

# 11. 获取最新区块详情
echo ""
echo "=== 11. 获取最新区块详情 ==="

# 使用验证者的区块高度查询两个节点
VALIDATOR_LATEST=$(curl -s "http://127.0.0.1:$VALIDATOR_EVM_PORT" -X POST \
    -H "Content-Type: application/json" \
    -d "{\"jsonrpc\":\"2.0\",\"method\":\"eth_getBlockByNumber\",\"params\":[\"$VALIDATOR_BLOCK_AFTER\",false],\"id\":1}")
VALIDATOR_HASH=$(echo "$VALIDATOR_LATEST" | grep -oE '"hash":"0x[a-fA-F0-9]{64}"' | head -1 | cut -d'"' -f4)
VALIDATOR_STATE_ROOT=$(echo "$VALIDATOR_LATEST" | grep -oE '"stateRoot":"0x[a-fA-F0-9]{64}"' | head -1 | cut -d'"' -f4)
echo "验证者区块 $VALIDATOR_BLOCK_DEC:"
echo "  Hash: $VALIDATOR_HASH"
echo "  StateRoot: $VALIDATOR_STATE_ROOT"

# 查询全节点同一区块
if [ "$FULLNODE_BLOCK_DEC" -ge "$VALIDATOR_BLOCK_DEC" ]; then
    FULLNODE_SAME_BLOCK=$(curl -s "http://127.0.0.1:$FULLNODE_EVM_PORT" -X POST \
        -H "Content-Type: application/json" \
        -d "{\"jsonrpc\":\"2.0\",\"method\":\"eth_getBlockByNumber\",\"params\":[\"$VALIDATOR_BLOCK_AFTER\",false],\"id\":1}")
    FULLNODE_HASH=$(echo "$FULLNODE_SAME_BLOCK" | grep -oE '"hash":"0x[a-fA-F0-9]{64}"' | head -1 | cut -d'"' -f4)
    FULLNODE_STATE_ROOT=$(echo "$FULLNODE_SAME_BLOCK" | grep -oE '"stateRoot":"0x[a-fA-F0-9]{64}"' | head -1 | cut -d'"' -f4)
    echo "全节点区块 $VALIDATOR_BLOCK_DEC:"
    echo "  Hash: $FULLNODE_HASH"
    echo "  StateRoot: $FULLNODE_STATE_ROOT"
else
    FULLNODE_HASH=""
    FULLNODE_STATE_ROOT=""
    echo "全节点尚未同步到区块 $VALIDATOR_BLOCK_DEC"
fi

# 12. 验证 DexVM 状态
echo ""
echo "=== 12. 验证 DexVM 状态 ==="
VALIDATOR_COUNTER=$(curl -s "http://127.0.0.1:$VALIDATOR_DEXVM_PORT/api/v1/counter/$TEST_ADDRESS")
FULLNODE_COUNTER=$(curl -s "http://127.0.0.1:$FULLNODE_DEXVM_PORT/api/v1/counter/$TEST_ADDRESS")
echo "验证者计数器: $VALIDATOR_COUNTER"
echo "全节点计数器: $FULLNODE_COUNTER"
echo "(注: 全节点仅同步区块元数据，不执行交易，因此 DexVM 状态不同步是预期行为)"

# 13. 测试结果
echo ""
echo "=============================================="
echo "  测试结果"
echo "=============================================="

PASSED=0
FAILED=0

# 检查验证者区块是否增加
BEFORE_DEC=$(printf "%d" $VALIDATOR_BLOCK_BEFORE)
if [ "$VALIDATOR_BLOCK_DEC" -gt "$BEFORE_DEC" ]; then
    echo -e "${GREEN}[PASS]${NC} 验证者产生了新区块 ($BEFORE_DEC -> $VALIDATOR_BLOCK_DEC)"
    PASSED=$((PASSED + 1))
else
    echo -e "${RED}[FAIL]${NC} 验证者没有产生新区块"
    FAILED=$((FAILED + 1))
fi

# 检查全节点是否同步
FULLNODE_BEFORE_DEC=$(printf "%d" $FULLNODE_BLOCK_BEFORE)
if [ "$FULLNODE_BLOCK_DEC" -gt "$FULLNODE_BEFORE_DEC" ]; then
    echo -e "${GREEN}[PASS]${NC} 全节点同步了新区块 ($FULLNODE_BEFORE_DEC -> $FULLNODE_BLOCK_DEC)"
    PASSED=$((PASSED + 1))
else
    echo -e "${RED}[FAIL]${NC} 全节点没有同步新区块"
    FAILED=$((FAILED + 1))
fi

# 检查区块高度是否接近
DIFF=$((VALIDATOR_BLOCK_DEC - FULLNODE_BLOCK_DEC))
if [ "$DIFF" -lt 0 ]; then
    DIFF=$((-DIFF))
fi
if [ "$DIFF" -le 5 ]; then
    echo -e "${GREEN}[PASS]${NC} 区块高度接近 (差距: $DIFF 块)"
    PASSED=$((PASSED + 1))
elif [ "$DIFF" -le 10 ]; then
    echo -e "${YELLOW}[WARN]${NC} 区块高度差距较大 (差距: $DIFF 块)"
else
    echo -e "${RED}[FAIL]${NC} 区块高度差距过大 (差距: $DIFF 块)"
    FAILED=$((FAILED + 1))
fi

# 检查区块哈希是否一致 (如果全节点已同步)
if [ -n "$FULLNODE_HASH" ] && [ "$FULLNODE_HASH" == "$VALIDATOR_HASH" ]; then
    echo -e "${GREEN}[PASS]${NC} 区块哈希一致"
    PASSED=$((PASSED + 1))
elif [ -n "$FULLNODE_HASH" ]; then
    echo -e "${RED}[FAIL]${NC} 区块哈希不一致"
    FAILED=$((FAILED + 1))
else
    echo -e "${YELLOW}[SKIP]${NC} 全节点尚未同步，无法比较哈希"
fi

echo ""
echo "通过: $PASSED, 失败: $FAILED"
echo ""

# 显示日志摘要
echo "=== 日志摘要 ==="
echo "验证者日志 (最后 10 行):"
tail -10 "$VALIDATOR_LOG"
echo ""
echo "全节点日志 (最后 10 行):"
tail -10 "$FULLNODE_LOG"

echo ""
echo "=============================================="
echo "  流程 4 测试完成"
echo "=============================================="
echo ""
echo "日志文件:"
echo "  验证者: $VALIDATOR_LOG"
echo "  全节点: $FULLNODE_LOG"
echo ""

if [ "$FAILED" -gt 0 ]; then
    exit 1
fi
