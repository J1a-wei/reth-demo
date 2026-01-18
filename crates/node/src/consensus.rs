//! POA consensus engine with block signing

use alloy_primitives::{keccak256, Address, B256};
use reth_ethereum_primitives::TransactionSigned;
use secp256k1::{Message, PublicKey, Secp256k1, SecretKey};
use std::{
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};
use tokio::{sync::mpsc, time::sleep};

/// POA consensus configuration
#[derive(Debug, Clone)]
pub struct PoaConfig {
    /// Validator secret key (for signing blocks)
    pub secret_key: SecretKey,
    /// Validator address (derived from secret key)
    pub validator: Address,
    /// Block interval
    pub block_interval: Duration,
    /// Starting block number
    pub starting_block: u64,
}

impl PoaConfig {
    /// Create new POA config from secret key
    pub fn new(secret_key: SecretKey, block_interval: Duration) -> Self {
        let validator = secret_key_to_address(&secret_key);
        Self {
            secret_key,
            validator,
            block_interval,
            starting_block: 0,
        }
    }

    /// Create from hex private key string
    pub fn from_hex_key(hex_key: &str, block_interval: Duration) -> Result<Self, String> {
        let hex_key = hex_key.strip_prefix("0x").unwrap_or(hex_key);
        let key_bytes = hex::decode(hex_key).map_err(|e| format!("Invalid hex: {}", e))?;
        let secret_key =
            SecretKey::from_slice(&key_bytes).map_err(|e| format!("Invalid key: {}", e))?;
        Ok(Self::new(secret_key, block_interval))
    }
}

/// Derive address from secret key
pub fn secret_key_to_address(secret_key: &SecretKey) -> Address {
    let secp = Secp256k1::new();
    let public_key = PublicKey::from_secret_key(&secp, secret_key);
    let public_key_bytes = public_key.serialize_uncompressed();
    // Skip the first byte (0x04 prefix) and hash the rest
    let hash = keccak256(&public_key_bytes[1..]);
    Address::from_slice(&hash[12..])
}

/// Block signature (65 bytes: r[32] + s[32] + v[1])
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct BlockSignature {
    pub r: B256,
    pub s: B256,
    pub v: u8,
}

impl BlockSignature {
    /// Create from bytes
    pub fn from_bytes(bytes: &[u8]) -> Option<Self> {
        if bytes.len() != 65 {
            return None;
        }
        Some(Self {
            r: B256::from_slice(&bytes[0..32]),
            s: B256::from_slice(&bytes[32..64]),
            v: bytes[64],
        })
    }

    /// Convert to bytes
    pub fn to_bytes(&self) -> [u8; 65] {
        let mut bytes = [0u8; 65];
        bytes[0..32].copy_from_slice(self.r.as_slice());
        bytes[32..64].copy_from_slice(self.s.as_slice());
        bytes[64] = self.v;
        bytes
    }

    /// Check if signature is empty/default
    pub fn is_empty(&self) -> bool {
        self.r == B256::ZERO && self.s == B256::ZERO && self.v == 0
    }
}

/// Block proposal with signature
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
    /// Block signature
    pub signature: BlockSignature,
}

impl BlockProposal {
    /// Compute the signing hash (hash of block header data)
    pub fn signing_hash(&self) -> B256 {
        let mut data = Vec::new();
        data.extend_from_slice(&self.number.to_be_bytes());
        data.extend_from_slice(self.parent_hash.as_slice());
        data.extend_from_slice(&self.timestamp.to_be_bytes());
        data.extend_from_slice(self.proposer.as_slice());
        keccak256(&data)
    }

    /// Sign the block with the given secret key
    pub fn sign(&mut self, secret_key: &SecretKey) {
        let hash = self.signing_hash();
        let secp = Secp256k1::new();
        let message = Message::from_digest(hash.0);
        let (recovery_id, signature) = secp
            .sign_ecdsa_recoverable(&message, secret_key)
            .serialize_compact();

        self.signature = BlockSignature {
            r: B256::from_slice(&signature[0..32]),
            s: B256::from_slice(&signature[32..64]),
            v: i32::from(recovery_id) as u8,
        };
    }

    /// Verify the block signature and return the signer address
    pub fn recover_signer(&self) -> Option<Address> {
        if self.signature.is_empty() {
            return None;
        }

        let hash = self.signing_hash();
        let secp = Secp256k1::new();
        let message = Message::from_digest(hash.0);

        // Reconstruct the signature
        let mut sig_bytes = [0u8; 64];
        sig_bytes[0..32].copy_from_slice(self.signature.r.as_slice());
        sig_bytes[32..64].copy_from_slice(self.signature.s.as_slice());

        let recovery_id =
            secp256k1::ecdsa::RecoveryId::try_from(self.signature.v as i32).ok()?;
        let recoverable_sig =
            secp256k1::ecdsa::RecoverableSignature::from_compact(&sig_bytes, recovery_id).ok()?;

        let public_key = secp.recover_ecdsa(&message, &recoverable_sig).ok()?;
        let public_key_bytes = public_key.serialize_uncompressed();
        let hash = keccak256(&public_key_bytes[1..]);
        Some(Address::from_slice(&hash[12..]))
    }

    /// Verify the block was signed by the expected proposer
    pub fn verify_signature(&self) -> bool {
        match self.recover_signer() {
            Some(signer) => signer == self.proposer,
            None => false,
        }
    }
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

                let mut proposal = BlockProposal {
                    number: block_number,
                    parent_hash,
                    timestamp: std::time::SystemTime::now()
                        .duration_since(std::time::UNIX_EPOCH)
                        .unwrap()
                        .as_secs(),
                    transactions: Vec::new(),
                    proposer: config.validator,
                    signature: BlockSignature::default(),
                };

                // Sign the block
                proposal.sign(&config.secret_key);

                tracing::debug!(
                    "Generated signed block proposal: number={}, proposer={:?}",
                    proposal.number,
                    proposal.proposer
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

        let mut proposal = BlockProposal {
            number: block_number,
            parent_hash,
            timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs(),
            transactions: vec![tx],
            proposer: self.config.validator,
            signature: BlockSignature::default(),
        };

        // Sign the block
        proposal.sign(&self.config.secret_key);

        self.proposal_tx
            .send(proposal)
            .map_err(|e| format!("Failed to submit transaction: {}", e))
    }

    /// Verify a block was signed by the expected validator
    pub fn verify_block(&self, proposal: &BlockProposal) -> bool {
        // Check if proposer matches our expected validator
        if proposal.proposer != self.config.validator {
            tracing::warn!(
                "Block proposer mismatch: expected {:?}, got {:?}",
                self.config.validator,
                proposal.proposer
            );
            return false;
        }

        // Verify the signature
        if !proposal.verify_signature() {
            tracing::warn!("Block signature verification failed");
            return false;
        }

        true
    }
}

/// Verify a block signature against a list of allowed validators
pub fn verify_block_signature(proposal: &BlockProposal, validators: &[Address]) -> bool {
    // Recover the signer
    let signer = match proposal.recover_signer() {
        Some(s) => s,
        None => {
            tracing::warn!("Failed to recover block signer");
            return false;
        }
    };

    // Check if signer is in the validators list
    if !validators.contains(&signer) {
        tracing::warn!(
            "Block signer {:?} is not in validators list",
            signer
        );
        return false;
    }

    // Check if signer matches proposer
    if signer != proposal.proposer {
        tracing::warn!(
            "Block signer {:?} does not match proposer {:?}",
            signer,
            proposal.proposer
        );
        return false;
    }

    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use alloy_primitives::address;

    // Test validator private key (DO NOT USE IN PRODUCTION)
    // Address: 0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266
    const TEST_PRIVATE_KEY: &str =
        "ac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80";

    fn test_secret_key() -> SecretKey {
        let key_bytes = hex::decode(TEST_PRIVATE_KEY).unwrap();
        SecretKey::from_slice(&key_bytes).unwrap()
    }

    #[test]
    fn test_secret_key_to_address() {
        let secret_key = test_secret_key();
        let address = secret_key_to_address(&secret_key);
        assert_eq!(
            address,
            address!("f39Fd6e51aad88F6F4ce6aB8827279cffFb92266")
        );
    }

    #[test]
    fn test_poa_config_from_hex_key() {
        let config =
            PoaConfig::from_hex_key(TEST_PRIVATE_KEY, Duration::from_millis(500)).unwrap();
        assert_eq!(
            config.validator,
            address!("f39Fd6e51aad88F6F4ce6aB8827279cffFb92266")
        );
    }

    #[test]
    fn test_block_signing() {
        let secret_key = test_secret_key();
        let expected_address = secret_key_to_address(&secret_key);

        let mut proposal = BlockProposal {
            number: 1,
            parent_hash: B256::ZERO,
            timestamp: 1234567890,
            transactions: vec![],
            proposer: expected_address,
            signature: BlockSignature::default(),
        };

        // Sign the block
        proposal.sign(&secret_key);

        // Verify signature is not empty
        assert!(!proposal.signature.is_empty());

        // Recover signer
        let signer = proposal.recover_signer().unwrap();
        assert_eq!(signer, expected_address);

        // Verify signature
        assert!(proposal.verify_signature());
    }

    #[test]
    fn test_invalid_signature() {
        let secret_key = test_secret_key();
        let expected_address = secret_key_to_address(&secret_key);

        let mut proposal = BlockProposal {
            number: 1,
            parent_hash: B256::ZERO,
            timestamp: 1234567890,
            transactions: vec![],
            proposer: expected_address,
            signature: BlockSignature::default(),
        };

        // Sign the block
        proposal.sign(&secret_key);

        // Tamper with the proposal
        proposal.number = 2;

        // Signature should now be invalid (signer won't match)
        let signer = proposal.recover_signer().unwrap();
        assert_ne!(signer, expected_address);
        assert!(!proposal.verify_signature());
    }

    #[test]
    fn test_verify_block_signature() {
        let secret_key = test_secret_key();
        let validator = secret_key_to_address(&secret_key);

        let mut proposal = BlockProposal {
            number: 1,
            parent_hash: B256::ZERO,
            timestamp: 1234567890,
            transactions: vec![],
            proposer: validator,
            signature: BlockSignature::default(),
        };

        proposal.sign(&secret_key);

        // Should pass with correct validator
        assert!(verify_block_signature(&proposal, &[validator]));

        // Should fail with wrong validator list
        let wrong_validator = address!("0000000000000000000000000000000000000001");
        assert!(!verify_block_signature(&proposal, &[wrong_validator]));
    }

    #[test]
    fn test_poa_consensus_creation() {
        let config = PoaConfig::new(test_secret_key(), Duration::from_secs(1));
        let consensus = PoaConsensus::new(config.clone());
        assert_eq!(consensus.current_block_number(), 0);
        assert_eq!(consensus.config().validator, config.validator);
    }

    #[tokio::test]
    async fn test_poa_block_production_with_signature() {
        let config = PoaConfig::new(test_secret_key(), Duration::from_millis(100));
        let expected_validator = config.validator;

        let consensus = PoaConsensus::new(config);
        let handle = consensus.start();

        tokio::time::sleep(Duration::from_millis(350)).await;

        let mut proposals = Vec::new();
        while let Some(proposal) = consensus.recv_proposal() {
            proposals.push(proposal);
        }

        assert!(proposals.len() >= 3, "Expected at least 3 blocks, got: {}", proposals.len());

        for proposal in &proposals {
            // Verify each block is properly signed
            assert!(proposal.verify_signature());
            assert_eq!(proposal.proposer, expected_validator);

            let signer = proposal.recover_signer().unwrap();
            assert_eq!(signer, expected_validator);
        }

        handle.abort();
    }

    #[test]
    fn test_finalize_block() {
        let config = PoaConfig::new(test_secret_key(), Duration::from_millis(500));
        let consensus = PoaConsensus::new(config);

        let block_hash = B256::from([1u8; 32]);
        consensus.finalize_block(block_hash);

        assert_eq!(*consensus.last_block_hash.lock().unwrap(), block_hash);
    }

    #[test]
    fn test_signature_bytes_roundtrip() {
        let sig = BlockSignature {
            r: B256::repeat_byte(0x11),
            s: B256::repeat_byte(0x22),
            v: 27,
        };

        let bytes = sig.to_bytes();
        let recovered = BlockSignature::from_bytes(&bytes).unwrap();

        assert_eq!(sig, recovered);
    }
}
