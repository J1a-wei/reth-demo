# P2P 网络流程

## 1. P2P 架构

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                              P2P Service                                    │
├─────────────────────────────────────────────────────────────────────────────┤
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                         P2pConfig                                    │   │
│  │  ┌──────────────┬──────────────┬──────────────┬──────────────────┐  │   │
│  │  │ secret_key   │ chain_id     │ genesis_hash │ listen_port      │  │   │
│  │  │ (节点密钥)   │ (链 ID)      │ (创世哈希)   │ (监听端口)       │  │   │
│  │  └──────────────┴──────────────┴──────────────┴──────────────────┘  │   │
│  │  ┌──────────────┬──────────────┐                                    │   │
│  │  │ boot_nodes   │ max_peers    │                                    │   │
│  │  │ (引导节点)   │ (最大连接数) │                                    │   │
│  │  └──────────────┴──────────────┘                                    │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                        P2pHandle                                     │   │
│  │  ├── local_id: PeerId           (本地节点 ID)                        │   │
│  │  ├── cmd_tx: Sender<SessionCommand>  (命令通道)                      │   │
│  │  ├── event_tx: broadcast::Sender<P2pEvent>  (事件广播)               │   │
│  │  └── peer_count: Arc<AtomicUsize>  (连接计数)                        │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                       PeerManager                                    │   │
│  │  ├── connected_peers: HashMap<PeerId, PeerInfo>                      │   │
│  │  ├── pending_connections: HashSet<PeerId>                            │   │
│  │  └── max_peers: usize                                                │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                      EthHandler (per peer)                           │   │
│  │  ├── peer_id: PeerId                                                 │   │
│  │  ├── stream: TcpStream (加密连接)                                    │   │
│  │  ├── capabilities: Vec<Capability>                                   │   │
│  │  └── request_id: u64 (请求计数器)                                    │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
│                                                                             │
└─────────────────────────────────────────────────────────────────────────────┘
```

## 2. 节点连接流程

```mermaid
sequenceDiagram
    participant Local as 本地节点
    participant TCP as TCP 连接
    participant Remote as 远程节点

    Note over Local: 启动 P2P 服务

    alt 作为客户端 (连接 bootnode)
        Local->>TCP: 建立 TCP 连接
        TCP->>Remote: connect()
    else 作为服务端 (接受连接)
        Remote->>TCP: connect()
        TCP->>Local: accept()
    end

    Local->>Remote: ECIES 握手 (加密)
    Remote->>Local: ECIES 握手响应

    Local->>Remote: Hello (P2P 协议版本)
    Remote->>Local: Hello 响应

    Local->>Remote: Status (ETH 协议)
    Note over Local,Remote: chain_id, genesis_hash,<br/>best_block_hash, total_difficulty
    Remote->>Local: Status 响应

    alt 协议兼容
        Note over Local,Remote: 连接成功
        Local->>Local: 添加到 PeerManager
        Local->>Local: 启动 EthHandler
        Local->>Local: 发送 PeerConnected 事件
    else 协议不兼容
        Local->>Remote: Disconnect
        Note over Local,Remote: 连接失败
    end
```

## 3. ETH 协议消息

```mermaid
flowchart TD
    subgraph "入站消息 (从远程节点接收)"
        IN_NBH[NewBlockHashes<br/>区块哈希通告]
        IN_NB[NewBlock<br/>完整区块]
        IN_GH[GetBlockHeaders<br/>请求区块头]
        IN_H[BlockHeaders<br/>区块头响应]
        IN_GB[GetBlockBodies<br/>请求区块体]
        IN_B[BlockBodies<br/>区块体响应]
        IN_TX[Transactions<br/>交易广播]
    end

    subgraph "P2P 事件"
        E_NBH[P2pEvent::NewBlockHash]
        E_NB[P2pEvent::NewBlock]
        E_GHR[P2pEvent::GetBlockHeadersRequest]
        E_H[P2pEvent::BlockHeaders]
        E_GBR[P2pEvent::GetBlockBodiesRequest]
        E_B[P2pEvent::BlockBodies]
        E_TX[P2pEvent::Transactions]
    end

    IN_NBH --> E_NBH
    IN_NB --> E_NB
    IN_GH --> E_GHR
    IN_H --> E_H
    IN_GB --> E_GBR
    IN_B --> E_B
    IN_TX --> E_TX

    subgraph "出站命令 (发送到远程节点)"
        OUT_BB[BroadcastBlock<br/>广播新区块]
        OUT_GH[GetBlockHeaders<br/>请求区块头]
        OUT_SH[SendBlockHeaders<br/>发送区块头]
        OUT_GB[GetBlockBodies<br/>请求区块体]
        OUT_SB[SendBlockBodies<br/>发送区块体]
        OUT_TX[BroadcastTransactions<br/>广播交易]
    end
```

## 4. 区块同步流程 (全节点)

```mermaid
sequenceDiagram
    participant Fullnode as 全节点
    participant Sync as BlockSyncManager
    participant P2P as P2P Handle
    participant Validator as 验证者节点

    Note over Fullnode: 连接到验证者

    Fullnode->>Sync: 启动同步

    Sync->>P2P: GetBlockHeaders(start=1, count=512)
    P2P->>Validator: 请求区块头

    Validator-->>P2P: BlockHeaders [h1, h2, ..., h512]
    P2P-->>Sync: P2pEvent::BlockHeaders

    Sync->>Sync: 保存 headers 到 pending_body_requests

    loop 对每个 header
        Sync->>Sync: 计算 header_hash
    end

    Sync->>P2P: GetBlockBodies(hashes=[h1, h2, ...])
    P2P->>Validator: 请求区块体

    Validator-->>P2P: BlockBodies [body1, body2, ...]
    P2P-->>Sync: P2pEvent::BlockBodies

    loop 对每个 body
        Sync->>Sync: 匹配 header + body
        Sync->>Sync: 创建 StoredBlock
        Sync->>Fullnode: 存储区块
        Sync->>Fullnode: 存储交易
    end

    Note over Sync: 检查是否需要继续同步

    alt 还有更多区块
        Sync->>Sync: 继续请求下一批
    else 已同步到最新
        Sync->>Sync: 等待新区块通告
    end
```

## 5. 新区块广播流程 (验证者)

```mermaid
sequenceDiagram
    participant Consensus as POA 共识
    participant Main as 主循环
    participant P2P as P2P Handle
    participant Peers as 所有连接节点

    Consensus->>Main: BlockProposal

    Main->>Main: 执行交易
    Main->>Main: 存储区块

    Main->>P2P: BroadcastBlock(hash, number)

    P2P->>Peers: NewBlockHashes(hash, number)

    Note over Peers: 每个节点收到通告后<br/>请求区块头和区块体

    Peers->>P2P: GetBlockHeaders
    P2P->>Main: P2pEvent::GetBlockHeadersRequest

    Main->>Main: 查询本地区块
    Main->>P2P: SendBlockHeaders(headers)
    P2P->>Peers: BlockHeaders

    Peers->>P2P: GetBlockBodies
    P2P->>Main: P2pEvent::GetBlockBodiesRequest

    Main->>Main: 查询本地交易
    Main->>P2P: SendBlockBodies(bodies)
    P2P->>Peers: BlockBodies
```

## 6. 交易转发流程

```mermaid
sequenceDiagram
    participant User as 用户
    participant Fullnode as 全节点 RPC
    participant Mempool as 交易池
    participant P2P as P2P Handle
    participant Validator as 验证者

    User->>Fullnode: eth_sendRawTransaction
    Fullnode->>Fullnode: 验证交易

    Fullnode->>Mempool: 添加到交易池
    Fullnode-->>User: tx_hash

    Fullnode->>P2P: BroadcastTransactions([tx])
    P2P->>Validator: Transactions 消息

    Validator->>Validator: P2pEvent::Transactions
    Validator->>Validator: 添加到本地交易池

    Note over Validator: 下一个区块生产时

    Validator->>Validator: 获取 pending 交易
    Validator->>Validator: 包含在新区块中
    Validator->>P2P: BroadcastBlock

    P2P->>Fullnode: NewBlockHashes
    Fullnode->>Fullnode: 同步新区块
```

## 7. Enode URL 格式

```
enode://<public_key>@<ip>:<port>

示例:
enode://c6ecdf9e2d5c7838b2787f71e533e0f97ed4d6dde57286884e16683603c4266bb800c67b4bc40cf68c44e0d750b1238306e16af0e16edc80dd24430eaf3d1253@15.235.230.59:30303

组成部分:
├── public_key: 64 字节的 secp256k1 公钥 (十六进制)
├── ip: 节点 IP 地址
└── port: P2P 端口
```

## 8. P2P 密钥管理

```
密钥存储位置: <datadir>/p2p_key

密钥格式: 32 字节的 secp256k1 私钥 (十六进制)

示例:
dc5ef21e7897d317d57cf5336f8be8d75123a8f075196ccec3c8693030fc20c0

节点启动时:
1. 检查 p2p_key 文件是否存在
2. 如果存在: 加载私钥
3. 如果不存在: 生成新私钥并保存
4. 从私钥派生公钥 (PeerId)
5. 构建 enode URL
```

## 9. SessionCommand 命令类型

```rust
enum SessionCommand {
    // 广播新区块哈希给所有节点
    BroadcastBlock { hash: B256, number: u64 },

    // 请求区块头 (从指定节点)
    GetBlockHeaders { peer_id: PeerId, start: u64, count: u64 },

    // 发送区块头响应 (给指定节点)
    SendBlockHeaders { peer_id: PeerId, request_id: u64, headers: Vec<Header> },

    // 请求区块体 (从指定节点)
    GetBlockBodies { peer_id: PeerId, hashes: Vec<B256> },

    // 发送区块体响应 (给指定节点)
    SendBlockBodies { peer_id: PeerId, request_id: u64, bodies: Vec<BlockBody> },

    // 广播交易给所有节点
    BroadcastTransactions { transactions: Vec<Vec<u8>> },
}
```

## 10. P2pEvent 事件类型

```rust
enum P2pEvent {
    // 节点连接
    PeerConnected { peer_id: PeerId, addr: SocketAddr },

    // 节点断开
    PeerDisconnected { peer_id: PeerId },

    // 收到新区块哈希通告
    NewBlockHash { peer_id: PeerId, hash: B256, number: u64 },

    // 收到完整新区块
    NewBlock { peer_id: PeerId, hash: B256, number: u64 },

    // 收到区块头请求
    GetBlockHeadersRequest { peer_id: PeerId, request_id: u64, start: HashOrNumber, limit: u64 },

    // 收到区块头响应
    BlockHeaders { peer_id: PeerId, request_id: u64, headers: Vec<Header> },

    // 收到区块体请求
    GetBlockBodiesRequest { peer_id: PeerId, request_id: u64, hashes: Vec<B256> },

    // 收到区块体响应
    BlockBodies { peer_id: PeerId, request_id: u64, bodies: Vec<BlockBody> },

    // 收到交易广播
    Transactions { peer_id: PeerId, transactions: Vec<Vec<u8>> },
}
```
