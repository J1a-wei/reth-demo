# API 参考文档

## 1. EVM JSON-RPC API (Port 8545)

### eth_chainId
获取链 ID

```bash
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}'
```

响应:
```json
{"jsonrpc":"2.0","id":1,"result":"0x3419"}
```

### eth_blockNumber
获取最新区块号

```bash
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'
```

响应:
```json
{"jsonrpc":"2.0","id":1,"result":"0x64"}
```

### eth_getBalance
查询账户余额

```bash
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc":"2.0",
    "method":"eth_getBalance",
    "params":["0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266", "latest"],
    "id":1
  }'
```

响应:
```json
{"jsonrpc":"2.0","id":1,"result":"0x21e19e0c9bab2400000"}
```

### eth_getTransactionCount
查询账户 nonce

```bash
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc":"2.0",
    "method":"eth_getTransactionCount",
    "params":["0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266", "latest"],
    "id":1
  }'
```

响应:
```json
{"jsonrpc":"2.0","id":1,"result":"0x5"}
```

### eth_sendRawTransaction
发送已签名交易

```bash
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc":"2.0",
    "method":"eth_sendRawTransaction",
    "params":["0xf86c..."],
    "id":1
  }'
```

响应:
```json
{"jsonrpc":"2.0","id":1,"result":"0x...txhash..."}
```

### eth_getBlockByNumber
按区块号查询区块

```bash
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc":"2.0",
    "method":"eth_getBlockByNumber",
    "params":["0x64", false],
    "id":1
  }'
```

响应:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "number": "0x64",
    "hash": "0x...",
    "parentHash": "0x...",
    "sha3Uncles": "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
    "logsBloom": "0x00...00",
    "transactionsRoot": "0x...",
    "stateRoot": "0x...",
    "receiptsRoot": "0x...",
    "miner": "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266",
    "difficulty": "0x1",
    "totalDifficulty": "0x65",
    "extraData": "0x",
    "size": "0x3e8",
    "gasLimit": "0x1c9c380",
    "gasUsed": "0x5208",
    "timestamp": "0x...",
    "transactions": ["0x...txhash..."],
    "uncles": [],
    "nonce": "0x0000000000000000",
    "baseFeePerGas": "0x3b9aca00"
  }
}
```

### eth_getBlockByHash
按区块哈希查询区块

```bash
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc":"2.0",
    "method":"eth_getBlockByHash",
    "params":["0x...", false],
    "id":1
  }'
```

### eth_getTransactionReceipt
查询交易收据

```bash
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc":"2.0",
    "method":"eth_getTransactionReceipt",
    "params":["0x...txhash..."],
    "id":1
  }'
```

响应:
```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "result": {
    "transactionHash": "0x...",
    "transactionIndex": "0x0",
    "blockHash": "0x...",
    "blockNumber": "0x64",
    "from": "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266",
    "to": "0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
    "cumulativeGasUsed": "0x5208",
    "gasUsed": "0x5208",
    "contractAddress": null,
    "logs": [],
    "logsBloom": "0x00...00",
    "status": "0x1",
    "type": "0x0"
  }
}
```

### eth_gasPrice
获取 gas 价格

```bash
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_gasPrice","params":[],"id":1}'
```

响应:
```json
{"jsonrpc":"2.0","id":1,"result":"0x3b9aca00"}
```

### eth_estimateGas
估算 gas 消耗

```bash
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc":"2.0",
    "method":"eth_estimateGas",
    "params":[{
      "from": "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266",
      "to": "0x70997970C51812dc3A010C7d01b50e0d17dc79C8",
      "value": "0xde0b6b3a7640000"
    }],
    "id":1
  }'
```

响应:
```json
{"jsonrpc":"2.0","id":1,"result":"0x5208"}
```

### web3_clientVersion
获取客户端版本

```bash
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"web3_clientVersion","params":[],"id":1}'
```

响应:
```json
{"jsonrpc":"2.0","id":1,"result":"DualVM/v0.1.0"}
```

### net_version
获取网络 ID

```bash
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"net_version","params":[],"id":1}'
```

响应:
```json
{"jsonrpc":"2.0","id":1,"result":"13337"}
```

---

## 2. DexVM REST API (Port 9845)

### GET /health
健康检查

```bash
curl http://localhost:9845/health
```

响应:
```json
{
  "status": "ok",
  "service": "dexvm-api",
  "version": "0.1.0"
}
```

### GET /api/v1/counter/{address}
查询计数器值

```bash
curl http://localhost:9845/api/v1/counter/0x1234567890123456789012345678901234567890
```

响应:
```json
{
  "address": "0x1234567890123456789012345678901234567890",
  "counter": 150
}
```

### POST /api/v1/counter/{address}/increment
增加计数器

```bash
curl -X POST http://localhost:9845/api/v1/counter/0x1234567890123456789012345678901234567890/increment \
  -H "Content-Type: application/json" \
  -d '{"amount": 100}'
```

响应:
```json
{
  "success": true,
  "tx_hash": "0x2c3a17acf01055d0f4a0c137f563b6ca174d887e974b206e8783dfd8c69fe919",
  "old_counter": 0,
  "new_counter": 100,
  "gas_used": 26000,
  "error": null
}
```

### POST /api/v1/counter/{address}/decrement
减少计数器

```bash
curl -X POST http://localhost:9845/api/v1/counter/0x1234567890123456789012345678901234567890/decrement \
  -H "Content-Type: application/json" \
  -d '{"amount": 50}'
```

成功响应:
```json
{
  "success": true,
  "tx_hash": "0xe6ee52fed119729be040246d4089707540ba5d55cc9d2865d131b72f4e1b1ad6",
  "old_counter": 100,
  "new_counter": 50,
  "gas_used": 26000,
  "error": null
}
```

失败响应 (underflow):
```json
{
  "success": false,
  "tx_hash": "0x...",
  "old_counter": 50,
  "new_counter": 50,
  "gas_used": 26000,
  "error": "Counter underflow: cannot decrement below zero"
}
```

### GET /api/v1/state-root
获取状态根

```bash
curl http://localhost:9845/api/v1/state-root
```

响应:
```json
{
  "evm_root": "0x4833df272715970eaf6d1120503b7f57b1eaa635dfaaf2c5e10519a1b7401779",
  "dexvm_root": "0x42b5a13a3990ea32d163c6ad54bd325908d732dfde215b240510b273965d7075",
  "combined": "0x..."
}
```

---

## 3. 使用 cast 发送交易

### 发送 ETH 转账

```bash
cast send \
  --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 \
  --rpc-url http://localhost:8545 \
  --legacy \
  0x70997970C51812dc3A010C7d01b50e0d17dc79C8 \
  --value 1ether
```

### 发送 DexVM 交易 (增加计数器)

```bash
# calldata: 0x00 (increment) + 0000000000000064 (100 in hex)
cast send \
  --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 \
  --rpc-url http://localhost:8545 \
  --legacy \
  0xddddddddddddddddddddddddddddddddddddddd1 \
  0x000000000000000064
```

### 调用预编译 (增加计数器)

```bash
# calldata: 0x00 (increment) + 0000000000000064 (100 in hex)
cast send \
  --private-key 0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80 \
  --rpc-url http://localhost:8545 \
  --legacy \
  0x0000000000000000000000000000000000000100 \
  0x000000000000000064
```

---

## 4. 错误码

### JSON-RPC 错误

| 错误码 | 描述 |
|--------|------|
| -32000 | Nonce too low |
| -32000 | Insufficient balance |
| -32000 | Failed to decode transaction |
| -32000 | Failed to recover signer |

### HTTP 状态码

| 状态码 | 描述 |
|--------|------|
| 200 | 成功 |
| 400 | 请求格式错误 |
| 404 | 资源不存在 |
| 500 | 服务器内部错误 |
