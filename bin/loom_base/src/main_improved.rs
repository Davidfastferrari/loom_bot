use alloy_network::Ethereum;
use alloy_provider::Provider;
use eyre::{eyre, Result};
use loom_core_blockchain::create_robust_provider;
use loom_core_topology::configure_dns_settings;
use std::env;
use std::time::Duration;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt::init();
    
    // Configure DNS settings
    configure_dns_settings();
    
    // Get RPC URL from environment or use default
    let rpc_url = env::var("RPC_URL").unwrap_or_else(|_| {
        info!("RPC_URL not set, using default mainnet RPC");
        "https://eth-mainnet.g.alchemy.com/v2/demo".to_string()
    });
    
    // Get transport type from environment or use default
    let transport_type = env::var("TRANSPORT_TYPE").unwrap_or_else(|_| {
        info!("TRANSPORT_TYPE not set, using HTTP");
        "http".to_string()
    });
    
    // Create a robust provider with automatic reconnection
    info!("Connecting to Ethereum node at {}", rpc_url);
    let provider = create_robust_provider::<Ethereum>(&rpc_url, &transport_type, 5).await?;
    
    // Test the connection by getting the latest block number
    match provider.get_block_number().await {
        Ok(block_number) => {
            info!("Successfully connected to Ethereum node. Latest block: {}", block_number);
        }
        Err(e) => {
            error!("Failed to get latest block number: {}", e);
            return Err(eyre!("Failed to get latest block number: {}", e));
        }
    }
    
    // Keep the application running
    info!("Application started successfully. Press Ctrl+C to exit.");
    loop {
        tokio::time::sleep(Duration::from_secs(10)).await;
        
        // Periodically check connection
        match provider.get_block_number().await {
            Ok(block_number) => {
                info!("Connection healthy. Latest block: {}", block_number);
            }
            Err(e) => {
                error!("Connection error: {}", e);
            }
        }
    }
}