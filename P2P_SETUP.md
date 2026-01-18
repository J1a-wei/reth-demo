# P2P 网络配置指南

## 验证者节点信息

| 配置项 | 值 |
|--------|-----|
| IP 地址 | 15.235.230.59 |
| P2P 端口 | 30303 |
| EVM RPC 端口 | 8545 |
| DexVM 端口 | 9845 |
| Chain ID | 13337 |

### Enode URL
```
enode://c6ecdf9e2d5c7838b2787f71e533e0f97ed4d6dde57286884e16683603c4266bb800c67b4bc40cf68c44e0d750b1238306e16af0e16edc80dd24430eaf3d1253@15.235.230.59:30303
```

## 启动验证者节点

```bash
./scripts/start_validator.sh
```

验证者节点的 P2P 密钥保存在 `validator_p2p.key` 文件中，确保 enode URL 固定不变。

## 启动全节点

### 方式 1: 使用默认配置连接验证者

```bash
./scripts/start_fullnode.sh
```

### 方式 2: 自定义配置

```bash
# 使用不同的端口
EVM_RPC_PORT=8547 DEXVM_PORT=9847 P2P_PORT=30305 ./scripts/start_fullnode.sh

# 连接到其他验证者 IP
VALIDATOR_IP=192.168.1.100 ./scripts/start_fullnode.sh

# 使用自定义 bootnode
BOOTNODE="enode://...@ip:port" ./scripts/start_fullnode.sh
```

## 验证连接

### 检查区块同步
```bash
# 验证者节点
curl -s http://15.235.230.59:8545 -X POST -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'

# 全节点
curl -s http://localhost:8546 -X POST -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'
```

### 发送交易测试
```bash
# 发送交易到全节点 (会转发到验证者)
cast send --private-key <your_key> \
  --rpc-url http://localhost:8546 \
  --legacy \
  <to_address> \
  --value 0.1ether
```

## 文件说明

| 文件 | 说明 |
|------|------|
| `validator_p2p.key` | 验证者 P2P 密钥 (固定，勿删除) |
| `genesis.json` | 创世区块配置 |
| `data/` | 验证者数据目录 |
| `data-fullnode/` | 全节点数据目录 |

## 注意事项

1. **P2P 密钥**: `validator_p2p.key` 包含验证者的固定 P2P 密钥，删除后需要更新所有全节点的 bootnode 配置
2. **防火墙**: 确保 P2P 端口 (30303) 对外开放
3. **创世文件**: 所有节点必须使用相同的 `genesis.json`
