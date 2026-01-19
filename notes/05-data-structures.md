# 核心数据结构

## 1. 交易类型

### DualVmTransaction (交易路由)

```rust
/// 双 VM 交易路由
pub enum DualVmTransaction {
    /// 路由到 EVM
    Evm(TransactionSigned),
    /// 路由到 DexVM
    DexVm(DexVmTransaction),
}

/// 路由规则
impl DualVmTransaction {
    pub fn from_ethereum_tx(tx: TransactionSigned) -> Self {
        match tx.to() {
            Some(to) if to == DEXVM_ROUTER_ADDRESS => {
                // 0xddddddddddddddddddddddddddddddddddddddd1
                Self::DexVm(DexVmTransaction::from_calldata(&tx))
            }
            _ => Self::Evm(tx),
        }
    }
}
```

### DexVmTransaction (DexVM 交易)

```rust
/// DexVM 原生交易
pub struct DexVmTransaction {
    pub from: Address,              // 发送者地址
    pub operation: DexVmOperation,  // 操作类型
    pub signature: Vec<u8>,         // 签名
}

/// DexVM 操作类型
pub enum DexVmOperation {
    Increment(u64),  // 增加计数器
    Decrement(u64),  // 减少计数器
    Query,           // 查询计数器
}
```

### 交易 Calldata 编码

```
┌─────────────────────────────────────────────────────────────┐
│                     Calldata Format (9 bytes)               │
├──────────┬──────────────────────────────────────────────────┤
│  Byte 0  │  Bytes 1-8                                       │
│  op_type │  amount (u64 big-endian)                         │
├──────────┼──────────────────────────────────────────────────┤
│   0x00   │  increment amount                                │
│   0x01   │  decrement amount                                │
│   0x02   │  (padding, ignored)                              │
└──────────┴──────────────────────────────────────────────────┘
```

## 2. 区块结构

### StoredBlock (存储的区块)

```rust
/// 持久化存储的区块
pub struct StoredBlock {
    pub number: u64,                    // 区块高度
    pub hash: B256,                     // 区块哈希
    pub parent_hash: B256,              // 父区块哈希
    pub timestamp: u64,                 // Unix 时间戳
    pub gas_limit: u64,                 // Gas 上限
    pub gas_used: u64,                  // 已使用 Gas
    pub miner: Address,                 // 出块者地址
    pub evm_state_root: B256,           // EVM 状态根
    pub dexvm_state_root: B256,         // DexVM 状态根
    pub combined_state_root: B256,      // 组合状态根
    pub transaction_hashes: Vec<B256>,  // 交易哈希列表
    pub transaction_count: u64,         // 交易数量
    pub signature: [u8; 65],            // POA 签名
}
```

### BlockProposal (区块提案)

```rust
/// 共识引擎生成的区块提案
pub struct BlockProposal {
    pub number: u64,                        // 区块高度
    pub parent_hash: B256,                  // 父区块哈希
    pub timestamp: u64,                     // 时间戳
    pub transactions: Vec<TransactionSigned>, // 交易列表
    pub proposer: Address,                  // 提案者地址
    pub signature: BlockSignature,          // 区块签名
}

/// 区块签名 (ECDSA)
pub struct BlockSignature {
    pub r: B256,    // r 分量 (32 bytes)
    pub s: B256,    // s 分量 (32 bytes)
    pub v: u8,      // 恢复 ID (1 byte)
}
```

### ConsensusHeader (以太坊兼容区块头)

```rust
/// 用于计算区块哈希的标准以太坊区块头
pub struct ConsensusHeader {
    pub parent_hash: B256,
    pub ommers_hash: B256,          // keccak256([0x80])
    pub beneficiary: Address,        // = miner
    pub state_root: B256,            // = combined_state_root
    pub transactions_root: B256,     // keccak256([0x80])
    pub receipts_root: B256,         // keccak256([0x80])
    pub logs_bloom: Bloom,           // Bloom::ZERO
    pub difficulty: U256,            // 0
    pub number: u64,
    pub gas_limit: u64,
    pub gas_used: u64,
    pub timestamp: u64,
    pub extra_data: Bytes,           // = signature bytes
    pub mix_hash: B256,              // B256::ZERO
    pub nonce: B64,                  // B64::ZERO
    pub base_fee_per_gas: Option<u64>, // Some(0)
    // ... EIP-4844 等字段为 None
}
```

## 3. 账户状态

### AccountState (EVM 账户)

```rust
/// EVM 账户状态
pub struct AccountState {
    pub balance: U256,                      // 余额
    pub nonce: u64,                         // 交易计数
    pub code_hash: B256,                    // 合约代码哈希
    pub code: Option<Bytes>,                // 合约字节码
    pub storage: HashMap<U256, U256>,       // 存储槽
}
```

### StoredDualvmAccount (MDBX 存储格式)

```rust
/// MDBX 中存储的账户
#[derive(Compact)]
pub struct StoredDualvmAccount {
    pub balance: U256,      // 余额
    pub nonce: u64,         // nonce
    pub code_hash: B256,    // 代码哈希
    pub is_contract: bool,  // 是否为合约
}
```

### DexVmState (DexVM 状态)

```rust
/// DexVM 内存状态
pub struct DexVmState {
    counters: HashMap<Address, u64>,  // 地址 -> 计数器值
}

impl DexVmState {
    /// 获取计数器 (不存在返回 0)
    pub fn get_counter(&self, address: &Address) -> u64;

    /// 设置计数器
    pub fn set_counter(&mut self, address: Address, value: u64);

    /// 增加计数器 (返回新值)
    pub fn increment_counter(&mut self, address: &Address, amount: u64) -> u64;

    /// 减少计数器 (underflow 检查)
    pub fn decrement_counter(&mut self, address: &Address, amount: u64) -> Result<u64>;

    /// 计算状态根
    pub fn state_root(&self) -> B256;
}
```

## 4. 执行结果

### DualVmExecutionResult (双 VM 执行结果)

```rust
/// 区块执行结果
pub struct DualVmExecutionResult {
    pub evm_receipts: Vec<Receipt>,         // EVM 收据列表
    pub dexvm_receipts: Vec<DexVmReceipt>,  // DexVM 收据列表
    pub total_gas_used: u64,                // 总 Gas 消耗
    pub evm_state_root: B256,               // EVM 状态根
    pub dexvm_state_root: B256,             // DexVM 状态根
    pub combined_state_root: B256,          // 组合状态根
}
```

### DexVmExecutionResult (单笔交易结果)

```rust
/// DexVM 交易执行结果
pub struct DexVmExecutionResult {
    pub success: bool,              // 是否成功
    pub old_counter: u64,           // 执行前计数器值
    pub new_counter: u64,           // 执行后计数器值
    pub gas_used: u64,              // Gas 消耗
    pub error: Option<String>,      // 错误信息
}
```

### Receipt (EVM 收据)

```rust
/// EVM 交易收据
pub struct Receipt {
    pub status: Eip658Value,        // 执行状态
    pub cumulative_gas_used: u64,   // 累计 Gas
    pub logs: Vec<Log>,             // 事件日志
}
```

### DexVmReceipt (DexVM 收据)

```rust
/// DexVM 交易收据
pub struct DexVmReceipt {
    pub from: Address,              // 发送者
    pub success: bool,              // 是否成功
    pub old_counter: u64,           // 操作前值
    pub new_counter: u64,           // 操作后值
    pub gas_used: u64,              // Gas 消耗
    pub error: Option<String>,      // 错误信息
}
```

## 5. 存储表结构

### MDBX 表定义

```rust
/// 所有数据库表
pub struct DualvmTableSet;

impl Tables for DualvmTableSet {
    type Tables = (
        DualvmBlocks,       // 区块头
        DualvmAccounts,     // EVM 账户
        DualvmCounters,     // DexVM 计数器
        DualvmStorage,      // 合约存储
        DualvmTxHashes,     // 交易索引
        DualvmTransactions, // 完整交易
    );
}
```

### 表结构详情

```
┌──────────────────────────────────────────────────────────────────────────┐
│                           DualvmBlocks                                   │
├──────────────────────────────────────────────────────────────────────────┤
│ Key: u64 (block number)                                                  │
│ Value: StoredDualvmBlock                                                 │
│   ├── number: u64                                                        │
│   ├── hash: B256                                                         │
│   ├── parent_hash: B256                                                  │
│   ├── timestamp: u64                                                     │
│   ├── gas_limit: u64                                                     │
│   ├── gas_used: u64                                                      │
│   ├── miner: Address                                                     │
│   ├── evm_state_root: B256                                               │
│   ├── dexvm_state_root: B256                                             │
│   ├── combined_state_root: B256                                          │
│   ├── transaction_hashes: Vec<B256>                                      │
│   ├── transaction_count: u64                                             │
│   └── signature: [u8; 65]                                                │
└──────────────────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────────────────┐
│                          DualvmAccounts                                  │
├──────────────────────────────────────────────────────────────────────────┤
│ Key: Address (20 bytes)                                                  │
│ Value: StoredDualvmAccount                                               │
│   ├── balance: U256                                                      │
│   ├── nonce: u64                                                         │
│   ├── code_hash: B256                                                    │
│   └── is_contract: bool                                                  │
└──────────────────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────────────────┐
│                          DualvmCounters                                  │
├──────────────────────────────────────────────────────────────────────────┤
│ Key: Address (20 bytes)                                                  │
│ Value: StoredCounter                                                     │
│   └── value: u64                                                         │
└──────────────────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────────────────┐
│                          DualvmStorage                                   │
├──────────────────────────────────────────────────────────────────────────┤
│ Key: StorageKey                                                          │
│   ├── address: Address                                                   │
│   └── slot: U256                                                         │
│ Value: StoredStorageValue                                                │
│   └── value: U256                                                        │
└──────────────────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────────────────┐
│                          DualvmTxHashes                                  │
├──────────────────────────────────────────────────────────────────────────┤
│ Key: B256 (tx_hash)                                                      │
│ Value: StoredTxInfo                                                      │
│   ├── block_number: u64                                                  │
│   └── tx_index: u32                                                      │
└──────────────────────────────────────────────────────────────────────────┘

┌──────────────────────────────────────────────────────────────────────────┐
│                        DualvmTransactions                                │
├──────────────────────────────────────────────────────────────────────────┤
│ Key: B256 (tx_hash)                                                      │
│ Value: StoredTransaction                                                 │
│   └── rlp_bytes: Vec<u8>                                                 │
└──────────────────────────────────────────────────────────────────────────┘
```

## 6. 配置结构

### NodeConfig (节点配置)

```rust
pub struct NodeConfig {
    pub chain_id: u64,          // 链 ID
    pub datadir: PathBuf,       // 数据目录
    pub evm_rpc_port: u16,      // EVM RPC 端口
    pub dexvm_rpc_port: u16,    // DexVM RPC 端口
}
```

### PoaConfig (共识配置)

```rust
pub struct PoaConfig {
    pub secret_key: SecretKey,      // 验证者私钥
    pub validator: Address,         // 验证者地址
    pub block_interval: Duration,   // 出块间隔
    pub starting_block: u64,        // 起始区块号
}
```

### P2pConfig (P2P 配置)

```rust
pub struct P2pConfig {
    pub secret_key: SecretKey,      // 节点私钥
    pub chain_id: u64,              // 链 ID
    pub genesis_hash: B256,         // 创世哈希
    pub listen_port: u16,           // 监听端口
    pub boot_nodes: Vec<TrustedPeer>, // 引导节点
    pub max_peers: usize,           // 最大连接数
}
```

## 7. RPC 结构

### TransactionReceipt (RPC 返回)

```rust
#[serde(rename_all = "camelCase")]
pub struct TransactionReceipt {
    pub transaction_hash: B256,
    pub transaction_index: U64,
    pub block_hash: B256,
    pub block_number: U64,
    pub from: Address,
    pub to: Option<Address>,
    pub cumulative_gas_used: U64,
    pub gas_used: U64,
    pub contract_address: Option<Address>,
    pub logs: Vec<Log>,
    pub logs_bloom: Bytes,      // 256 bytes
    pub status: U64,            // 1 = success, 0 = fail
    pub tx_type: U64,           // 0 = legacy
}
```

### BlockInfo (RPC 返回)

```rust
#[serde(rename_all = "camelCase")]
pub struct BlockInfo {
    pub number: U64,
    pub hash: B256,
    pub parent_hash: B256,
    pub sha3_uncles: B256,
    pub logs_bloom: Bytes,
    pub transactions_root: B256,
    pub state_root: B256,
    pub receipts_root: B256,
    pub miner: Address,
    pub difficulty: U256,
    pub total_difficulty: U256,
    pub extra_data: Bytes,
    pub size: U64,
    pub gas_limit: U64,
    pub gas_used: U64,
    pub timestamp: U64,
    pub transactions: Vec<B256>,
    pub uncles: Vec<B256>,
    pub nonce: B64,
    pub base_fee_per_gas: Option<U256>,
}
```

## 8. 常量定义

```rust
/// DexVM 路由地址 (发送到这个地址的交易路由到 DexVM)
pub const DEXVM_ROUTER_ADDRESS: Address =
    address!("ddddddddddddddddddddddddddddddddddddddd1");

/// 计数器预编译地址 (EVM 调用这个地址可以操作 DexVM 计数器)
pub const COUNTER_PRECOMPILE_ADDRESS: Address =
    address!("0000000000000000000000000000000000000100");

/// 操作码
pub const OP_INCREMENT: u8 = 0x00;
pub const OP_DECREMENT: u8 = 0x01;
pub const OP_QUERY: u8 = 0x02;

/// Gas 常量
pub const COUNTER_OP_GAS: u64 = 5000;
pub const QUERY_OP_GAS: u64 = 3000;
pub const BASE_TX_GAS: u64 = 21000;
```
