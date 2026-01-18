# dex-reth

基于 reth 的双虚拟机区块链节点，同时运行 EVM 和自定义 DexVM。

A dual virtual machine blockchain node built on top of [reth](https://github.com/paradigmxyz/reth).

## 项目概述 / Overview

dex-reth 是一个双虚拟机区块链系统，在单个节点中运行两个 VM：
- **EVM (Ethereum Virtual Machine)**: 标准智能合约执行
- **DexVM (Custom VM)**: 简单计数器状态管理

系统为每个 VM 计算独立的状态根，然后组合：
```
combined_root = keccak256(evm_root || dexvm_root)
```

## 功能特性 / Features

- 双 VM 执行 (EVM + DexVM)
- POA (Proof of Authority) 共识
- MDBX 持久化存储（EVM 账户 + DexVM 计数器）
- EVM JSON-RPC (以太坊兼容)
- DexVM REST API
- P2P 网络 (eth devp2p 协议)
- 全节点区块同步

## 环境要求 / Requirements

- Rust 1.84+
- Linux/macOS

## 构建 / Build

```bash
# 构建项目 (Debug)
cargo build

# 构建项目 (Release)
cargo build --release

# 运行测试
cargo test

# 代码格式化
cargo +nightly fmt --all

# 代码检查
cargo clippy --all-features
```

## 快速开始 / Quick Start

### 运行节点 / Run Node

```bash
# 基础模式 (仅 RPC，不出块)
cargo run --release --bin dex-reth -- --datadir ./data

# 启用 POA 共识 (自动出块)
cargo run --release --bin dex-reth -- \
    --datadir ./data \
    --genesis genesis.json \
    --enable-consensus \
    --validator 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266 \
    --block-interval-ms 500

# 启用 P2P 网络
cargo run --release --bin dex-reth -- \
    --datadir ./data \
    --enable-consensus \
    --enable-p2p \
    --p2p-port 30303
```

### 使用启动脚本 / Use Scripts

```bash
# 启动验证者节点
./scripts/start_validator.sh

# 启动全节点 (只读)
./scripts/start_fullnode.sh
```

### 命令行参数 / CLI Options

| 参数 | 默认值 | 描述 |
|------|--------|------|
| `--evm-rpc-port` | 8545 | EVM JSON-RPC 端口 |
| `--dexvm-port` | 9845 | DexVM REST API 端口 |
| `--p2p-port` | 30303 | P2P 监听端口 |
| `--enable-p2p` | false | 启用 P2P 网络 |
| `--enable-consensus` | false | 启用 POA 共识 |
| `--validator` | 0x...0001 | 验证者地址 |
| `--block-interval-ms` | 500 | 出块间隔 (毫秒) |
| `--datadir` | ./data | 数据目录 |
| `--genesis` | - | 创世文件路径 |
| `--log-level` | info | 日志级别 |
| `--max-peers` | 50 | 最大 P2P 连接数 |

## 测试 / Testing

### 单元测试 / Unit Tests

```bash
cargo test
```

### 三个测试流程 / Three Test Flows

本项目包含三个端到端测试流程，验证双 VM 系统的完整功能：

#### 流程 1: Foundry 部署 PiggyBank 合约 (验证 EVM)

测试 EVM 合约部署、调用、状态存储功能。

**前置条件**:
- 安装 Foundry: `curl -L https://foundry.paradigm.xyz | bash && foundryup`
- 节点已启动并使用创世配置

**运行测试**:
```bash
./scripts/test_flow1_piggybank.sh
```

**测试内容**:
1. 检查节点状态
2. 部署 PiggyBank.sol 合约
3. 存款测试 (deposit 1 ETH)
4. 查询合约余额
5. 取款测试 (withdraw 0.5 ETH)

**手动测试**:
```bash
# 部署合约
cd contracts
forge create PiggyBank.sol:PiggyBank \
    --rpc-url http://127.0.0.1:8545 \
    --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 \
    --legacy --broadcast

# 存款 1 ETH
cast send <contract_address> "deposit()" \
    --value 1ether \
    --rpc-url http://127.0.0.1:8545 \
    --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 \
    --legacy

# 查询余额
cast call <contract_address> "balances(address)" 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266 \
    --rpc-url http://127.0.0.1:8545

# 取款 0.5 ETH
cast send <contract_address> "withdraw(uint256)" 500000000000000000 \
    --rpc-url http://127.0.0.1:8545 \
    --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 \
    --legacy
```

---

#### 流程 2: EVM 预编译合约调用 DexVM (跨 VM 调用)

测试通过 EVM 预编译合约 (0x100) 调用 DexVM 计数器，验证跨 VM 调用和原子性回滚。

**预编译合约地址**: `0x0000000000000000000000000000000000000100`

**Calldata 格式**: `[op: 1 byte][amount: 8 bytes big-endian]`
- `0x00` = Increment (增加计数器)
- `0x01` = Decrement (减少计数器)
- `0x02` = Query (查询计数器)

**运行测试**:
```bash
./scripts/test_flow2_precompile.sh
```

**测试内容**:
1. 检查节点状态
2. 查询初始 DexVM 计数器
3. 通过预编译合约增加计数器 (+10)
4. 查询 DexVM 计数器 (验证跨 VM 状态同步)
5. 通过预编译合约查询计数器 (eth_call)
6. 通过预编译合约减少计数器 (-5)
7. 获取状态根

**手动测试**:
```bash
# 增加计数器 +10 (op=0x00, amount=10=0x000000000000000a)
cast send 0x0000000000000000000000000000000000000100 0x00000000000000000a \
    --rpc-url http://127.0.0.1:8545 \
    --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 \
    --legacy

# 减少计数器 -5 (op=0x01, amount=5=0x0000000000000005)
cast send 0x0000000000000000000000000000000000000100 0x010000000000000005 \
    --rpc-url http://127.0.0.1:8545 \
    --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 \
    --legacy

# 查询计数器 (eth_call, op=0x02)
curl -X POST http://127.0.0.1:8545 \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"eth_call","params":[{"from":"0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266","to":"0x0000000000000000000000000000000000000100","data":"0x020000000000000000"},"latest"],"id":1}'

# 验证 DexVM 状态
curl http://127.0.0.1:9845/api/v1/counter/0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266
```

---

#### 流程 3: DexVM RPC 直接操作计数器

测试 DexVM REST API 独立接口功能。

**运行测试**:
```bash
./scripts/test_flow3_dexvm_rpc.sh
```

**测试内容**:
1. 健康检查
2. 查询初始计数器
3. 增加计数器 (+10)
4. 增加计数器 (+5)
5. 减少计数器 (-3)
6. 查询最终计数器 (预期变化: +12)
7. 测试减少溢出 (应该失败)
8. 获取状态根
9. 测试其他地址

**手动测试**:
```bash
# 健康检查
curl http://127.0.0.1:9845/health

# 查询计数器
curl http://127.0.0.1:9845/api/v1/counter/0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266

# 增加计数器
curl -X POST http://127.0.0.1:9845/api/v1/counter/0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266/increment \
    -H "Content-Type: application/json" \
    -d '{"amount": 10}'

# 减少计数器
curl -X POST http://127.0.0.1:9845/api/v1/counter/0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266/decrement \
    -H "Content-Type: application/json" \
    -d '{"amount": 5}'

# 获取状态根
curl http://127.0.0.1:9845/api/v1/state-root
```

---

#### 流程 4: P2P 全节点同步测试

测试验证者和全节点之间的 P2P 区块同步功能。

**运行测试**:
```bash
./scripts/test_flow4_p2p_sync.sh
```

**测试内容**:
1. 清理数据目录
2. 启动验证者节点（带 P2P）
3. 获取验证者 enode URL
4. 启动全节点，连接验证者
5. 在验证者上发送 DexVM 交易
6. 等待区块同步
7. 验证区块高度和状态一致性

**手动测试**:
```bash
# 终端 1: 启动验证者
./scripts/start_validator.sh
# 记录日志中的 Enode URL: enode://...@127.0.0.1:30303

# 终端 2: 启动全节点连接验证者
BOOTNODE="enode://...@127.0.0.1:30303" ./scripts/start_fullnode.sh

# 终端 3: 检查同步状态
# 验证者区块高度
curl -s http://127.0.0.1:8545 -X POST -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'

# 全节点区块高度
curl -s http://127.0.0.1:8546 -X POST -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'
```

---

### 运行所有测试 / Run All Tests

```bash
# 先启动节点
./scripts/start_validator.sh

# 在另一个终端运行测试
./scripts/test_flow1_piggybank.sh  # EVM 合约测试
./scripts/test_flow2_precompile.sh # 跨 VM 调用测试
./scripts/test_flow3_dexvm_rpc.sh  # DexVM API 测试
./scripts/test_flow4_p2p_sync.sh   # P2P 同步测试 (独立运行，会自动启动节点)
```

### 测试验证点 / Test Verification

| 流程 | 验证点 |
|------|--------|
| 流程 1 | EVM 合约部署、调用、状态存储正常 |
| 流程 2 | EVM → 预编译 → DexVM 跨 VM 调用正常，原子性回滚正常 |
| 流程 3 | DexVM 独立 RPC 接口正常 |
| 流程 4 | P2P 区块同步正常，全节点能同步验证者区块 |

### 集成测试脚本 / Integration Test Scripts

```bash
# EVM JSON-RPC 测试
./scripts/test_evm.sh

# DexVM 计数器测试
./scripts/test_counter.sh

# 端到端测试
./scripts/test_e2e.sh
```

### 手动测试 / Manual Testing

#### EVM JSON-RPC

```bash
# 获取链 ID
curl -X POST http://localhost:8545 \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}'

# 获取区块号
curl -X POST http://localhost:8545 \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'

# 查询账户余额
curl -X POST http://localhost:8545 \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"eth_getBalance","params":["0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266","latest"],"id":1}'

# 获取最新区块
curl -X POST http://localhost:8545 \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"eth_getBlockByNumber","params":["latest",false],"id":1}'

# 获取 gas 价格
curl -X POST http://localhost:8545 \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"eth_gasPrice","params":[],"id":1}'

# 客户端版本
curl -X POST http://localhost:8545 \
    -H "Content-Type: application/json" \
    -d '{"jsonrpc":"2.0","method":"web3_clientVersion","params":[],"id":1}'
```

#### DexVM REST API

```bash
# 健康检查
curl http://localhost:9845/health

# 查询计数器
curl http://localhost:9845/api/v1/counter/0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266

# 增加计数器
curl -X POST http://localhost:9845/api/v1/counter/0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266/increment \
    -H "Content-Type: application/json" \
    -d '{"amount": 10}'

# 减少计数器
curl -X POST http://localhost:9845/api/v1/counter/0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266/decrement \
    -H "Content-Type: application/json" \
    -d '{"amount": 5}'

# 获取状态根
curl http://localhost:9845/api/v1/state-root
```

## API 端点 / API Endpoints

| 端口 | 服务 | 协议 |
|------|------|------|
| 8545 | EVM RPC | JSON-RPC |
| 9845 | DexVM API | REST |
| 30303 | P2P | devp2p |

### EVM JSON-RPC 方法

| 方法 | 描述 |
|------|------|
| `eth_chainId` | 获取链 ID |
| `eth_blockNumber` | 获取当前区块号 |
| `eth_getBalance` | 查询账户余额 |
| `eth_getTransactionCount` | 获取账户 nonce |
| `eth_sendRawTransaction` | 发送签名交易 |
| `eth_getBlockByNumber` | 按区块号查询区块 |
| `eth_getBlockByHash` | 按哈希查询区块 |
| `eth_getTransactionReceipt` | 获取交易回执 |
| `eth_gasPrice` | 获取 gas 价格 |
| `eth_call` | 执行只读调用 |
| `eth_estimateGas` | 估算 gas |
| `web3_clientVersion` | 获取客户端版本 |
| `net_version` | 获取网络版本 |

### DexVM REST API

| 方法 | 路径 | 描述 |
|------|------|------|
| GET | `/health` | 健康检查 |
| GET | `/api/v1/counter/:address` | 查询计数器 |
| POST | `/api/v1/counter/:address/increment` | 增加计数器 |
| POST | `/api/v1/counter/:address/decrement` | 减少计数器 |
| GET | `/api/v1/state-root` | 获取状态根 |

## 创世文件格式 / Genesis Format

```json
{
  "config": {
    "chainId": 13337,
    "homesteadBlock": 0,
    "eip150Block": 0,
    "eip155Block": 0,
    "eip158Block": 0,
    "byzantiumBlock": 0,
    "constantinopleBlock": 0,
    "petersburgBlock": 0,
    "istanbulBlock": 0,
    "berlinBlock": 0,
    "londonBlock": 0,
    "shanghaiTime": 0,
    "cancunTime": 0
  },
  "alloc": {
    "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266": {
      "balance": "10000000000000000000000"
    }
  }
}
```

## 测试账户 / Test Accounts

创世文件包含 10 个预置账户（Hardhat 默认账户），每个账户有 10,000 ETH：

| 地址 | 私钥 |
|------|------|
| 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266 | 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 |
| 0x70997970C51812dc3A010C7d01b50e0d17dc79C8 | 0x59c6995e998f97a5a0044966f0945389dc9e86dae88c7a8412f4603b6b78690d |
| 0x3C44CdDdB6a900fa2b585dd299e03d12FA4293BC | 0x5de4111afa1a4b94908f83103eb1f1706367c2e68ca870fc3fb9a804cdab365a |
| 0x90F79bf6EB2c4f870365E785982E1f101E93b906 | 0x7c852118294e51e653712a81e05800f419141751be58f605c371e15141b007a6 |
| 0x15d34AAf54267DB7D7c367839AAf71A00a2C6A65 | 0x47e179ec197488593b187f80a00eb0da91f1b9d0b13f8733639f19c30a34926a |

## 交易路由 / Transaction Routing

交易根据目标地址路由：
- 地址 `0xddddddddddddddddddddddddddddddddddddddd1` → DexVM
- 其他所有地址 → EVM

### DexVM Calldata 格式

```
[op_type: u8][amount: u64 big-endian]
```
- `0` = Increment (增加)
- `1` = Decrement (减少)
- `2` = Query (查询)

### 预编译合约 / Precompile Contract

计数器预编译地址：`0x0000000000000000000000000000000000000100`

**Calldata 格式**: `[op: 1 byte][amount: 8 bytes big-endian]`
- `0x00` + amount = Increment (增加计数器)
- `0x01` + amount = Decrement (减少计数器)
- `0x02` + padding = Query (查询计数器)

**示例**:
```bash
# 增加计数器 10: 0x00 + 000000000000000a
cast send 0x0000000000000000000000000000000000000100 0x00000000000000000a ...

# 减少计数器 5: 0x01 + 0000000000000005
cast send 0x0000000000000000000000000000000000000100 0x010000000000000005 ...

# 查询计数器: 0x02 + padding
cast call 0x0000000000000000000000000000000000000100 0x020000000000000000 ...
```

## 项目结构 / Project Structure

```
dex-reth/
├── bin/dex-reth/          # CLI 入口
│   └── main.rs
├── crates/
│   ├── primitives/        # 核心类型 (DualVmTransaction, DexVmReceipt)
│   ├── dexvm/             # DexVM 实现 (state.rs, executor.rs, precompiles.rs)
│   ├── evm/               # EVM 执行器 (基于 revm 27)
│   ├── storage/           # MDBX 数据库
│   ├── rpc/               # REST API (Axum) + JSON-RPC (jsonrpsee)
│   ├── p2p/               # P2P 网络 (eth devp2p)
│   └── node/              # 节点集成 (DualVmNode, POA 共识, 跨 VM 执行)
├── contracts/             # Solidity 合约
│   └── PiggyBank.sol      # 测试存钱罐合约
├── scripts/               # 测试脚本
│   ├── start_validator.sh       # 启动验证者节点
│   ├── start_fullnode.sh        # 启动全节点
│   ├── test_flow1_piggybank.sh  # 流程1: EVM 合约部署测试
│   ├── test_flow2_precompile.sh # 流程2: 跨 VM 预编译调用测试
│   ├── test_flow3_dexvm_rpc.sh  # 流程3: DexVM RPC 测试
│   ├── test_flow4_p2p_sync.sh   # 流程4: P2P 全节点同步测试
│   ├── test_evm.sh              # EVM JSON-RPC 测试
│   ├── test_counter.sh          # DexVM 计数器测试
│   └── test_e2e.sh              # 端到端测试
├── draft/                 # 设计文档
│   ├── discussion_test_flows.md      # 测试流程设计
│   └── discussion_block_structure.md # 区块结构设计
├── genesis.json           # 创世配置
├── Cargo.toml
├── CLAUDE.md              # 开发指南
└── README.md
```

## 状态根计算 / State Root

系统为每个 VM 计算独立的状态根，然后组合成最终的 `combined_state_root`。

### 计算公式

| 状态根 | 计算方式 | 代码位置 |
|--------|---------|----------|
| EVM | `keccak256(addr + balance + nonce + code_hash)` | `crates/storage/src/state_store.rs:301` |
| DexVM | `keccak256(sorted(addr + counter))` | `crates/dexvm/src/state.rs:58` |
| Combined | `keccak256(evm_root \|\| dexvm_root)` | `crates/node/src/executor.rs:195` |

### EVM 状态根

遍历所有账户，按地址排序后计算：

```rust
// crates/storage/src/state_store.rs:301-332
pub fn state_root(&self) -> B256 {
    let mut data = Vec::new();
    for (addr, account) in walker {
        data.extend_from_slice(addr.as_slice());           // 20 bytes
        data.extend_from_slice(&account.balance.to_be_bytes::<32>()); // 32 bytes
        data.extend_from_slice(&account.nonce.to_be_bytes());         // 8 bytes
        data.extend_from_slice(account.code_hash.as_slice());         // 32 bytes
    }
    keccak256(&data)
}
```

### DexVM 状态根

遍历所有计数器，按地址排序后计算：

```rust
// crates/dexvm/src/state.rs:58-75
pub fn state_root(&self) -> B256 {
    let mut accounts: Vec<_> = self.counters.iter().collect();
    accounts.sort_by_key(|(addr, _)| *addr);

    let mut data = Vec::new();
    for (addr, counter) in accounts {
        data.extend_from_slice(addr.as_slice());  // 20 bytes
        data.extend_from_slice(&counter.to_be_bytes()); // 8 bytes
    }
    keccak256(&data)
}
```

### 组合状态根

简单拼接两个状态根后哈希：

```rust
// crates/node/src/executor.rs:195-202
fn combine_state_roots(&self, evm_root: B256, dexvm_root: B256) -> B256 {
    let mut data = Vec::with_capacity(64);
    data.extend_from_slice(evm_root.as_slice());   // 32 bytes
    data.extend_from_slice(dexvm_root.as_slice()); // 32 bytes
    keccak256(&data)
}
```

### 调用流程

在区块执行完成后计算状态根：

```rust
// crates/node/src/executor.rs:136-138
let evm_state_root = evm_executor.state_root();
let dexvm_state_root = dexvm_executor.state_root();
let combined_state_root = self.combine_state_roots(evm_state_root, dexvm_state_root);
```

## 依赖版本 / Dependencies

| 依赖 | 版本 |
|------|------|
| reth | v1.5.1 |
| alloy | v1.x |
| revm | 27.x |
| axum | latest |
| jsonrpsee | latest |
| tokio | latest |
| Rust | 1.84+ |

## 开发说明 / Development Notes

- POA 共识：单验证者，可配置出块间隔（默认 500ms）
- 数据持久化到 `./data` 目录
- 日志级别：debug, info, warn, error
- **状态持久化**：
  - EVM 账户余额和 nonce 持久化到 MDBX
  - DexVM 计数器状态持久化到 MDBX
  - 节点重启后自动恢复所有状态
- **P2P 同步**：
  - 验证者节点广播新区块
  - 全节点通过 devp2p 协议同步区块头和区块体
  - 支持通过 `--bootnodes` 参数连接验证者

## License

MIT OR Apache-2.0
