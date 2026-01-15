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
- MDBX 持久化存储
- EVM JSON-RPC (以太坊兼容)
- DexVM REST API
- P2P 网络 (eth devp2p 协议)

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

存款/取款预编译地址：`0x0000000000000000000000000000000000000100`
- 发送 ETH 且 calldata 为空 = 存款
- 发送带金额的 calldata = 取款
- 空调用 = 查询余额

## 项目结构 / Project Structure

```
dex-reth/
├── bin/dex-reth/          # CLI 入口
│   └── main.rs
├── crates/
│   ├── primitives/        # 核心类型 (DualVmTransaction, DexVmReceipt)
│   ├── dexvm/             # DexVM 实现 (state.rs, executor.rs)
│   ├── evm/               # EVM 执行器 (基于 revm 27)
│   ├── storage/           # MDBX 数据库
│   ├── rpc/               # REST API (Axum) + JSON-RPC (jsonrpsee)
│   ├── p2p/               # P2P 网络 (eth devp2p)
│   └── node/              # 节点集成 (DualVmNode, POA 共识)
├── scripts/               # 测试脚本
│   ├── start_validator.sh # 启动验证者节点
│   ├── start_fullnode.sh  # 启动全节点
│   ├── test_evm.sh        # EVM 测试
│   ├── test_counter.sh    # DexVM 测试
│   └── test_e2e.sh        # 端到端测试
├── genesis.json           # 创世配置
├── Cargo.toml
├── CLAUDE.md              # 开发指南
└── README.md
```

## 状态根计算 / State Root

- EVM: `keccak256(sorted_account_data)`
- DexVM: `keccak256(sorted_counter_data)`
- 组合: `keccak256(evm_root || dexvm_root)`

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

## License

MIT OR Apache-2.0
