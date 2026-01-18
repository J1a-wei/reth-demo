#!/bin/bash
#
# 流程 3: DexVM RPC 直接操作计数器
#
# 测试内容:
# - 健康检查
# - 查询计数器
# - 增加计数器
# - 减少计数器
# - 获取状态根
#
# DexVM REST API:
# - GET  /health                              - 健康检查
# - GET  /api/v1/counter/:address             - 查询计数器
# - POST /api/v1/counter/:address/increment   - 增加计数器
# - POST /api/v1/counter/:address/decrement   - 减少计数器
# - GET  /api/v1/state-root                   - 获取状态根
#

set -e

# 配置
DEXVM_URL="${DEXVM_URL:-http://127.0.0.1:9845}"
TEST_ADDRESS="${TEST_ADDRESS:-0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266}"

echo "=============================================="
echo "  流程 3: DexVM RPC 直接操作计数器"
echo "=============================================="
echo ""
echo "DexVM API: $DEXVM_URL"
echo "测试地址: $TEST_ADDRESS"
echo ""

# 1. 健康检查
echo "=== 1. 健康检查 ==="
HEALTH=$(curl -s "$DEXVM_URL/health")
echo "响应: $HEALTH"
if echo "$HEALTH" | grep -q '"status":"ok"'; then
    echo "状态: OK"
else
    echo "Error: 健康检查失败"
    exit 1
fi

# 2. 查询初始计数器
echo ""
echo "=== 2. 查询初始计数器 ==="
INITIAL=$(curl -s "$DEXVM_URL/api/v1/counter/$TEST_ADDRESS")
echo "响应: $INITIAL"
INITIAL_VALUE=$(echo $INITIAL | grep -oE '"counter":[0-9]+' | cut -d':' -f2)
echo "初始值: $INITIAL_VALUE"

# 3. 增加计数器 (+10)
echo ""
echo "=== 3. 增加计数器 (+10) ==="
INCREMENT_RESULT=$(curl -s -X POST "$DEXVM_URL/api/v1/counter/$TEST_ADDRESS/increment" \
    -H "Content-Type: application/json" \
    -d '{"amount": 10}')
echo "响应: $INCREMENT_RESULT"
if echo "$INCREMENT_RESULT" | grep -q '"success":true'; then
    echo "状态: 成功"
else
    echo "Warning: 操作可能失败"
fi

# 4. 查询计数器
echo ""
echo "=== 4. 查询计数器 (预期: +10) ==="
AFTER_INC=$(curl -s "$DEXVM_URL/api/v1/counter/$TEST_ADDRESS")
echo "响应: $AFTER_INC"
AFTER_INC_VALUE=$(echo $AFTER_INC | grep -oE '"counter":[0-9]+' | cut -d':' -f2)
echo "当前值: $AFTER_INC_VALUE"

# 5. 再次增加计数器 (+5)
echo ""
echo "=== 5. 增加计数器 (+5) ==="
INCREMENT_RESULT2=$(curl -s -X POST "$DEXVM_URL/api/v1/counter/$TEST_ADDRESS/increment" \
    -H "Content-Type: application/json" \
    -d '{"amount": 5}')
echo "响应: $INCREMENT_RESULT2"

# 6. 减少计数器 (-3)
echo ""
echo "=== 6. 减少计数器 (-3) ==="
DECREMENT_RESULT=$(curl -s -X POST "$DEXVM_URL/api/v1/counter/$TEST_ADDRESS/decrement" \
    -H "Content-Type: application/json" \
    -d '{"amount": 3}')
echo "响应: $DECREMENT_RESULT"
if echo "$DECREMENT_RESULT" | grep -q '"success":true'; then
    echo "状态: 成功"
else
    echo "Warning: 操作可能失败"
fi

# 7. 查询最终计数器
echo ""
echo "=== 7. 查询最终计数器 ==="
FINAL=$(curl -s "$DEXVM_URL/api/v1/counter/$TEST_ADDRESS")
echo "响应: $FINAL"
FINAL_VALUE=$(echo $FINAL | grep -oE '"counter":[0-9]+' | cut -d':' -f2)
echo "最终值: $FINAL_VALUE"

# 8. 测试减少溢出 (应该失败)
echo ""
echo "=== 8. 测试减少溢出 (应该失败) ==="
OVERFLOW_RESULT=$(curl -s -X POST "$DEXVM_URL/api/v1/counter/$TEST_ADDRESS/decrement" \
    -H "Content-Type: application/json" \
    -d '{"amount": 999999}')
echo "响应: $OVERFLOW_RESULT"
if echo "$OVERFLOW_RESULT" | grep -q '"success":false'; then
    echo "状态: 正确拒绝 (计数器下溢)"
else
    echo "Warning: 预期失败但操作成功?"
fi

# 9. 获取状态根
echo ""
echo "=== 9. 获取状态根 ==="
STATE_ROOT=$(curl -s "$DEXVM_URL/api/v1/state-root")
echo "响应: $STATE_ROOT"

# 10. 测试其他地址
echo ""
echo "=== 10. 测试其他地址 ==="
OTHER_ADDRESS="0x70997970C51812dc3A010C7d01b50e0d17dc79C8"
echo "地址: $OTHER_ADDRESS"
OTHER_COUNTER=$(curl -s "$DEXVM_URL/api/v1/counter/$OTHER_ADDRESS")
echo "计数器: $OTHER_COUNTER"

# 总结
echo ""
echo "=============================================="
echo "  流程 3 测试完成"
echo "=============================================="
echo ""
echo "测试结果:"
echo "  - 健康检查: OK"
echo "  - 初始计数器: $INITIAL_VALUE"
echo "  - 最终计数器: $FINAL_VALUE"
echo "  - 预期变化: +10 +5 -3 = +12"
echo ""
