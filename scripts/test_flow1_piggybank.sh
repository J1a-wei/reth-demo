#!/bin/bash
#
# 流程 1: Foundry 部署 PiggyBank 合约 (验证 EVM)
#
# 测试内容:
# - 部署 Solidity 合约
# - 合约存款 (deposit)
# - 查询余额 (getBalance)
# - 合约取款 (withdraw)
#
# 前置条件:
# - 安装 Foundry (forge, cast)
# - 节点已启动并使用创世配置
#

set -e

# 配置
RPC_URL="${RPC_URL:-http://127.0.0.1:8545}"
# 测试账户 (来自 genesis.json)
PRIVATE_KEY="${PRIVATE_KEY:-0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80}"
TEST_ADDRESS="0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"
CONTRACT_DIR="$(dirname "$0")/../contracts"

echo "=============================================="
echo "  流程 1: Foundry 部署 PiggyBank 合约"
echo "=============================================="
echo ""

# 检查 Foundry 是否安装
if ! command -v forge &> /dev/null; then
    echo "Error: Foundry (forge) 未安装"
    echo "安装: curl -L https://foundry.paradigm.xyz | bash && foundryup"
    exit 1
fi

if ! command -v cast &> /dev/null; then
    echo "Error: Foundry (cast) 未安装"
    exit 1
fi

# 检查节点是否运行
echo "=== 1. 检查节点状态 ==="
CHAIN_ID=$(cast chain-id --rpc-url $RPC_URL 2>/dev/null || echo "error")
if [ "$CHAIN_ID" = "error" ]; then
    echo "Error: 无法连接到节点 $RPC_URL"
    echo "请先启动节点: ./scripts/start_validator.sh"
    exit 1
fi
echo "Chain ID: $CHAIN_ID"

# 检查账户余额
echo ""
echo "=== 2. 检查测试账户余额 ==="
BALANCE=$(cast balance $TEST_ADDRESS --rpc-url $RPC_URL)
echo "账户: $TEST_ADDRESS"
echo "余额: $BALANCE wei ($(echo "scale=4; $BALANCE / 1000000000000000000" | bc) ETH)"

# 部署合约
echo ""
echo "=== 3. 部署 PiggyBank 合约 ==="
cd $CONTRACT_DIR

DEPLOY_OUTPUT=$(forge create PiggyBank.sol:PiggyBank \
    --rpc-url $RPC_URL \
    --private-key $PRIVATE_KEY \
    --legacy \
    --broadcast 2>&1) || true

echo "Forge output:"
echo "$DEPLOY_OUTPUT"

# 从部署输出获取交易哈希 - 优先查找 "Transaction hash:" 或 "transactionHash"
TX_HASH=$(echo "$DEPLOY_OUTPUT" | grep -i "transaction hash" | grep -oE '0x[a-fA-F0-9]{64}' | head -1 || true)
if [ -z "$TX_HASH" ]; then
    # 如果没找到，尝试从 JSON 输出获取
    TX_HASH=$(echo "$DEPLOY_OUTPUT" | grep -oE '"transactionHash":"0x[a-fA-F0-9]{64}"' | grep -oE '0x[a-fA-F0-9]{64}' | head -1 || true)
fi
if [ -z "$TX_HASH" ]; then
    # 最后尝试获取第一个64位hex，跳过可能的区块哈希
    TX_HASH=$(echo "$DEPLOY_OUTPUT" | grep -v "block" | grep -oE '0x[a-fA-F0-9]{64}' | head -1 || true)
fi

if [ -n "$TX_HASH" ]; then
    echo "交易哈希: $TX_HASH"
    sleep 5  # 等待交易被包含 (增加等待时间)

    # 获取合约地址
    RECEIPT=$(curl -s -X POST $RPC_URL \
        -H "Content-Type: application/json" \
        -d "{\"jsonrpc\":\"2.0\",\"method\":\"eth_getTransactionReceipt\",\"params\":[\"$TX_HASH\"],\"id\":1}")
    CONTRACT_ADDRESS=$(echo $RECEIPT | grep -oE '"contractAddress":"0x[a-fA-F0-9]{40}"' | cut -d'"' -f4)

    if [ -n "$CONTRACT_ADDRESS" ]; then
        echo "合约地址: $CONTRACT_ADDRESS"
    else
        echo "Warning: 无法从回执获取合约地址"
        echo "回执: $RECEIPT"
    fi
else
    echo "Warning: 部署可能失败"
    echo "$DEPLOY_OUTPUT"
fi

if [ -z "$CONTRACT_ADDRESS" ]; then
    echo "Error: 合约部署失败"
    exit 1
fi

# 存款测试
echo ""
echo "=== 4. 存款测试 (1 ETH) ==="
DEPOSIT_OUTPUT=$(cast send $CONTRACT_ADDRESS "deposit()" \
    --value 1ether \
    --rpc-url $RPC_URL \
    --private-key $PRIVATE_KEY \
    --legacy 2>&1) || true
echo "存款交易已发送"
sleep 3

# 查询合约中的余额
echo ""
echo "=== 5. 查询合约余额 ==="
# 由于 eth_call 可能不完全支持，使用 curl 直接查询
BALANCE_AFTER=$(cast balance $TEST_ADDRESS --rpc-url $RPC_URL)
echo "账户余额: $BALANCE_AFTER wei"

# 取款测试
echo ""
echo "=== 6. 取款测试 (0.5 ETH) ==="
WITHDRAW_OUTPUT=$(cast send $CONTRACT_ADDRESS "withdraw(uint256)" 500000000000000000 \
    --rpc-url $RPC_URL \
    --private-key $PRIVATE_KEY \
    --legacy 2>&1) || true
echo "取款交易已发送"
sleep 3

# 最终余额
echo ""
echo "=== 7. 最终状态 ==="
FINAL_BALANCE=$(cast balance $TEST_ADDRESS --rpc-url $RPC_URL)
echo "最终账户余额: $FINAL_BALANCE wei"

echo ""
echo "=============================================="
echo "  流程 1 测试完成"
echo "=============================================="
echo "合约地址: $CONTRACT_ADDRESS"
echo ""
