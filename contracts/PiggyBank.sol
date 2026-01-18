// SPDX-License-Identifier: MIT
pragma solidity ^0.8.0;

/// @title PiggyBank - 简单的 ETH 存钱罐合约
/// @notice 用于测试 EVM 合约部署和交互
contract PiggyBank {
    mapping(address => uint256) public balances;

    event Deposit(address indexed user, uint256 amount);
    event Withdraw(address indexed user, uint256 amount);

    /// @notice 存款 ETH
    function deposit() external payable {
        balances[msg.sender] += msg.value;
        emit Deposit(msg.sender, msg.value);
    }

    /// @notice 取款 ETH
    /// @param amount 取款金额
    function withdraw(uint256 amount) external {
        require(balances[msg.sender] >= amount, "Insufficient balance");
        balances[msg.sender] -= amount;
        payable(msg.sender).transfer(amount);
        emit Withdraw(msg.sender, amount);
    }

    /// @notice 查询用户余额
    /// @param user 用户地址
    /// @return 用户在合约中的余额
    function getBalance(address user) external view returns (uint256) {
        return balances[user];
    }
}
