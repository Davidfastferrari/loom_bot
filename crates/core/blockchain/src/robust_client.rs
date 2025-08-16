
use loom_core_blockchain_actors_blockchain::{BackrunBot, ArbitrageBot};
use loom_core_blockchain_actors_block_history::BlockHistoryActor;
use alloy_network::Network;
use alloy_provider::{Provider, ProviderBuilder, RootProvider};
use alloy_rpc_client::{ClientBuilder, WsConnect};
use eyre::{eyre, Result};
use loom_core_topology_shared::{create_optimized_ws_connect, RateLimitedProvider};
use std::time::Duration;
use tracing::{debug, error, info, warn};
use url::Url;

/// Creates a robust provider with automatic reconnection and error handling
pub async fn create_robust_provider<N>(
    url: &str,
    transport_type: &str,
    max_retries: usize,
) -> Result<RateLimitedProvider<N>>
where
    N: Network,
    RootProvider: Provider<N>,
    RateLimitedProvider<N>: Provider<N>,
{
    let mut retry_count = 0;
    let max_retry_delay = Duration::from_secs(10);

    loop {
        if retry_count >= max_retries {
            return Err(eyre!("Maximum connection retries ({}) exceeded", max_retries));
        }

        if retry_count > 0 {
            let delay = Duration::from_secs(2u64.pow(retry_count as u32).min(max_retry_delay.as_secs()));
            warn!(
                "Connection attempt failed. Retrying in {} seconds (attempt {}/{})",
                delay.as_secs(),
                retry_count + 1,
                max_retries
            );
            tokio::time::sleep(delay).await;
        }

        match transport_type.to_lowercase().as_str() {
            "ws" => {
                info!("Connecting to WebSocket endpoint: {}", url);
                let parsed_url = match Url::parse(url) {
                    Ok(url) => url,
                    Err(e) => {
                        error!("Failed to parse WebSocket URL: {}", e);
                        retry_count += 1;
                        continue;
                    }
                };

                // Use our optimized WebSocket client builder
                let _ws_builder = create_optimized_ws_connect(url);
                let ws_connect = WsConnect::new(parsed_url);

                match ClientBuilder::default().ws(ws_connect).await {
                    Ok(client) => {
                        let provider = ProviderBuilder::new()
                            .disable_recommended_fillers()
                            .on_client(client);
                        let provider = RateLimitedProvider::new(provider, 1);
                        debug!("Successfully connected to WebSocket endpoint");
                        return Ok(provider);
                    }
                    Err(e) => {
                        error!("Failed to connect to WebSocket endpoint: {}", e);
                        retry_count += 1;
                        continue;
                    }
                }
            }
            "http" => {
                info!("Connecting to HTTP endpoint: {}", url);
                let parsed_url = match Url::parse(url) {
                    Ok(url) => url,
                    Err(e) => {
                        error!("Failed to parse HTTP URL: {}", e);
                        retry_count += 1;
                        continue;
                    }
                };

                // Create HTTP client
                let client = ClientBuilder::default().http(parsed_url);
                let provider = ProviderBuilder::new()
                    .disable_recommended_fillers()
                    .on_client(client);
                let provider = RateLimitedProvider::new(provider, 1);
                debug!("Successfully connected to HTTP endpoint");
                return Ok(provider);
            }
            _ => {
                error!("Unsupported transport type: {}", transport_type);
                return Err(eyre!("Unsupported transport type: {}", transport_type));
            }
        }
    }
}

pub fn start_bots() {
    let backrun_bot = BackrunBot::new();
    let arbitrage_bot = ArbitrageBot::new();
    let block_history_actor = BlockHistoryActor::new();

    backrun_bot.run();
    arbitrage_bot.run();
    block_history_actor.run();
}
