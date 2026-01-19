# 交易处理流程

## 1. 交易提交流程

```mermaid
sequenceDiagram
    participant User as 用户/钱包
    participant RPC as RPC Server
    participant Mempool as 交易池
    participant Consensus as POA 共识
    participant Executor as DualVmExecutor
    participant Storage as 存储层

    User->>RPC: eth_sendRawTransaction(tx_bytes)
    RPC->>RPC: 解码交易 (RLP)
    RPC->>RPC: 恢复签名者地址
    RPC->>Storage: 查询账户 nonce & balance
    Storage-->>RPC: 返回账户状态

    alt nonce 检查失败
        RPC-->>User: 错误: Nonce too low
    else 余额不足
        RPC-->>User: 错误: Insufficient balance
    else 验证通过
        RPC->>Mempool: 添加到交易池
        RPC->>RPC: 广播到 P2P 网络
        RPC-->>User: 返回 tx_hash
    end

    Note over Consensus: 每 500ms 出块

    Consensus->>Mempool: 获取待处理交易
    Mempool-->>Consensus: 返回交易列表
    Consensus->>Consensus: 创建区块提案 (BlockProposal)
    Consensus->>Executor: 执行交易
    Executor->>Storage: 更新状态
    Executor-->>Consensus: 返回执行结果
    Consensus->>Storage: 存储区块
    Consensus->>Consensus: 广播区块到 P2P
```

## 2. 交易路由决策

```mermaid
flowchart TD
    START[接收交易] --> DECODE[解码 RLP 交易]
    DECODE --> CHECK_TO{检查目标地址}

    CHECK_TO -->|to == DEXVM_ROUTER<br/>0xddd...dd1| DEXVM[路由到 DexVM]
    CHECK_TO -->|to == PRECOMPILE<br/>0x000...100| PRECOMPILE[跨 VM 预编译]
    CHECK_TO -->|其他地址| EVM[路由到 EVM]

    DEXVM --> PARSE_CALLDATA[解析 calldata]
    PARSE_CALLDATA --> DEXVM_OP{操作类型}
    DEXVM_OP -->|0x00| INCREMENT[增加计数器]
    DEXVM_OP -->|0x01| DECREMENT[减少计数器]
    DEXVM_OP -->|0x02| QUERY[查询计数器]

    INCREMENT --> DEXVM_EXEC[DexVmExecutor 执行]
    DECREMENT --> DEXVM_EXEC
    QUERY --> DEXVM_EXEC

    EVM --> EVM_EXEC[SimpleEvmExecutor 执行]

    PRECOMPILE --> CROSS_VM[获取两个 VM 的写锁]
    CROSS_VM --> PRECOMPILE_EXEC[PrecompileExecutor 执行]
    PRECOMPILE_EXEC --> UPDATE_BOTH[更新 EVM + DexVM 状态]

    DEXVM_EXEC --> RESULT[返回执行结果]
    EVM_EXEC --> RESULT
    UPDATE_BOTH --> RESULT
```

## 3. DexVM 交易 Calldata 格式

```
┌─────────────┬─────────────────────────────────────┐
│  Byte 0     │  Bytes 1-8                          │
│  操作类型    │  金额 (u64 big-endian)               │
├─────────────┼─────────────────────────────────────┤
│  0x00       │  increment amount                   │
│  0x01       │  decrement amount                   │
│  0x02       │  padding (query 不需要金额)          │
└─────────────┴─────────────────────────────────────┘

示例:
增加 100: 0x00 0000000000000064
减少 50:  0x01 0000000000000032
查询:     0x02 0000000000000000
```

## 4. EVM 交易执行流程

```mermaid
flowchart TD
    START[接收 EVM 交易] --> RECOVER[恢复签名者地址]
    RECOVER --> CHECK_PRECOMPILE{是否调用预编译?}

    CHECK_PRECOMPILE -->|是| PRECOMPILE_FLOW[预编译执行流程]
    CHECK_PRECOMPILE -->|否| NORMAL_FLOW[普通交易流程]

    NORMAL_FLOW --> GET_STATE[获取账户状态]
    GET_STATE --> CHECK_NONCE{nonce 匹配?}

    CHECK_NONCE -->|否| FAIL_NONCE[返回失败: nonce 不匹配]
    CHECK_NONCE -->|是| CHECK_BALANCE{余额充足?}

    CHECK_BALANCE -->|否| FAIL_BALANCE[返回失败: 余额不足]
    CHECK_BALANCE -->|是| DEDUCT[扣除余额 + gas]

    DEDUCT --> INC_NONCE[增加 nonce]
    INC_NONCE --> TRANSFER{是否有转账?}

    TRANSFER -->|是| ADD_TO[增加接收方余额]
    TRANSFER -->|否| SKIP_TRANSFER[跳过]

    ADD_TO --> SUCCESS[返回成功 Receipt]
    SKIP_TRANSFER --> SUCCESS

    PRECOMPILE_FLOW --> GET_LOCKS[获取 EVM + DexVM 写锁]
    GET_LOCKS --> CHECK_PRECOMPILE_BALANCE{余额充足?}

    CHECK_PRECOMPILE_BALANCE -->|否| ROLLBACK[回滚状态]
    CHECK_PRECOMPILE_BALANCE -->|是| DEDUCT_GAS[扣除 gas]

    DEDUCT_GAS --> EXEC_PRECOMPILE[执行预编译]
    EXEC_PRECOMPILE --> CHECK_RESULT{执行成功?}

    CHECK_RESULT -->|否| ROLLBACK
    CHECK_RESULT -->|是| COMMIT_DEXVM[提交 DexVM 状态]

    ROLLBACK --> INC_NONCE_FAIL[增加 nonce]
    INC_NONCE_FAIL --> FAIL_RECEIPT[返回失败 Receipt]

    COMMIT_DEXVM --> INC_NONCE2[增加 nonce]
    INC_NONCE2 --> SUCCESS
```

## 5. DexVM 交易执行流程

```mermaid
flowchart TD
    START[接收 DexVM 交易] --> PARSE[解析操作类型和金额]
    PARSE --> OP_TYPE{操作类型}

    OP_TYPE -->|Increment| INC_CHECK[检查操作]
    OP_TYPE -->|Decrement| DEC_CHECK{当前值 >= 金额?}
    OP_TYPE -->|Query| QUERY_OP[查询当前值]

    INC_CHECK --> INC_EXEC[执行增加]
    INC_EXEC --> INC_RESULT[返回新值]

    DEC_CHECK -->|否| DEC_FAIL[返回错误: underflow]
    DEC_CHECK -->|是| DEC_EXEC[执行减少]
    DEC_EXEC --> DEC_RESULT[返回新值]

    QUERY_OP --> QUERY_RESULT[返回当前值]

    INC_RESULT --> CREATE_RECEIPT[创建 DexVmReceipt]
    DEC_RESULT --> CREATE_RECEIPT
    DEC_FAIL --> CREATE_RECEIPT
    QUERY_RESULT --> CREATE_RECEIPT

    CREATE_RECEIPT --> COMMIT[提交到 pending_state]
    COMMIT --> RETURN[返回执行结果]

    subgraph "DexVmExecutionResult"
        R_SUCCESS[success: bool]
        R_OLD[old_counter: u64]
        R_NEW[new_counter: u64]
        R_GAS[gas_used: u64]
        R_ERROR[error: Option<String>]
    end
```

## 6. Gas 消耗

| 操作 | Base Gas | 操作 Gas | 总 Gas |
|------|----------|----------|--------|
| EVM 转账 | 21,000 | - | 21,000 |
| DexVM Increment | 21,000 | 5,000 | 26,000 |
| DexVM Decrement | 21,000 | 5,000 | 26,000 |
| DexVM Query | 21,000 | 3,000 | 24,000 |
| 预编译调用 | 21,000 | 5,000 | 26,000 |

## 7. 交易收据格式

### EVM Receipt (alloy_consensus::Receipt)
```rust
Receipt {
    status: Eip658Value,        // true = 成功, false = 失败
    cumulative_gas_used: u64,   // 累计 gas 消耗
    logs: Vec<Log>,             // 事件日志
}
```

### DexVM Receipt
```rust
DexVmReceipt {
    from: Address,              // 发送者
    success: bool,              // 是否成功
    old_counter: u64,           // 操作前计数器值
    new_counter: u64,           // 操作后计数器值
    gas_used: u64,              // gas 消耗
    error: Option<String>,      // 错误信息
}
```

## 8. 地址角色详解

### 关键地址说明

| 地址 | 角色 | 说明 |
|------|------|------|
| `0xddd...dd1` | **to** (目标地址) | 固定的路由地址，告诉节点"这是 DexVM 交易" |
| 用户自己的地址 | **from** (发送者) | 从交易签名中恢复，作为计数器的 key |

**重要**: `0xddddddddddddddddddddddddddddddddddddddd1` 只是一个"门牌号"，用于告诉系统走 DexVM 通道。用户**不需要**持有这个地址的私钥。

### 交易签名与地址恢复

```
Alice (地址 0xAAAA) 想操作自己的计数器：

1. 构造交易
   to:   0xddddddddddddddddddddddddddddddddddddddd1  ← 固定路由地址
   data: 0x00 0000000000000064  (increment 100)

2. 用 Alice 的私钥签名

3. 发送到节点

4. 节点处理：
   - 从签名恢复出 from = 0xAAAA (Alice)
   - 看到 to = 0xddd...dd1，路由到 DexVM
   - 操作 counters[0xAAAA] += 100  ← Alice 的计数器
```

### 计数器存储结构

```
counters = {
    0xAAAA...: 100,   // Alice 的计数器
    0xBBBB...: 50,    // Bob 的计数器
    0xCCCC...: 200,   // Carol 的计数器
}
```

每个用户有自己独立的计数器，**key 是用户自己的地址 (from)**，不是路由地址。

## 9. REST API vs 以太坊交易

系统提供两种方式操作 DexVM 计数器，它们有本质区别：

### 对比表

| 特性 | REST API (Port 9845) | 以太坊交易 (to=0xddd...dd1) |
|------|---------------------|---------------------------|
| 打包进区块 | ❌ 不会 | ✅ 会 |
| P2P 网络同步 | ❌ 不会 | ✅ 会 |
| 交易签名验证 | ❌ 无 | ✅ 有 |
| 其他节点可验证 | ❌ 不能 | ✅ 能 |
| 钱包兼容 | ❌ 需要定制 | ✅ MetaMask 等可用 |
| from 地址来源 | URL 参数 (可伪造) | 签名恢复 (不可伪造) |

### REST API 的问题

```bash
# REST API: 任何人都可以冒充任何地址
curl -X POST http://localhost:9845/api/v1/counter/0xAAAA.../increment \
  -d '{"amount": 100}'
# from = URL 中的 0xAAAA，无需证明身份
```

```rust
// 服务器代码
let tx = DexVmTransaction {
    from: address,  // 直接从 URL 取，谁都可以填
    signature: [],  // 空的，没有验证
};
```

### 以太坊交易的安全性

```bash
# 以太坊交易: 必须用私钥签名
cast send 0xddddddddddddddddddddddddddddddddddddddd1 \
  --private-key 0xAlice私钥 \
  0x000000000000000064
# from = 从签名恢复，只有私钥持有者才能操作
```

### 状态同步差异

```
REST API (本地操作，不同步):
  节点A修改计数器 → 只有节点A知道 → 节点B查询时值不同 → 状态不一致

以太坊交易 (上链，全网同步):
  节点A发送TX → 打包进区块 → P2P广播 → 节点B收到区块并执行 → 状态一致
```

### 使用场景建议

| 场景 | 推荐方式 |
|------|---------|
| 本地调试/测试 | REST API |
| 生产环境状态变更 | 以太坊交易 |
| 需要全网共识 | 以太坊交易 |
| 查询计数器值 | REST API (只读，无副作用) |
