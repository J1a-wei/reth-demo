//! Execution inspector for tracing EVM execution
//!
//! Collects logs, state changes, and execution traces during EVM execution.

use alloy_primitives::{Address, Bytes, Log, B256, U256};
use revm::{
    context::ContextTr,
    interpreter::{CallInputs, CallOutcome, CreateInputs, CreateOutcome, Interpreter, InterpreterTypes},
    Inspector,
};
use std::collections::HashMap;

/// Execution trace entry
#[derive(Debug, Clone)]
pub enum TraceEntry {
    /// Call to another contract
    Call {
        from: Address,
        to: Address,
        value: U256,
        input: Bytes,
        gas: u64,
    },
    /// Contract creation
    Create {
        from: Address,
        value: U256,
        init_code: Bytes,
        gas: u64,
    },
    /// Call result
    CallResult {
        success: bool,
        output: Bytes,
        gas_used: u64,
    },
    /// Create result
    CreateResult {
        success: bool,
        address: Option<Address>,
        gas_used: u64,
    },
    /// Log emitted
    Log {
        address: Address,
        topics: Vec<B256>,
        data: Bytes,
    },
    /// Storage read
    StorageRead {
        address: Address,
        slot: U256,
        value: U256,
    },
    /// Storage write
    StorageWrite {
        address: Address,
        slot: U256,
        old_value: U256,
        new_value: U256,
    },
}

/// Execution inspector that collects traces and logs
#[derive(Debug, Default)]
pub struct ExecutionInspector {
    /// Collected trace entries
    traces: Vec<TraceEntry>,
    /// Collected logs
    logs: Vec<Log>,
    /// Storage changes per address
    storage_changes: HashMap<Address, HashMap<U256, (U256, U256)>>,
    /// Call depth
    call_depth: usize,
    /// Gas tracking per depth
    gas_at_depth: Vec<u64>,
}

impl ExecutionInspector {
    /// Create new inspector
    pub fn new() -> Self {
        Self::default()
    }

    /// Get collected traces
    pub fn traces(&self) -> &[TraceEntry] {
        &self.traces
    }

    /// Get collected logs
    pub fn logs(&self) -> &[Log] {
        &self.logs
    }

    /// Get storage changes
    pub fn storage_changes(&self) -> &HashMap<Address, HashMap<U256, (U256, U256)>> {
        &self.storage_changes
    }

    /// Clear all collected data
    pub fn clear(&mut self) {
        self.traces.clear();
        self.logs.clear();
        self.storage_changes.clear();
        self.call_depth = 0;
        self.gas_at_depth.clear();
    }

    /// Get total gas used across all traces
    pub fn total_gas_used(&self) -> u64 {
        self.traces
            .iter()
            .filter_map(|t| match t {
                TraceEntry::CallResult { gas_used, .. } => Some(*gas_used),
                TraceEntry::CreateResult { gas_used, .. } => Some(*gas_used),
                _ => None,
            })
            .sum()
    }
}

impl<CTX: ContextTr, INTR: InterpreterTypes> Inspector<CTX, INTR> for ExecutionInspector {
    fn call(&mut self, _ctx: &mut CTX, inputs: &mut CallInputs) -> Option<CallOutcome> {
        self.traces.push(TraceEntry::Call {
            from: inputs.caller,
            to: inputs.target_address,
            value: inputs.value.get(),
            input: Bytes::new(), // Input is in CallInput enum, simplified for now
            gas: inputs.gas_limit,
        });

        self.call_depth += 1;
        self.gas_at_depth.push(inputs.gas_limit);

        None // Continue execution
    }

    fn call_end(&mut self, _ctx: &mut CTX, _inputs: &CallInputs, outcome: &mut CallOutcome) {
        let gas_used = self
            .gas_at_depth
            .pop()
            .unwrap_or(0)
            .saturating_sub(outcome.gas().remaining());

        self.traces.push(TraceEntry::CallResult {
            success: outcome.result.is_ok(),
            output: outcome.result.output.clone(),
            gas_used,
        });

        self.call_depth = self.call_depth.saturating_sub(1);
    }

    fn create(&mut self, _ctx: &mut CTX, inputs: &mut CreateInputs) -> Option<CreateOutcome> {
        self.traces.push(TraceEntry::Create {
            from: inputs.caller,
            value: inputs.value,
            init_code: inputs.init_code.clone(),
            gas: inputs.gas_limit,
        });

        self.call_depth += 1;
        self.gas_at_depth.push(inputs.gas_limit);

        None // Continue execution
    }

    fn create_end(
        &mut self,
        _ctx: &mut CTX,
        _inputs: &CreateInputs,
        outcome: &mut CreateOutcome,
    ) {
        let gas_used = self
            .gas_at_depth
            .pop()
            .unwrap_or(0)
            .saturating_sub(outcome.gas().remaining());

        self.traces.push(TraceEntry::CreateResult {
            success: outcome.result.is_ok(),
            address: outcome.address,
            gas_used,
        });

        self.call_depth = self.call_depth.saturating_sub(1);
    }

    fn log(&mut self, _interp: &mut Interpreter<INTR>, _ctx: &mut CTX, log: Log) {
        self.traces.push(TraceEntry::Log {
            address: log.address,
            topics: log.data.topics().to_vec(),
            data: log.data.data.clone(),
        });

        self.logs.push(log);
    }
}

/// Simple inspector that only collects logs
#[derive(Debug, Default)]
pub struct LogCollector {
    logs: Vec<Log>,
}

impl LogCollector {
    /// Create new log collector
    pub fn new() -> Self {
        Self::default()
    }

    /// Get collected logs
    pub fn logs(&self) -> &[Log] {
        &self.logs
    }

    /// Take collected logs
    pub fn take_logs(&mut self) -> Vec<Log> {
        std::mem::take(&mut self.logs)
    }
}

impl<CTX: ContextTr, INTR: InterpreterTypes> Inspector<CTX, INTR> for LogCollector {
    fn log(&mut self, _interp: &mut Interpreter<INTR>, _ctx: &mut CTX, log: Log) {
        self.logs.push(log);
    }
}

/// Gas tracker inspector
#[derive(Debug, Default)]
pub struct GasTracker {
    /// Total gas used
    total_gas_used: u64,
    /// Gas used per call depth
    gas_per_depth: Vec<u64>,
    /// Current depth
    current_depth: usize,
}

impl GasTracker {
    /// Create new gas tracker
    pub fn new() -> Self {
        Self::default()
    }

    /// Get total gas used
    pub fn total_gas_used(&self) -> u64 {
        self.total_gas_used
    }

    /// Get gas used per depth
    pub fn gas_per_depth(&self) -> &[u64] {
        &self.gas_per_depth
    }
}

impl<CTX: ContextTr, INTR: InterpreterTypes> Inspector<CTX, INTR> for GasTracker {
    fn call(&mut self, _ctx: &mut CTX, _inputs: &mut CallInputs) -> Option<CallOutcome> {
        if self.current_depth >= self.gas_per_depth.len() {
            self.gas_per_depth.push(0);
        }
        self.current_depth += 1;
        None
    }

    fn call_end(&mut self, _ctx: &mut CTX, inputs: &CallInputs, outcome: &mut CallOutcome) {
        let gas_used = inputs.gas_limit.saturating_sub(outcome.gas().remaining());

        if self.current_depth > 0 && self.current_depth <= self.gas_per_depth.len() {
            self.gas_per_depth[self.current_depth - 1] += gas_used;
        }

        self.total_gas_used += gas_used;
        self.current_depth = self.current_depth.saturating_sub(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inspector_creation() {
        let inspector = ExecutionInspector::new();
        assert!(inspector.traces().is_empty());
        assert!(inspector.logs().is_empty());
    }

    #[test]
    fn test_log_collector() {
        let collector = LogCollector::new();
        assert!(collector.logs().is_empty());
    }

    #[test]
    fn test_gas_tracker() {
        let tracker = GasTracker::new();
        assert_eq!(tracker.total_gas_used(), 0);
    }
}
