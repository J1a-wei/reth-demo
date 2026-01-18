# 讨论：状态管理架构

## 问题

DexVM 的 `DexVmState` 为什么使用 HashMap 而不是直接操作数据库？

## 结论

**采用两层架构：内存层 + 持久层**

这是区块链客户端的标准模式，Ethereum 客户端（geth、reth）都采用类似设计。

---

## 架构概览

```
┌─────────────────────────────────────────────────────────┐
│                    交易执行层                            │
│  ┌─────────────────────────────────────────────────┐   │
│  │            DexVmExecutor                         │   │
│  │  ┌─────────────┐    ┌─────────────────────┐     │   │
│  │  │   state     │    │   pending_state     │     │   │
│  │  │  (已提交)    │◄───│   (待提交)          │     │   │
│  │  │  HashMap    │    │   HashMap           │     │   │
│  │  └─────────────┘    └─────────────────────┘     │   │
│  └─────────────────────────────────────────────────┘   │
│                         │                               │
│                         │ commit() / sync              │
│                         ▼                               │
├─────────────────────────────────────────────────────────┤
│                    持久化层                              │
│  ┌─────────────────────────────────────────────────┐   │
│  │              StateStore (MDBX)                   │   │
│  │  ┌─────────────────────────────────────────┐    │   │
│  │  │          DualvmCounters 表               │    │   │
│  │  │         Address -> Counter              │    │   │
│  │  └─────────────────────────────────────────┘    │   │
│  └─────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────┘
```

## 两层结构

| 层级 | 结构 | 代码位置 | 用途 |
|------|------|----------|------|
| 内存层 | `DexVmState` (HashMap) | `crates/dexvm/src/state.rs` | 执行时的工作状态 |
| 持久层 | `DualvmCounters` (MDBX) | `crates/storage/src/state_store.rs` | 数据库持久化 |

---

## 设计原因

### 1. 执行效率

交易执行时需要频繁读写状态：

```rust
// HashMap: O(1) 访问
let counter = state.get_counter(&address);  // 快速
state.set_counter(address, counter + 1);    // 快速

// 直接数据库: 每次都有 I/O 开销
let counter = db.get::<DualvmCounters>(address)?;  // 慢
db.put::<DualvmCounters>(address, counter + 1)?;   // 慢
```

一个区块可能有数百笔交易，内存操作比磁盘 I/O 快几个数量级。

### 2. 原子性和回滚

```rust
// DexVmExecutor 结构
pub struct DexVmExecutor {
    state: DexVmState,         // 已提交状态
    pending_state: DexVmState, // 待提交状态（执行中）
}

impl DexVmExecutor {
    // 执行交易时修改 pending_state
    pub fn execute_transaction(&mut self, tx: &DexVmTransaction) -> Result<...> {
        // 修改 pending_state，不影响 state
        self.pending_state.increment_counter(tx.from, amount);
    }

    // 成功后提交
    pub fn commit(&mut self) {
        self.state = self.pending_state.clone();
    }

    // 失败时回滚：直接丢弃 pending_state，state 不变
}
```

这种设计支持：
- **单交易回滚**：交易失败时，pending_state 可以被重置
- **跨 VM 原子性**：EVM 调用 DexVM 预编译失败时，两边都能回滚

### 3. 批量写入

```
区块执行流程：
1. 加载状态到内存
2. 执行交易 1 → 修改内存
3. 执行交易 2 → 修改内存
4. ...
5. 执行交易 N → 修改内存
6. 计算状态根
7. 一次性写入数据库  ← 只有一次磁盘 I/O
```

如果每笔交易都写数据库：
- N 笔交易 = N 次磁盘写入
- 性能差，且难以保证原子性

### 4. 状态根计算

状态根需要遍历所有账户：

```rust
// DexVmState::state_root()
pub fn state_root(&self) -> B256 {
    let mut accounts: Vec<_> = self.counters.iter().collect();
    accounts.sort_by_key(|(addr, _)| *addr);
    // ...
    keccak256(&data)
}
```

HashMap 在内存中，遍历快速。如果每次都从数据库读取，会很慢。

---

## 持久化时机

### 1. 区块执行完成后

```rust
// crates/node/src/executor.rs
pub fn execute_transactions(&mut self, transactions: Vec<...>) -> Result<...> {
    // 执行所有交易...

    // 同步 pending 到 state
    dexvm_executor.sync_pending_to_state();

    // 计算状态根
    let dexvm_state_root = dexvm_executor.state_root();
}
```

### 2. 区块存储时

```rust
// 区块存储到数据库时，状态也会持久化
block_store.store_block(block)?;
state_store.set_counter(address, value)?;
```

### 3. 节点启动时

```rust
// 从数据库加载状态到内存
let counters = state_store.all_counters();
let mut dexvm_state = DexVmState::new();
for (addr, value) in counters {
    dexvm_state.set_counter(addr, value);
}
```

---

## 对比：EVM 的类似设计

Ethereum 客户端采用相同模式：

| 组件 | 内存结构 | 持久化 |
|------|----------|--------|
| geth | StateDB (内存 trie 缓存) | LevelDB |
| reth | State (BundleState) | MDBX |
| dex-reth | DexVmState (HashMap) | MDBX DualvmCounters |

---

## 代码位置

| 文件 | 功能 |
|------|------|
| `crates/dexvm/src/state.rs` | DexVmState 内存状态 |
| `crates/dexvm/src/executor.rs` | DexVmExecutor 双缓冲执行 |
| `crates/storage/src/state_store.rs` | StateStore 持久化 |
| `crates/storage/src/tables.rs` | DualvmCounters 表定义 |
| `crates/node/src/executor.rs` | 区块执行和状态同步 |

---

## 状态

**已确定**：采用两层架构（内存 + 持久化）。
