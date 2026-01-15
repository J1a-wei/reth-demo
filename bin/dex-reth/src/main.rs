//! dex-reth node binary
//!
//! A dual virtual machine blockchain node with EVM and DexVM support.

use alloy_primitives::{keccak256, Address, B256, U256};
use clap::Parser;
use dex_node::{DualVmNode, PoaConfig};
use dex_p2p::{P2pConfig, P2pService};
use serde::Deserialize;
use std::{collections::HashMap, path::PathBuf, time::Duration};

/// dex-reth node command line arguments
#[derive(Debug, Parser)]
#[clap(name = "dex-reth", about = "dex-reth - Dual Virtual Machine Node")]
struct Cli {
    /// EVM JSON-RPC port
    #[clap(long, default_value = "8545")]
    evm_rpc_port: u16,

    /// DexVM REST API port
    #[clap(long, default_value = "9845")]
    dexvm_port: u16,

    /// P2P listen port
    #[clap(long, default_value = "30303")]
    p2p_port: u16,

    /// Enable P2P networking
    #[clap(long)]
    enable_p2p: bool,

    /// Boot nodes (enode URLs)
    #[clap(long)]
    bootnodes: Vec<String>,

    /// Log level
    #[clap(long, default_value = "info")]
    log_level: String,

    /// Genesis file path
    #[clap(long)]
    genesis: Option<PathBuf>,

    /// Enable POA consensus
    #[clap(long)]
    enable_consensus: bool,

    /// Validator address
    #[clap(long, default_value = "0x0000000000000000000000000000000000000001")]
    validator: String,

    /// Block interval (milliseconds)
    #[clap(long, default_value = "500")]
    block_interval_ms: u64,

    /// Data directory
    #[clap(long, default_value = "./data")]
    datadir: PathBuf,

    /// Maximum number of P2P peers
    #[clap(long, default_value = "50")]
    max_peers: usize,
}

/// Genesis file format
#[derive(Debug, Deserialize)]
struct GenesisFile {
    config: GenesisConfig,
    alloc: HashMap<Address, AccountAlloc>,
}

#[derive(Debug, Deserialize)]
struct GenesisConfig {
    #[serde(rename = "chainId")]
    chain_id: u64,
}

#[derive(Debug, Deserialize)]
struct AccountAlloc {
    balance: String,
}

#[tokio::main]
async fn main() -> eyre::Result<()> {
    let cli = Cli::parse();

    init_tracing(&cli.log_level)?;

    tracing::info!("====================================");
    tracing::info!("  Starting dex-reth Node v0.1.0");
    tracing::info!("====================================");
    tracing::info!("EVM JSON-RPC port: {}", cli.evm_rpc_port);
    tracing::info!("DexVM REST API port: {}", cli.dexvm_port);
    tracing::info!("Data directory: {}", cli.datadir.display());

    // Load genesis file
    let (chain_id, genesis_alloc, genesis_hash) = if let Some(genesis_path) = &cli.genesis {
        tracing::info!("Loading genesis file from: {}", genesis_path.display());
        let genesis_data = std::fs::read_to_string(genesis_path)?;
        let genesis: GenesisFile = serde_json::from_str(&genesis_data)?;

        let chain_id = genesis.config.chain_id;
        tracing::info!("Chain ID: {}", chain_id);

        let mut alloc = HashMap::new();
        for (address, account) in genesis.alloc {
            let balance = if account.balance.starts_with("0x") {
                U256::from_str_radix(&account.balance[2..], 16)?
            } else {
                U256::from_str_radix(&account.balance, 10)?
            };

            tracing::info!("Genesis account: {} with balance {} wei", address, balance);

            alloc.insert(address, balance);
        }

        // Compute genesis hash from genesis data
        let genesis_hash = keccak256(genesis_data.as_bytes());

        (chain_id, Some(alloc), genesis_hash)
    } else {
        tracing::info!("No genesis file specified, using default chain ID 1");
        (1, None, B256::ZERO)
    };

    // Create node
    let mut node = DualVmNode::with_full_config(
        chain_id,
        genesis_alloc.clone().unwrap_or_default(),
        cli.datadir.clone(),
        None,
    );

    // Start P2P service if enabled
    let _p2p_handle = if cli.enable_p2p {
        tracing::info!("P2P networking enabled on port {}", cli.p2p_port);

        let p2p_config = P2pConfig::new(
            P2pConfig::random_secret_key(),
            chain_id,
            genesis_hash,
        )
        .with_port(cli.p2p_port)
        .with_max_peers(cli.max_peers);

        let p2p_service = P2pService::new(p2p_config);
        let handle = p2p_service.start().await?;

        tracing::info!("P2P service started, local_id={:?}", handle.local_id());

        Some(handle)
    } else {
        tracing::info!("P2P networking disabled");
        None
    };

    // Configure POA consensus
    if cli.enable_consensus {
        let validator =
            cli.validator.parse().map_err(|e| eyre::eyre!("Invalid validator address: {}", e))?;

        let latest_block = node.block_store().latest_block_number();
        let last_block_hash = node
            .block_store()
            .get_block_by_number(latest_block)
            .map(|b| b.hash)
            .unwrap_or_default();

        tracing::info!("POA consensus enabled");
        tracing::info!("Validator address: {:?}", validator);
        tracing::info!("Block interval: {}ms", cli.block_interval_ms);
        tracing::info!("Continuing from block {} (hash: {:?})", latest_block, last_block_hash);

        let poa_config = PoaConfig {
            validator,
            block_interval: Duration::from_millis(cli.block_interval_ms),
            starting_block: latest_block,
        };

        node.set_consensus(poa_config, last_block_hash);
    } else {
        tracing::info!("POA consensus not enabled (RPC-only mode)");
    }

    // Start EVM JSON-RPC service
    let evm_rpc_handle = node.start_evm_rpc(cli.evm_rpc_port).await?;
    tracing::info!("EVM JSON-RPC available at: http://127.0.0.1:{}", cli.evm_rpc_port);

    // Start DexVM REST API service
    let dexvm_rpc_handle = node.start_dexvm_rpc(cli.dexvm_port).await?;
    tracing::info!("DexVM REST API available at: http://127.0.0.1:{}", cli.dexvm_port);

    tracing::info!("====================================");
    tracing::info!("  dex-reth Node started successfully");
    tracing::info!("====================================");
    tracing::info!("");
    tracing::info!("Endpoints:");
    tracing::info!("  - EVM RPC:    http://127.0.0.1:{}", cli.evm_rpc_port);
    tracing::info!("  - DexVM API:  http://127.0.0.1:{}", cli.dexvm_port);
    tracing::info!("  - Health:     http://127.0.0.1:{}/health", cli.dexvm_port);
    if cli.enable_p2p {
        tracing::info!("  - P2P:        0.0.0.0:{}", cli.p2p_port);
    }
    tracing::info!("");
    tracing::info!("Data stored in: {}", cli.datadir.display());

    if cli.enable_consensus {
        let consensus_handle =
            node.start_consensus().ok_or_else(|| eyre::eyre!("Failed to start consensus"))?;

        tracing::info!("POA consensus engine started, auto block production enabled");

        let consensus_loop = tokio::spawn(async move {
            if let Err(e) = node.run_consensus_loop().await {
                tracing::error!("Consensus loop error: {}", e);
            }
        });

        tracing::info!("");
        tracing::info!("Press Ctrl+C to stop");

        tokio::signal::ctrl_c().await?;

        tracing::info!("");
        tracing::info!("Shutting down dex-reth Node...");

        consensus_handle.abort();
        consensus_loop.abort();
        dexvm_rpc_handle.abort();
        evm_rpc_handle.stop()?;
    } else {
        tracing::info!("");
        tracing::info!("Press Ctrl+C to stop");

        tokio::signal::ctrl_c().await?;

        tracing::info!("");
        tracing::info!("Shutting down dex-reth Node...");

        dexvm_rpc_handle.abort();
        evm_rpc_handle.stop()?;
    }

    tracing::info!("dex-reth Node stopped.");
    Ok(())
}

fn init_tracing(level: &str) -> eyre::Result<()> {
    let filter = tracing_subscriber::EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(level));

    tracing_subscriber::fmt()
        .with_env_filter(filter)
        .try_init()
        .map_err(|e| eyre::eyre!("Failed to initialize tracing: {}", e))?;

    Ok(())
}
