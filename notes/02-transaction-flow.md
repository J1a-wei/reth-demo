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
