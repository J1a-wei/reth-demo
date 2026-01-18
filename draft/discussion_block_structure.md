# 讨论：双 VM 区块结构设计

## 设计决策

| 决定 | 说明 |
|------|------|
| 方向 | 扩展以太坊，P2P 兼容 |
| 节点 | 所有节点都运行我们的程序，内部格式自由 |
| 交易排序 | EVM 和 DexVM 分开排序 |
| transactions_root | 合并计算 |
| 存储 | 需要存储交易体和收据 |

## 业务定位

| VM | 用途 |
|----|------|
| EVM | 边缘业务：治理、充提等 |
| DexVM | 核心业务：下单、撤单等 |

## 区块结构

```rust
// 区块头
struct BlockHeader {
    number: u64,
    parent_hash: B256,
    timestamp: u64,
    beneficiary: Address,

    // State roots
    state_root: B256,           // combined = keccak256(evm_root || dexvm_root)
    evm_state_root: B256,
    dexvm_state_root: B256,

    // Tx/Receipt roots
    transactions_root: B256,    // 合并计算
    receipts_root: B256,

    gas_limit: u64,
    gas_used: u64,
}

// 区块体
struct BlockBody {
    evm_transactions: Vec<TransactionSigned>,
    dexvm_transactions: Vec<DexVmTransaction>,
}

// DexVM 交易（简单格式）
struct DexVmTransaction {
    from: Address,
    nonce: u64,
    operation: DexOperation,
    amount: u64,
    signature: Signature,
}
```

## EVM 触发 DexVM

通过预编译合约实现：

```
用户 → EVM 交易 → 调用预编译合约 (0x0...100) → 触发 DexVM 逻辑
```

待定问题：
1. 原子性 - EVM 调用 DexVM 失败时的回滚
2. Gas 计算
3. Receipt 生成

## 当前阶段原则

**重要：先验证架构可行性**

- DexVM 逻辑：简单的 Counter 状态机
- 不要过度设计 DEX 逻辑
- 等架构验证通过后再扩展

## 实现状态

### EVM → DexVM 跨 VM 调用 ✅ 已实现

通过预编译合约实现：

```
用户 → EVM 交易 → 调用预编译合约 (0x0...100) → 触发 DexVM 逻辑
```

实现细节：
1. **原子性** - 如果 DexVM 操作失败，EVM 状态自动回滚
2. **Gas 计算** - 固定值 (26000 for increment/decrement, 24000 for query)
3. **Receipt 生成** - 使用标准 EVM Receipt，status 表示成功/失败

### 待确定
- 交易体存储格式
- Receipt 存储格式
- transactions_root 计算方式
