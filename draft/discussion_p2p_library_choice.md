# 讨论：P2P 库选择

## 问题

P2P 实现中，为何不直接使用 reth 现成的库（如 reth-network）？

## 结论

**并不是"完全自己实现"，而是复用底层协议、自定义上层逻辑。**

这是一个务实的中间路线：使用 reth 的 P2P 原语，但没有使用 reth 完整的 NetworkManager。

---

## 当前架构

```
┌─────────────────────────────────────────┐
│          dex-reth P2P 模块              │  ← 自己实现的上层逻辑
│  (service.rs, eth_handler.rs, peer.rs)  │     (~1600行代码)
├─────────────────────────────────────────┤
│          reth-eth-wire                  │  ← 复用 reth 的协议实现
│  (P2PStream, ECIES, ETH Messages)       │
├─────────────────────────────────────────┤
│          TCP + Tokio                    │  ← 标准网络层
└─────────────────────────────────────────┘
```

### 已使用的 reth 组件

| 组件 | 用途 |
|------|------|
| `reth-eth-wire` | P2PStream、ECIES 加密、ETH 协议消息类型 |
| `reth-network-peers` | PeerId、TrustedPeer、enode URI 解析 |
| `reth-ecies` | ECIES 加密/解密通信 |
| `reth-eth-wire-types` | ETH 协议消息类型定义 |

### 自己实现的部分

| 文件 | 行数 | 功能 |
|------|------|------|
| `service.rs` | ~572行 | P2P 服务主循环、连接管理 |
| `eth_handler.rs` | ~347行 | ETH 消息处理、区块同步 |
| `session.rs` | ~263行 | 握手、加密通信建立 |
| `peer.rs` | ~219行 | Peer 状态管理 |
| `config.rs` | ~153行 | 配置、密钥管理 |

---

## 为什么不直接用 reth 完整的 NetworkManager？

### 1. 耦合度过高

reth 的 `NetworkManager` 是为完整的 Ethereum 节点设计的，深度绑定了：
- 完整的 Kademlia DHT 节点发现
- 事务池广播和状态同步
- Execution Layer 和 Consensus 紧密集成
- 复杂的 peer 评分和惩罚机制

dex-reth 是双 VM 系统（EVM + DexVM），有自定义的状态根计算和事务路由，很难直接套用。

### 2. 简化的需求

dex-reth 采用了更简单的方案：
- **静态引导节点** 而不是完整的 DHT 发现
- **单 validator POA** 而不是复杂的共识
- 主要用于私有网络/测试网

完整的 reth P2P 对这个场景来说过于复杂。

### 3. 可控性

自己组装 P2P 层可以：
- 精确控制哪些消息被处理
- 更容易调试和理解数据流
- 避免 reth 内部更新带来的 breaking changes

### 4. ETH 协议兼容性

通过使用 `reth-eth-wire`，项目保持了与标准 Ethereum 节点（Geth、Besu）的**协议兼容性**（eth68 版本），同时又能自定义上层逻辑。

---

## 方案对比

| 方案 | 优点 | 缺点 |
|-----|------|------|
| 完全使用 reth P2P | 功能完整、经过生产验证 | 耦合度高、难以定制 |
| 完全自己实现 | 完全可控 | 工作量大、容易出 bug |
| **当前方案** | 复用成熟组件、可定制 | 需要维护集成代码 |

---

## 直接用 reth-network 的优势（供参考）

如果未来需要更完整的 P2P 功能，可以考虑迁移到 `reth-network`：
- 更成熟稳定
- 内置 peer discovery (discv4/discv5)
- 更好的连接管理和重连机制
- 交易广播已实现
- 持续维护

---

## 当前实现的关键特性

| 特性 | 实现方式 |
|------|----------|
| Peer 发现 | 静态引导节点列表 + DNS 解析 |
| 连接加密 | ECIES (Elliptic Curve Integrated Encryption Scheme) |
| 协议握手 | 3 层: ECIES → P2P → ETH Status |
| 消息编码 | RLP 编码 + alloy_rlp |
| 并发模式 | Tokio 异步 + mpsc/broadcast 通道 |
| 最大节点 | 可配置，默认 50 |
| 区块同步 | 请求/响应模式（headers → bodies） |
| ETH 兼容 | eth68 版本，可与标准节点互操作 |

---

## 状态

**已确定**：采用当前的中间路线方案。
