# DEX-RETH 架构文档索引

## 文档列表

| 编号 | 文件 | 内容 |
|------|------|------|
| 01 | [architecture-overview.md](01-architecture-overview.md) | 系统架构总览、模块依赖关系 |
| 02 | [transaction-flow.md](02-transaction-flow.md) | 交易处理流程、路由决策 |
| 03 | [block-production.md](03-block-production.md) | 区块生产与 POA 共识 |
| 04 | [p2p-networking.md](04-p2p-networking.md) | P2P 网络通信与区块同步 |
| 05 | [data-structures.md](05-data-structures.md) | 核心数据结构定义 |
| 06 | [storage-layer.md](06-storage-layer.md) | 存储层与 MDBX 数据库 |
| 07 | [cross-vm-execution.md](07-cross-vm-execution.md) | 跨 VM 执行与预编译合约 |

## 快速导航

### 概念理解
- **什么是 Dual VM?** → [01-architecture-overview.md](01-architecture-overview.md)
- **交易如何路由?** → [02-transaction-flow.md](02-transaction-flow.md)
- **如何实现跨 VM 调用?** → [07-cross-vm-execution.md](07-cross-vm-execution.md)

### 核心流程
- **区块如何产生?** → [03-block-production.md](03-block-production.md)
- **节点如何同步?** → [04-p2p-networking.md](04-p2p-networking.md)
- **数据如何持久化?** → [06-storage-layer.md](06-storage-layer.md)

### 数据结构
- **交易格式** → [05-data-structures.md#1-交易类型](05-data-structures.md)
- **区块格式** → [05-data-structures.md#2-区块结构](05-data-structures.md)
- **存储表结构** → [05-data-structures.md#5-存储表结构](05-data-structures.md)

## 关键地址

| 地址 | 用途 |
|------|------|
| `0xddddddddddddddddddddddddddddddddddddddd1` | DexVM 路由地址 (纯 DexVM 交易) |
| `0x0000000000000000000000000000000000000100` | 计数器预编译 (跨 VM 调用) |

## 端口服务

| 端口 | 服务 | 协议 |
|------|------|------|
| 8545 | EVM JSON-RPC | HTTP |
| 9845 | DexVM REST API | HTTP |
| 30303 | P2P | devp2p |

## 项目结构

```
dex-reth/
├── bin/dex-reth/           # CLI 入口
├── crates/
│   ├── primitives/         # 核心类型
│   ├── dexvm/              # DexVM 实现
│   ├── storage/            # MDBX 存储
│   ├── rpc/                # RPC 服务
│   ├── p2p/                # P2P 网络
│   └── node/               # 节点编排
├── scripts/                # 启动脚本
├── notes/                  # 架构文档 (本目录)
├── genesis.json            # 创世配置
└── CLAUDE.md               # 开发指南
```

## 常用命令

```bash
# 构建
cargo build --release

# 启动验证者节点
./scripts/start_validator.sh

# 启动全节点
./scripts/start_fullnode.sh

# 查询区块高度
curl -s http://localhost:8545 -X POST \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'

# 查询计数器
curl -s http://localhost:9845/api/v1/counter/0x1234...

# 增加计数器
curl -s -X POST http://localhost:9845/api/v1/counter/0x1234.../increment \
  -H "Content-Type: application/json" \
  -d '{"amount": 100}'
```

## 核心概念图

```
                    ┌─────────────────────────────────────┐
                    │           Transaction               │
                    └─────────────────┬───────────────────┘
                                      │
                    ┌─────────────────▼───────────────────┐
                    │         Transaction Router          │
                    │   (DualVmTransaction::from_ethereum_tx)│
                    └─────────────────┬───────────────────┘
                                      │
            ┌─────────────────────────┼─────────────────────────┐
            │                         │                         │
            ▼                         ▼                         ▼
    ┌───────────────┐         ┌───────────────┐         ┌───────────────┐
    │   EVM TX      │         │  Precompile   │         │   DexVM TX    │
    │ (普通交易)     │         │ (跨 VM 调用)  │         │ (计数器操作)   │
    └───────┬───────┘         └───────┬───────┘         └───────┬───────┘
            │                         │                         │
            ▼                         ▼                         ▼
    ┌───────────────┐         ┌───────────────┐         ┌───────────────┐
    │SimpleEvmExecutor│       │PrecompileExecutor│      │DexVmExecutor  │
    └───────┬───────┘         └───────┬───────┘         └───────┬───────┘
            │                         │                         │
            ▼                         ▼                         ▼
    ┌───────────────┐         ┌───────────────┐         ┌───────────────┐
    │  StateStore   │◄────────│ Both Stores   │────────►│  DexVmState   │
    │ (EVM 账户)    │         │ (原子更新)     │         │ (计数器)       │
    └───────────────┘         └───────────────┘         └───────────────┘
            │                         │                         │
            └─────────────────────────┼─────────────────────────┘
                                      │
                    ┌─────────────────▼───────────────────┐
                    │         State Root 计算             │
                    │  combined = keccak256(evm || dexvm) │
                    └─────────────────────────────────────┘
```
