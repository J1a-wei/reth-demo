# 讨论：三个测试流程

## 确定的需求

1. 用户可以通过预编译合约，通过 EVM 调用计数器，修改 DexVM 状态
2. 需要验证三个流程的可行性

## 流程 1：Foundry 部署 ETH 存钱罐（验证 EVM）

```solidity
// PiggyBank.sol
contract PiggyBank {
    mapping(address => uint256) public balances;

    function deposit() external payable {
        balances[msg.sender] += msg.value;
    }

    function withdraw(uint256 amount) external {
        require(balances[msg.sender] >= amount);
        balances[msg.sender] -= amount;
        payable(msg.sender).transfer(amount);
    }
}
```

测试命令：
```bash
# 部署
forge create PiggyBank --rpc-url http://127.0.0.1:8545 --private-key <key>

# 存款
cast send <contract> "deposit()" --value 1ether --rpc-url http://127.0.0.1:8545 --private-key <key>

# 查询余额
cast call <contract> "balances(address)" <address> --rpc-url http://127.0.0.1:8545

# 取款
cast send <contract> "withdraw(uint256)" 0.5ether --rpc-url http://127.0.0.1:8545 --private-key <key>
```

## 流程 2：EVM 预编译合约调用 DexVM

预编译合约地址: `0x0000000000000000000000000000000000000100`

Calldata 格式:
- `[op: 1 byte][amount: 8 bytes big-endian]`
- op = 0x00 → Increment
- op = 0x01 → Decrement
- op = 0x02 → Query

测试命令：
```bash
# 查询计数器 (op=0x02)
cast call 0x0000000000000000000000000000000000000100 0x020000000000000000 --rpc-url http://127.0.0.1:8545

# 增加计数器 (op=0x00, amount=10)
cast send 0x0000000000000000000000000000000000000100 0x00000000000000000a --rpc-url http://127.0.0.1:8545 --private-key <key>

# 减少计数器 (op=0x01, amount=5)
cast send 0x0000000000000000000000000000000000000100 0x010000000000000005 --rpc-url http://127.0.0.1:8545 --private-key <key>
```

## 流程 3：DexVM RPC 直接操作计数器

DexVM REST API: `http://127.0.0.1:9845`

测试命令：
```bash
# 健康检查
curl http://127.0.0.1:9845/health

# 查询计数器
curl http://127.0.0.1:9845/api/v1/counter/<address>

# 增加计数器
curl -X POST http://127.0.0.1:9845/api/v1/counter/<address>/increment \
  -H "Content-Type: application/json" \
  -d '{"amount": 10}'

# 减少计数器
curl -X POST http://127.0.0.1:9845/api/v1/counter/<address>/decrement \
  -H "Content-Type: application/json" \
  -d '{"amount": 5}'

# 查询 state root
curl http://127.0.0.1:9845/api/v1/state-root
```

## 验证点

| 流程 | 验证点 |
|------|--------|
| 1 | EVM 合约部署、调用、状态存储正常 |
| 2 | EVM → 预编译 → DexVM 跨 VM 调用正常，原子性回滚正常 |
| 3 | DexVM 独立 RPC 接口正常 |

## 关键设计决定

| 问题 | 决定 |
|------|------|
| 原子性 | 需要回滚，EVM 和 DexVM 作为一个原子操作 |
| Gas | 固定值 |
| Receipt | 先忽略 |

## 实现状态

### 已完成
- ✅ 预编译合约支持 Counter 操作 (Increment/Decrement/Query)
- ✅ EVM → DexVM 跨 VM 调用
- ✅ 原子性执行：如果 DexVM 操作失败，EVM 状态回滚
- ✅ 单元测试覆盖

### 测试状态

#### 流程 1: Foundry 部署 ETH 存钱罐 ✅ 通过
```bash
# 部署合约
forge create PiggyBank.sol:PiggyBank --rpc-url http://127.0.0.1:8545 --private-key <key> --legacy --broadcast
# 合约地址: 0xd85be814469baa0b0bad920d0e484690e0d2c767
# 交易成功执行, gas_used=337440

# 存款测试
cast send <contract> "deposit()" --value 1ether --rpc-url http://127.0.0.1:8545 --private-key <key> --legacy
# 交易成功执行, gas_used=21064
# 余额正确减少 (从 1000 ETH -> ~999 ETH)
```

#### 流程 2: EVM 预编译合约调用 DexVM ✅ 单元测试通过
- 69 个单元测试全部通过
- 测试覆盖: Counter 增加/减少/查询、跨 VM 调用、原子回滚
- 端到端测试需要更新 RPC 层以使用 DualVmExecutor

#### 流程 3: DexVM RPC 直接操作计数器 ✅ 通过
```bash
# 所有操作正常工作
curl http://127.0.0.1:9845/health  # {"status":"ok"}
curl http://127.0.0.1:9845/api/v1/counter/<address>  # 查询计数器
curl -X POST .../increment -d '{"amount": 10}'  # 增加计数器
curl -X POST .../decrement -d '{"amount": 5}'   # 减少计数器
curl http://127.0.0.1:9845/api/v1/state-root    # 获取状态根
```

### 已知问题
- RPC receipt 缺少 `logsBloom` 字段 (Foundry 工具报警告但不影响执行)
- eth_call 对预编译合约返回空 (需要后续实现)

## 实现细节

### 预编译合约
- 地址: `0x0000000000000000000000000000000000000100`
- Calldata 格式: `[op: 1 byte][amount: 8 bytes big-endian]`
  - `0x00` = Increment
  - `0x01` = Decrement
  - `0x02` = Query

### 关键文件
- `crates/dexvm/src/precompiles.rs` - 预编译合约实现
- `crates/node/src/evm_executor.rs` - EVM 执行器，处理预编译调用
- `crates/node/src/executor.rs` - 双 VM 执行器，协调跨 VM 调用
