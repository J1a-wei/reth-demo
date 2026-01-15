# Reth 以太坊主网数据库结构

## 存储引擎

Reth 使用 **MDBX** (Modified LMDB) 作为主存储引擎，采用混合存储架构：
- **MDBX**: 存储动态变化的热数据
- **静态文件 (NippyJar)**: 存储不可变的历史数据（支持 Zstd/LZ4 压缩）

## 核心表定义

所有表定义在 `crates/storage/db-api/src/tables/mod.rs`，共 **31 个表**：

### 区块头表

| 表名 | Key | Value |
|------|-----|-------|
| CanonicalHeaders | BlockNumber (u64) | HeaderHash (B256) |
| Headers | BlockNumber | Header |
| HeaderNumbers | BlockHash (B256) | BlockNumber |
| HeaderTerminalDifficulties | BlockNumber | CompactU256 |
| BlockBodyIndices | BlockNumber | StoredBlockBodyIndices |
| BlockOmmers | BlockNumber | StoredBlockOmmers\<Header\> |
| BlockWithdrawals | BlockNumber | StoredBlockWithdrawals |

### 交易表

| 表名 | Key | Value |
|------|-----|-------|
| Transactions | TxNumber (u64) | TransactionSigned |
| Receipts | TxNumber | Receipt |
| TransactionHashNumbers | TxHash (B256) | TxNumber |
| TransactionBlocks | TxNumber | BlockNumber |
| TransactionSenders | TxNumber | Address |

### 账户状态表

| 表名 | Key | Value | SubKey |
|------|-----|-------|--------|
| PlainAccountState | Address | Account | - |
| HashedAccounts | B256 (hashed addr) | Account | - |
| AccountChangeSets | BlockNumber | AccountBeforeTx | Address (DUPSORT) |
| AccountsHistory | ShardedKey\<Address\> | BlockNumberList | - |

### 存储状态表

| 表名 | Key | Value | SubKey |
|------|-----|-------|--------|
| PlainStorageState | Address | StorageEntry | B256 (DUPSORT) |
| HashedStorages | B256 | StorageEntry | B256 (DUPSORT) |
| StorageChangeSets | BlockNumberAddress | StorageEntry | B256 (DUPSORT) |
| StoragesHistory | StorageShardedKey | BlockNumberList | - |

### 字节码表

| 表名 | Key | Value |
|------|-----|-------|
| Bytecodes | B256 (bytecode hash) | Bytecode |

### Merkle Patricia Trie 表

| 表名 | Key | Value | SubKey |
|------|-----|-------|--------|
| AccountsTrie | StoredNibbles | BranchNodeCompact | - |
| StoragesTrie | B256 (hashed addr) | StorageTrieEntry | StoredNibblesSubKey (DUPSORT) |
| AccountsTrieChangeSets | BlockNumber | TrieChangeSetsEntry | StoredNibblesSubKey (DUPSORT) |
| StoragesTrieChangeSets | BlockNumberHashedAddress | TrieChangeSetsEntry | StoredNibblesSubKey (DUPSORT) |

### 同步与元数据表

| 表名 | Key | Value |
|------|-----|-------|
| StageCheckpoints | StageId (String) | StageCheckpoint |
| StageCheckpointProgresses | StageId | Vec\<u8\> |
| PruneCheckpoints | PruneSegment | PruneCheckpoint |
| VersionHistory | u64 (unix timestamp) | ClientVersion |
| ChainState | ChainStateKey | BlockNumber |
| Metadata | String | Vec\<u8\> |

## 关键文件位置

```
crates/storage/
├── db/src/mdbx.rs              # MDBX 初始化配置
├── db-api/src/
│   ├── tables/mod.rs           # 表定义宏 (核心!)
│   ├── table.rs                # Table trait
│   └── models/                 # 数据模型
│       ├── accounts.rs         # Account 模型
│       ├── blocks.rs           # Block 模型
│       ├── sharded_key.rs      # 分片 Key
│       └── storage_sharded_key.rs
└── codecs/                     # 编码/解码
```

## 编码和压缩策略

**Encode/Decode 方案**:
- `BlockNumber`, `TxNumber`: 大端字节序 (8 字节)
- `Address`: 直接编码 (20 字节)
- `B256`: 直接编码 (32 字节)
- `String`: UTF-8 编码
- 复杂结构（Account, Receipt, Header 等）: 使用 `Compact` 编码

**Compress/Decompress**:
- 大多数值类型使用 `Compact` 编码进行压缩
- 历史列表使用 `RoaringTreemap` 位图压缩
- 静态文件使用 `NippyJar`（支持 Zstd 和 LZ4 压缩）

## 特殊设计

### DUPSORT 表

MDBX 支持特殊的 DUPSORT 表，允许相同的主键对应多个值。Reth 中有 7 个 DUPSORT 表：

1. **PlainStorageState**: Key = Address, SubKey = B256 (存储键)
2. **HashedStorages**: Key = B256, SubKey = B256
3. **AccountChangeSets**: Key = BlockNumber, SubKey = Address
4. **StorageChangeSets**: Key = BlockNumberAddress, SubKey = B256
5. **StoragesTrie**: Key = B256, SubKey = StoredNibblesSubKey
6. **AccountsTrieChangeSets**: Key = BlockNumber, SubKey = StoredNibblesSubKey
7. **StoragesTrieChangeSets**: Key = BlockNumberHashedAddress, SubKey = StoredNibblesSubKey

### ShardedKey\<T\>

用于大型历史数据的分片存储：
- 组成: 原始 Key + 最高 BlockNumber
- 作用: 快速定位历史数据范围
- 分片大小: 2,000 个索引/分片

### StorageShardedKey

用于存储历史数据：
- 组成: Address (20字节) + Storage Key (32字节) + BlockNumber (8字节)
- 总大小: 60 字节

### IntegerList

使用 `RoaringTreemap` 位图高效存储整数列表：
- 支持高效压缩
- 支持直接访问，无需完全解压

## 核心 Rust 结构体定义

### Account (账户)

**文件位置**: `crates/primitives-traits/src/account.rs`

```rust
/// An Ethereum account.
pub struct Account {
    /// Account nonce.
    pub nonce: u64,
    /// Account balance.
    pub balance: U256,
    /// Hash of the account's bytecode.
    pub bytecode_hash: Option<B256>,
}
```

**说明**:
- `nonce`: 账户的交易计数器，防止重放攻击
- `balance`: 账户余额（单位: wei）
- `bytecode_hash`: 合约代码的 keccak256 哈希，普通账户为 `None`

---

### Header (区块头)

**来源**: `alloy_consensus::Header` (重新导出于 `crates/primitives-traits/src/header/sealed.rs`)

```rust
/// Ethereum block header.
pub struct Header {
    /// Parent block hash.
    pub parent_hash: B256,
    /// Ommers (uncle blocks) hash.
    pub ommers_hash: B256,
    /// Beneficiary address (miner/validator).
    pub beneficiary: Address,
    /// State root hash.
    pub state_root: B256,
    /// Transactions root hash.
    pub transactions_root: B256,
    /// Receipts root hash.
    pub receipts_root: B256,
    /// Bloom filter for logs.
    pub logs_bloom: Bloom,
    /// Block difficulty (PoW, deprecated in PoS).
    pub difficulty: U256,
    /// Block number.
    pub number: u64,
    /// Gas limit.
    pub gas_limit: u64,
    /// Gas used.
    pub gas_used: u64,
    /// Block timestamp (seconds since epoch).
    pub timestamp: u64,
    /// Extra data (max 32 bytes).
    pub extra_data: Bytes,
    /// Mix hash (PoW) / prevRandao (PoS).
    pub mix_hash: B256,
    /// Nonce (PoW, deprecated in PoS).
    pub nonce: B64,
    /// Base fee per gas (EIP-1559).
    pub base_fee_per_gas: Option<u64>,
    /// Withdrawals root (EIP-4895, Shanghai).
    pub withdrawals_root: Option<B256>,
    /// Blob gas used (EIP-4844, Cancun).
    pub blob_gas_used: Option<u64>,
    /// Excess blob gas (EIP-4844, Cancun).
    pub excess_blob_gas: Option<u64>,
    /// Parent beacon block root (EIP-4788, Cancun).
    pub parent_beacon_block_root: Option<B256>,
    /// Requests hash (EIP-7685, Prague).
    pub requests_hash: Option<B256>,
}
```

---

### SealedHeader (密封区块头)

**文件位置**: `crates/primitives-traits/src/header/sealed.rs`

```rust
/// Seals the header with the block hash.
/// Uses lazy sealing to avoid hashing until needed.
pub struct SealedHeader<H = Header> {
    /// Block hash (lazily computed).
    hash: OnceLock<BlockHash>,
    /// Locked Header fields.
    header: H,
}
```

---

### Block trait (区块接口)

**文件位置**: `crates/primitives-traits/src/block/mod.rs`

```rust
/// Abstraction of block data type.
pub trait Block {
    /// Header part of the block.
    type Header: BlockHeader;
    /// The block's body contains transactions and additional data.
    type Body: BlockBody<OmmerHeader = Self::Header>;

    /// Create new block instance.
    fn new(header: Self::Header, body: Self::Body) -> Self;
    /// Returns reference to block header.
    fn header(&self) -> &Self::Header;
    /// Returns reference to block body.
    fn body(&self) -> &Self::Body;
    /// Splits the block into its header and body.
    fn split(self) -> (Self::Header, Self::Body);
}
```

**实现**: `alloy_consensus::Block<T, H>` 实现了该 trait。

---

### SealedBlock (密封区块)

**文件位置**: `crates/primitives-traits/src/block/sealed.rs`

```rust
/// Sealed full block composed of the block's header and body.
/// Uses lazy sealing to avoid hashing the header until needed.
pub struct SealedBlock<B: Block> {
    /// Sealed Header.
    header: SealedHeader<B::Header>,
    /// The block's body.
    body: B::Body,
}
```

---

### Transaction (交易枚举)

**文件位置**: `crates/ethereum/primitives/src/transaction.rs`

```rust
/// A raw transaction.
/// Transaction types were introduced in EIP-2718.
pub enum Transaction {
    /// Legacy transaction (type 0x0).
    Legacy(TxLegacy),
    /// Transaction with AccessList (EIP-2930), type 0x1.
    Eip2930(TxEip2930),
    /// Transaction with priority fee (EIP-1559), type 0x2.
    Eip1559(TxEip1559),
    /// Shard Blob Transactions (EIP-4844), type 0x3.
    Eip4844(TxEip4844),
    /// EOA Set Code Transactions (EIP-7702), type 0x4.
    Eip7702(TxEip7702),
}
```

**交易类型说明**:
| 类型 | EIP | 描述 |
|------|-----|------|
| Legacy | - | 传统交易，包含 gasPrice |
| Eip2930 | EIP-2930 | 带访问列表的交易 |
| Eip1559 | EIP-1559 | 动态费用交易 (maxFeePerGas, maxPriorityFeePerGas) |
| Eip4844 | EIP-4844 | Blob 交易 (用于 L2 数据) |
| Eip7702 | EIP-7702 | EOA 代码设置交易 |

---

### TransactionSigned (签名交易)

**文件位置**: `crates/ethereum/primitives/src/transaction.rs`

```rust
/// Signed Ethereum transaction.
pub struct TransactionSigned {
    /// Transaction hash (lazily computed).
    hash: OnceLock<TxHash>,
    /// The transaction signature values.
    signature: Signature,
    /// Raw transaction info.
    transaction: Transaction,
}
```

---

### TxLegacy (传统交易)

**来源**: `alloy_consensus::TxLegacy`

```rust
/// Legacy transaction.
pub struct TxLegacy {
    /// Chain ID (optional, EIP-155).
    pub chain_id: Option<ChainId>,
    /// Transaction nonce.
    pub nonce: u64,
    /// Gas price.
    pub gas_price: u128,
    /// Gas limit.
    pub gas_limit: u64,
    /// Recipient address (None for contract creation).
    pub to: TxKind,
    /// Transfer value.
    pub value: U256,
    /// Input data.
    pub input: Bytes,
}
```

---

### TxEip1559 (EIP-1559 交易)

**来源**: `alloy_consensus::TxEip1559`

```rust
/// EIP-1559 transaction.
pub struct TxEip1559 {
    /// Chain ID.
    pub chain_id: ChainId,
    /// Transaction nonce.
    pub nonce: u64,
    /// Gas limit.
    pub gas_limit: u64,
    /// Maximum fee per gas.
    pub max_fee_per_gas: u128,
    /// Maximum priority fee per gas (tip).
    pub max_priority_fee_per_gas: u128,
    /// Recipient address.
    pub to: TxKind,
    /// Transfer value.
    pub value: U256,
    /// Access list.
    pub access_list: AccessList,
    /// Input data.
    pub input: Bytes,
}
```

---

### TxEip4844 (Blob 交易)

**来源**: `alloy_consensus::TxEip4844`

```rust
/// EIP-4844 Blob transaction.
pub struct TxEip4844 {
    /// Chain ID.
    pub chain_id: ChainId,
    /// Transaction nonce.
    pub nonce: u64,
    /// Gas limit.
    pub gas_limit: u64,
    /// Maximum fee per gas.
    pub max_fee_per_gas: u128,
    /// Maximum priority fee per gas.
    pub max_priority_fee_per_gas: u128,
    /// Recipient address (must be contract).
    pub to: Address,
    /// Transfer value.
    pub value: U256,
    /// Access list.
    pub access_list: AccessList,
    /// Blob versioned hashes.
    pub blob_versioned_hashes: Vec<B256>,
    /// Maximum fee per blob gas.
    pub max_fee_per_blob_gas: u128,
    /// Input data.
    pub input: Bytes,
}
```

---

## 总结

Reth 通过以下设计实现高性能:

1. **分层存储**: 热数据用 MDBX，历史数据用静态文件
2. **高效编码**: 使用 Compact 编码和 RoaringTreemap 压缩
3. **DUPSORT 表**: 优化历史查询效率
4. **Sharded 设计**: 解决大数据集的存储问题
5. **类型安全**: 强类型 Key/Value 确保数据完整性
