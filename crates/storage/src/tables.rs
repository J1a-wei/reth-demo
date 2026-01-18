//! DualVM database tables

use alloy_primitives::{Address, BlockNumber, B256, U256};
use bytes::BufMut;
use reth_codecs::Compact;
use reth_db_api::table::{Compress, Decompress, Decode, Encode, Table, TableInfo};
use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Helper module for serializing [u8; 65] as hex string
mod signature_serde {
    use super::*;

    pub fn serialize<S>(bytes: &[u8; 65], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let hex_string = hex::encode(bytes);
        serializer.serialize_str(&hex_string)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 65], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s: String = Deserialize::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        if bytes.len() != 65 {
            return Err(serde::de::Error::custom("signature must be 65 bytes"));
        }
        let mut arr = [0u8; 65];
        arr.copy_from_slice(&bytes);
        Ok(arr)
    }
}

/// Table name constants
pub mod table_names {
    pub const DUALVM_BLOCKS: &str = "DualvmBlocks";
    pub const DUALVM_ACCOUNTS: &str = "DualvmAccounts";
    pub const DUALVM_COUNTERS: &str = "DualvmCounters";
    pub const DUALVM_STORAGE: &str = "DualvmStorage";
    pub const DUALVM_TX_HASHES: &str = "DualvmTxHashes";
    pub const DUALVM_TRANSACTIONS: &str = "DualvmTransactions";
}

/// Storage key combining address and slot
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize, Default)]
pub struct StorageKey {
    pub address: Address,
    pub slot: U256,
}

impl Encode for StorageKey {
    type Encoded = Vec<u8>;

    fn encode(self) -> Self::Encoded {
        let mut buf = Vec::with_capacity(52);
        buf.extend_from_slice(self.address.as_slice());
        buf.extend_from_slice(&self.slot.to_be_bytes::<32>());
        buf
    }
}

impl Decode for StorageKey {
    fn decode(value: &[u8]) -> Result<Self, reth_db_api::DatabaseError> {
        if value.len() < 52 {
            return Err(reth_db_api::DatabaseError::Decode);
        }
        let address = Address::from_slice(&value[..20]);
        let slot = U256::from_be_slice(&value[20..52]);
        Ok(Self { address, slot })
    }
}

/// DualVM block header stored in database
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredDualvmBlock {
    pub hash: B256,
    pub parent_hash: B256,
    pub timestamp: u64,
    pub gas_limit: u64,
    pub gas_used: u64,
    pub miner: Address,
    pub evm_state_root: B256,
    pub dexvm_state_root: B256,
    pub combined_state_root: B256,
    pub transaction_count: u64,
    /// Block signature (65 bytes: r[32] + s[32] + v[1])
    #[serde(with = "signature_serde", default = "default_signature")]
    pub signature: [u8; 65],
    /// Transaction hashes included in this block
    #[serde(default)]
    pub transaction_hashes: Vec<B256>,
}

fn default_signature() -> [u8; 65] {
    [0u8; 65]
}

impl Default for StoredDualvmBlock {
    fn default() -> Self {
        Self {
            hash: B256::default(),
            parent_hash: B256::default(),
            timestamp: 0,
            gas_limit: 0,
            gas_used: 0,
            miner: Address::default(),
            evm_state_root: B256::default(),
            dexvm_state_root: B256::default(),
            combined_state_root: B256::default(),
            transaction_count: 0,
            signature: [0u8; 65],
            transaction_hashes: vec![],
        }
    }
}

impl Compact for StoredDualvmBlock {
    fn to_compact<B>(&self, buf: &mut B) -> usize
    where
        B: BufMut + AsMut<[u8]>,
    {
        buf.put_slice(self.hash.as_slice());
        buf.put_slice(self.parent_hash.as_slice());
        buf.put_u64(self.timestamp);
        buf.put_u64(self.gas_limit);
        buf.put_u64(self.gas_used);
        buf.put_slice(self.miner.as_slice());
        buf.put_slice(self.evm_state_root.as_slice());
        buf.put_slice(self.dexvm_state_root.as_slice());
        buf.put_slice(self.combined_state_root.as_slice());
        buf.put_u64(self.transaction_count);
        buf.put_slice(&self.signature);
        // Write transaction hashes count and data
        buf.put_u32(self.transaction_hashes.len() as u32);
        for tx_hash in &self.transaction_hashes {
            buf.put_slice(tx_hash.as_slice());
        }
        245 + 4 + self.transaction_hashes.len() * 32
    }

    fn from_compact(buf: &[u8], _len: usize) -> (Self, &[u8]) {
        let hash = B256::from_slice(&buf[0..32]);
        let parent_hash = B256::from_slice(&buf[32..64]);
        let timestamp = u64::from_be_bytes(buf[64..72].try_into().unwrap());
        let gas_limit = u64::from_be_bytes(buf[72..80].try_into().unwrap());
        let gas_used = u64::from_be_bytes(buf[80..88].try_into().unwrap());
        let miner = Address::from_slice(&buf[88..108]);
        let evm_state_root = B256::from_slice(&buf[108..140]);
        let dexvm_state_root = B256::from_slice(&buf[140..172]);
        let combined_state_root = B256::from_slice(&buf[172..204]);
        let transaction_count = u64::from_be_bytes(buf[204..212].try_into().unwrap());
        let mut signature = [0u8; 65];
        let mut transaction_hashes = vec![];
        let mut remaining = &buf[212..];

        // Handle old blocks without signature (backwards compatibility)
        if buf.len() >= 277 {
            signature.copy_from_slice(&buf[212..277]);
            remaining = &buf[277..];

            // Read transaction hashes if present (new format)
            if remaining.len() >= 4 {
                let tx_count = u32::from_be_bytes(remaining[0..4].try_into().unwrap()) as usize;
                remaining = &remaining[4..];

                for _ in 0..tx_count {
                    if remaining.len() >= 32 {
                        transaction_hashes.push(B256::from_slice(&remaining[0..32]));
                        remaining = &remaining[32..];
                    }
                }
            }
        }

        (
            Self {
                hash,
                parent_hash,
                timestamp,
                gas_limit,
                gas_used,
                miner,
                evm_state_root,
                dexvm_state_root,
                combined_state_root,
                transaction_count,
                signature,
                transaction_hashes,
            },
            remaining,
        )
    }
}

impl Compress for StoredDualvmBlock {
    type Compressed = Vec<u8>;

    fn compress_to_buf<B: BufMut + AsMut<[u8]>>(&self, buf: &mut B) {
        self.to_compact(buf);
    }
}

impl Decompress for StoredDualvmBlock {
    fn decompress(value: &[u8]) -> Result<Self, reth_db_api::DatabaseError> {
        // Accept both old format (212 bytes) and new format (277 bytes)
        if value.len() < 212 {
            return Err(reth_db_api::DatabaseError::Decode);
        }
        let (block, _) = Self::from_compact(value, value.len());
        Ok(block)
    }
}

/// DualVM account state stored in database
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StoredDualvmAccount {
    pub balance: U256,
    pub nonce: u64,
    pub code_hash: B256,
    pub is_contract: bool,
}

impl Compact for StoredDualvmAccount {
    fn to_compact<B>(&self, buf: &mut B) -> usize
    where
        B: BufMut + AsMut<[u8]>,
    {
        buf.put_slice(&self.balance.to_be_bytes::<32>());
        buf.put_u64(self.nonce);
        buf.put_slice(self.code_hash.as_slice());
        buf.put_u8(self.is_contract as u8);
        73
    }

    fn from_compact(buf: &[u8], _len: usize) -> (Self, &[u8]) {
        let balance = U256::from_be_slice(&buf[0..32]);
        let nonce = u64::from_be_bytes(buf[32..40].try_into().unwrap());
        let code_hash = B256::from_slice(&buf[40..72]);
        let is_contract = buf[72] != 0;

        (Self { balance, nonce, code_hash, is_contract }, &buf[73..])
    }
}

impl Compress for StoredDualvmAccount {
    type Compressed = Vec<u8>;

    fn compress_to_buf<B: BufMut + AsMut<[u8]>>(&self, buf: &mut B) {
        self.to_compact(buf);
    }
}

impl Decompress for StoredDualvmAccount {
    fn decompress(value: &[u8]) -> Result<Self, reth_db_api::DatabaseError> {
        if value.len() < 73 {
            return Err(reth_db_api::DatabaseError::Decode);
        }
        let (account, _) = Self::from_compact(value, value.len());
        Ok(account)
    }
}

/// DexVM counter value
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StoredCounter {
    pub value: u64,
}

impl Compact for StoredCounter {
    fn to_compact<B>(&self, buf: &mut B) -> usize
    where
        B: BufMut + AsMut<[u8]>,
    {
        buf.put_u64(self.value);
        8
    }

    fn from_compact(buf: &[u8], _len: usize) -> (Self, &[u8]) {
        let value = u64::from_be_bytes(buf[0..8].try_into().unwrap());
        (Self { value }, &buf[8..])
    }
}

impl Compress for StoredCounter {
    type Compressed = Vec<u8>;

    fn compress_to_buf<B: BufMut + AsMut<[u8]>>(&self, buf: &mut B) {
        self.to_compact(buf);
    }
}

impl Decompress for StoredCounter {
    fn decompress(value: &[u8]) -> Result<Self, reth_db_api::DatabaseError> {
        if value.len() < 8 {
            return Err(reth_db_api::DatabaseError::Decode);
        }
        let (counter, _) = Self::from_compact(value, value.len());
        Ok(counter)
    }
}

/// Storage value wrapper
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StoredStorageValue {
    pub value: U256,
}

impl Compact for StoredStorageValue {
    fn to_compact<B>(&self, buf: &mut B) -> usize
    where
        B: BufMut + AsMut<[u8]>,
    {
        buf.put_slice(&self.value.to_be_bytes::<32>());
        32
    }

    fn from_compact(buf: &[u8], _len: usize) -> (Self, &[u8]) {
        let value = U256::from_be_slice(&buf[0..32]);
        (Self { value }, &buf[32..])
    }
}

impl Compress for StoredStorageValue {
    type Compressed = Vec<u8>;

    fn compress_to_buf<B: BufMut + AsMut<[u8]>>(&self, buf: &mut B) {
        self.to_compact(buf);
    }
}

impl Decompress for StoredStorageValue {
    fn decompress(value: &[u8]) -> Result<Self, reth_db_api::DatabaseError> {
        if value.len() < 32 {
            return Err(reth_db_api::DatabaseError::Decode);
        }
        let (storage, _) = Self::from_compact(value, value.len());
        Ok(storage)
    }
}

/// Transaction info stored for lookup
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StoredTxInfo {
    pub block_number: BlockNumber,
    pub tx_index: u64,
}

/// Full transaction data stored for block body retrieval
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct StoredTransaction {
    /// RLP-encoded transaction bytes
    pub rlp_bytes: Vec<u8>,
}

impl Compact for StoredTransaction {
    fn to_compact<B>(&self, buf: &mut B) -> usize
    where
        B: BufMut + AsMut<[u8]>,
    {
        let len = self.rlp_bytes.len();
        buf.put_u32(len as u32);
        buf.put_slice(&self.rlp_bytes);
        4 + len
    }

    fn from_compact(buf: &[u8], _len: usize) -> (Self, &[u8]) {
        let data_len = u32::from_be_bytes(buf[0..4].try_into().unwrap()) as usize;
        let rlp_bytes = buf[4..4 + data_len].to_vec();
        (Self { rlp_bytes }, &buf[4 + data_len..])
    }
}

impl Compress for StoredTransaction {
    type Compressed = Vec<u8>;

    fn compress_to_buf<B: BufMut + AsMut<[u8]>>(&self, buf: &mut B) {
        self.to_compact(buf);
    }
}

impl Decompress for StoredTransaction {
    fn decompress(value: &[u8]) -> Result<Self, reth_db_api::DatabaseError> {
        if value.len() < 4 {
            return Err(reth_db_api::DatabaseError::Decode);
        }
        let (tx, _) = Self::from_compact(value, value.len());
        Ok(tx)
    }
}

impl Compact for StoredTxInfo {
    fn to_compact<B>(&self, buf: &mut B) -> usize
    where
        B: BufMut + AsMut<[u8]>,
    {
        buf.put_u64(self.block_number);
        buf.put_u64(self.tx_index);
        16
    }

    fn from_compact(buf: &[u8], _len: usize) -> (Self, &[u8]) {
        let block_number = u64::from_be_bytes(buf[0..8].try_into().unwrap());
        let tx_index = u64::from_be_bytes(buf[8..16].try_into().unwrap());
        (Self { block_number, tx_index }, &buf[16..])
    }
}

impl Compress for StoredTxInfo {
    type Compressed = Vec<u8>;

    fn compress_to_buf<B: BufMut + AsMut<[u8]>>(&self, buf: &mut B) {
        self.to_compact(buf);
    }
}

impl Decompress for StoredTxInfo {
    fn decompress(value: &[u8]) -> Result<Self, reth_db_api::DatabaseError> {
        if value.len() < 16 {
            return Err(reth_db_api::DatabaseError::Decode);
        }
        let (info, _) = Self::from_compact(value, value.len());
        Ok(info)
    }
}

// Table definitions

/// DualVM blocks table: BlockNumber -> StoredDualvmBlock
#[derive(Debug)]
pub struct DualvmBlocks;

impl Table for DualvmBlocks {
    const NAME: &'static str = table_names::DUALVM_BLOCKS;
    const DUPSORT: bool = false;
    type Key = BlockNumber;
    type Value = StoredDualvmBlock;
}

impl TableInfo for DualvmBlocks {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn is_dupsort(&self) -> bool {
        Self::DUPSORT
    }
}

/// DualVM accounts table: Address -> StoredDualvmAccount
#[derive(Debug)]
pub struct DualvmAccounts;

impl Table for DualvmAccounts {
    const NAME: &'static str = table_names::DUALVM_ACCOUNTS;
    const DUPSORT: bool = false;
    type Key = Address;
    type Value = StoredDualvmAccount;
}

impl TableInfo for DualvmAccounts {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn is_dupsort(&self) -> bool {
        Self::DUPSORT
    }
}

/// DualVM counters table (for DexVM): Address -> StoredCounter
#[derive(Debug)]
pub struct DualvmCounters;

impl Table for DualvmCounters {
    const NAME: &'static str = table_names::DUALVM_COUNTERS;
    const DUPSORT: bool = false;
    type Key = Address;
    type Value = StoredCounter;
}

impl TableInfo for DualvmCounters {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn is_dupsort(&self) -> bool {
        Self::DUPSORT
    }
}

/// DualVM storage table: StorageKey -> StoredStorageValue
#[derive(Debug)]
pub struct DualvmStorage;

impl Table for DualvmStorage {
    const NAME: &'static str = table_names::DUALVM_STORAGE;
    const DUPSORT: bool = false;
    type Key = StorageKey;
    type Value = StoredStorageValue;
}

impl TableInfo for DualvmStorage {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn is_dupsort(&self) -> bool {
        Self::DUPSORT
    }
}

/// DualVM transaction hashes table: B256 -> StoredTxInfo
#[derive(Debug)]
pub struct DualvmTxHashes;

impl Table for DualvmTxHashes {
    const NAME: &'static str = table_names::DUALVM_TX_HASHES;
    const DUPSORT: bool = false;
    type Key = B256;
    type Value = StoredTxInfo;
}

impl TableInfo for DualvmTxHashes {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn is_dupsort(&self) -> bool {
        Self::DUPSORT
    }
}

/// DualVM transactions table: B256 (tx_hash) -> StoredTransaction
#[derive(Debug)]
pub struct DualvmTransactions;

impl Table for DualvmTransactions {
    const NAME: &'static str = table_names::DUALVM_TRANSACTIONS;
    const DUPSORT: bool = false;
    type Key = B256;
    type Value = StoredTransaction;
}

impl TableInfo for DualvmTransactions {
    fn name(&self) -> &'static str {
        Self::NAME
    }

    fn is_dupsort(&self) -> bool {
        Self::DUPSORT
    }
}

/// TableSet implementation for DualVM tables
pub struct DualvmTableSet;

impl reth_db_api::TableSet for DualvmTableSet {
    fn tables() -> Box<dyn Iterator<Item = Box<dyn TableInfo>>> {
        Box::new(
            vec![
                Box::new(DualvmBlocks) as Box<dyn TableInfo>,
                Box::new(DualvmAccounts) as Box<dyn TableInfo>,
                Box::new(DualvmCounters) as Box<dyn TableInfo>,
                Box::new(DualvmStorage) as Box<dyn TableInfo>,
                Box::new(DualvmTxHashes) as Box<dyn TableInfo>,
                Box::new(DualvmTransactions) as Box<dyn TableInfo>,
            ]
            .into_iter(),
        )
    }
}
