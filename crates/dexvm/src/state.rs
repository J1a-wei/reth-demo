use alloy_primitives::{keccak256, Address, B256};
use std::collections::HashMap;

/// DexVM state
///
/// Manages account counter state for the DexVM
#[derive(Debug, Clone, Default)]
pub struct DexVmState {
    /// Account counters: address -> counter value
    counters: HashMap<Address, u64>,
}

impl DexVmState {
    /// Create new empty state
    pub fn new() -> Self {
        Self { counters: HashMap::new() }
    }

    /// Get counter value for address
    pub fn get_counter(&self, address: &Address) -> u64 {
        self.counters.get(address).copied().unwrap_or(0)
    }

    /// Set counter value for address
    pub fn set_counter(&mut self, address: Address, value: u64) {
        if value == 0 {
            self.counters.remove(&address);
        } else {
            self.counters.insert(address, value);
        }
    }

    /// Increment counter and return new value
    pub fn increment_counter(&mut self, address: Address, amount: u64) -> u64 {
        let current = self.get_counter(&address);
        let new_value = current.saturating_add(amount);
        self.set_counter(address, new_value);
        new_value
    }

    /// Decrement counter and return (success, new_value)
    pub fn decrement_counter(&mut self, address: Address, amount: u64) -> Result<u64, String> {
        let current = self.get_counter(&address);
        if amount > current {
            return Err(format!(
                "Counter underflow: have {}, want to decrement {}",
                current, amount
            ));
        }
        let new_value = current - amount;
        self.set_counter(address, new_value);
        Ok(new_value)
    }

    /// Calculate state root
    ///
    /// Simple implementation: keccak256(sorted_account_data)
    pub fn state_root(&self) -> B256 {
        if self.counters.is_empty() {
            return B256::ZERO;
        }

        // Collect and sort accounts
        let mut accounts: Vec<_> = self.counters.iter().collect();
        accounts.sort_by_key(|(addr, _)| *addr);

        // Hash sorted data
        let mut data = Vec::new();
        for (addr, counter) in accounts {
            data.extend_from_slice(addr.as_slice());
            data.extend_from_slice(&counter.to_be_bytes());
        }

        keccak256(&data)
    }

    /// Get all accounts
    pub fn all_accounts(&self) -> &HashMap<Address, u64> {
        &self.counters
    }

    /// Get account count
    pub fn account_count(&self) -> usize {
        self.counters.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::address;

    #[test]
    fn test_counter_operations() {
        let mut state = DexVmState::new();
        let addr = address!("1111111111111111111111111111111111111111");

        // Initial value should be 0
        assert_eq!(state.get_counter(&addr), 0);

        // Increment
        let new_val = state.increment_counter(addr, 10);
        assert_eq!(new_val, 10);
        assert_eq!(state.get_counter(&addr), 10);

        // Decrement
        let new_val = state.decrement_counter(addr, 3).unwrap();
        assert_eq!(new_val, 7);
        assert_eq!(state.get_counter(&addr), 7);

        // Decrement overflow should fail
        let result = state.decrement_counter(addr, 100);
        assert!(result.is_err());
        assert_eq!(state.get_counter(&addr), 7); // Value unchanged
    }

    #[test]
    fn test_state_root() {
        let mut state = DexVmState::new();

        // Empty state should have zero root
        assert_eq!(state.state_root(), B256::ZERO);

        // Add some accounts
        let addr1 = address!("1111111111111111111111111111111111111111");
        let addr2 = address!("2222222222222222222222222222222222222222");

        state.set_counter(addr1, 100);
        state.set_counter(addr2, 200);

        // Should produce non-zero root
        let root = state.state_root();
        assert_ne!(root, B256::ZERO);

        // Same state should produce same root (deterministic)
        let root2 = state.state_root();
        assert_eq!(root, root2);

        // Different state should produce different root
        state.set_counter(addr1, 101);
        let root3 = state.state_root();
        assert_ne!(root, root3);
    }

    #[test]
    fn test_zero_counter_removal() {
        let mut state = DexVmState::new();
        let addr = address!("1111111111111111111111111111111111111111");

        state.set_counter(addr, 10);
        assert_eq!(state.account_count(), 1);

        // Setting to zero should remove the account
        state.set_counter(addr, 0);
        assert_eq!(state.account_count(), 0);
        assert_eq!(state.get_counter(&addr), 0);
    }
}
