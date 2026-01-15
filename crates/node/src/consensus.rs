//! POA consensus engine

use alloy_primitives::{Address, B256};
use reth_ethereum_primitives::TransactionSigned;
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{sync::mpsc, time::sleep};

/// POA consensus configuration
#[derive(Debug, Clone)]
pub struct PoaConfig {
    /// Validator address
    pub validator: Address,
    /// Block interval
    pub block_interval: Duration,
    /// Starting block number
    pub starting_block: u64,
}

impl Default for PoaConfig {
    fn default() -> Self {
        Self {
            validator: Address::ZERO,
            block_interval: Duration::from_millis(500),
            starting_block: 0,
        }
    }
}

/// Block proposal
#[derive(Debug, Clone)]
pub struct BlockProposal {
    /// Block number
    pub number: u64,
    /// Parent block hash
    pub parent_hash: B256,
    /// Timestamp
    pub timestamp: u64,
    /// Transactions
    pub transactions: Vec<TransactionSigned>,
    /// Proposer (validator) address
    pub proposer: Address,
}

/// POA consensus engine
pub struct PoaConsensus {
    config: PoaConfig,
    current_block: Arc<Mutex<u64>>,
    last_block_hash: Arc<Mutex<B256>>,
    proposal_tx: mpsc::UnboundedSender<BlockProposal>,
    proposal_rx: Arc<Mutex<mpsc::UnboundedReceiver<BlockProposal>>>,
}

impl PoaConsensus {
    /// Create new POA consensus engine
    pub fn new(config: PoaConfig) -> Self {
        let (proposal_tx, proposal_rx) = mpsc::unbounded_channel();

        Self {
            current_block: Arc::new(Mutex::new(config.starting_block)),
            last_block_hash: Arc::new(Mutex::new(B256::ZERO)),
            config,
            proposal_tx,
            proposal_rx: Arc::new(Mutex::new(proposal_rx)),
        }
    }

    /// Start the consensus engine
    pub fn start(&self) -> tokio::task::JoinHandle<()> {
        let config = self.config.clone();
        let current_block = Arc::clone(&self.current_block);
        let last_block_hash = Arc::clone(&self.last_block_hash);
        let proposal_tx = self.proposal_tx.clone();

        tokio::spawn(async move {
            tracing::info!(
                "POA consensus started, validator: {:?}, block_interval: {:?}",
                config.validator,
                config.block_interval
            );

            let mut last_block_time = Instant::now();

            loop {
                let elapsed = last_block_time.elapsed();
                if elapsed < config.block_interval {
                    sleep(config.block_interval - elapsed).await;
                }

                last_block_time = Instant::now();

                let block_number = {
                    let mut block = current_block.lock().unwrap();
                    *block += 1;
                    *block
                };

                let parent_hash = *last_block_hash.lock().unwrap();

                let proposal = BlockProposal {
                    number: block_number,
                    parent_hash,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    transactions: Vec::new(),
                    proposer: config.validator,
                };

                tracing::debug!(
                    "Generated block proposal: number={}, parent_hash={:?}",
                    proposal.number,
                    proposal.parent_hash
                );

                if proposal_tx.send(proposal).is_err() {
                    tracing::error!("Cannot send block proposal, receiver closed");
                    break;
                }
            }
        })
    }

    /// Receive block proposal
    pub fn recv_proposal(&self) -> Option<BlockProposal> {
        self.proposal_rx.lock().unwrap().try_recv().ok()
    }

    /// Finalize block
    pub fn finalize_block(&self, block_hash: B256) {
        *self.last_block_hash.lock().unwrap() = block_hash;
        tracing::debug!("Block finalized, hash={:?}", block_hash);
    }

    /// Get current block number
    pub fn current_block_number(&self) -> u64 {
        *self.current_block.lock().unwrap()
    }

    /// Get config
    pub fn config(&self) -> &PoaConfig {
        &self.config
    }

    /// Set last block hash (for recovery from storage)
    pub fn set_last_block_hash(&mut self, hash: B256) {
        *self.last_block_hash.lock().unwrap() = hash;
    }

    /// Submit transaction
    pub fn submit_transaction(&self, tx: TransactionSigned) -> Result<(), String> {
        let block_number = {
            let mut block = self.current_block.lock().unwrap();
            *block += 1;
            *block
        };

        let parent_hash = *self.last_block_hash.lock().unwrap();

        let proposal = BlockProposal {
            number: block_number,
            parent_hash,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            transactions: vec![tx],
            proposer: self.config.validator,
        };

        self.proposal_tx.send(proposal).map_err(|e| format!("Failed to submit transaction: {}", e))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::address;

    #[test]
    fn test_poa_config_default() {
        let config = PoaConfig::default();
        assert_eq!(config.block_interval, Duration::from_millis(500));
        assert_eq!(config.starting_block, 0);
    }

    #[test]
    fn test_poa_consensus_creation() {
        let config = PoaConfig {
            validator: address!("1111111111111111111111111111111111111111"),
            block_interval: Duration::from_secs(1),
            starting_block: 0,
        };

        let consensus = PoaConsensus::new(config.clone());
        assert_eq!(consensus.current_block_number(), 0);
        assert_eq!(consensus.config().validator, config.validator);
    }

    #[tokio::test]
    async fn test_poa_block_production() {
        let config = PoaConfig {
            validator: address!("1111111111111111111111111111111111111111"),
            block_interval: Duration::from_millis(100),
            starting_block: 0,
        };

        let consensus = PoaConsensus::new(config);
        let handle = consensus.start();

        tokio::time::sleep(Duration::from_millis(350)).await;

        let mut proposals = Vec::new();
        while let Some(proposal) = consensus.recv_proposal() {
            proposals.push(proposal);
        }

        assert!(proposals.len() >= 3, "Expected at least 3 blocks, got: {}", proposals.len());

        for (i, proposal) in proposals.iter().enumerate() {
            assert_eq!(proposal.number, (i + 1) as u64);
        }

        handle.abort();
    }

    #[test]
    fn test_finalize_block() {
        let consensus = PoaConsensus::new(PoaConfig::default());

        let block_hash = B256::from([1u8; 32]);
        consensus.finalize_block(block_hash);

        assert_eq!(*consensus.last_block_hash.lock().unwrap(), block_hash);
    }
}
