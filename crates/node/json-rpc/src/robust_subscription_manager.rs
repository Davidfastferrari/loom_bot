use alloy_network::Ethereum;
use alloy_provider::{Provider, ProviderBuilder, WsConnect};
use alloy_rpc_types::Header;
use eyre::{eyre, Result};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::time::{interval, sleep, timeout};
use tracing::{debug, error, info, warn};
use url::Url;
use futures_util::StreamExt;

/// Robust subscription manager with automatic reconnection and health monitoring
pub struct RobustSubscriptionManager {
    primary_url: String,
    backup_urls: Vec<String>,
    current_url_index: usize,
    reconnect_attempts: usize,
    max_reconnect_attempts: usize,
    reconnect_delay: Duration,
    health_check_interval: Duration,
    connection_timeout: Duration,
}

impl RobustSubscriptionManager {
    pub fn new(primary_url: String, backup_urls: Vec<String>) -> Self {
        Self {
            primary_url,
            backup_urls,
            current_url_index: 0,
            reconnect_attempts: 0,
            max_reconnect_attempts: 20,
            reconnect_delay: Duration::from_secs(2),
            health_check_interval: Duration::from_secs(30),
            connection_timeout: Duration::from_secs(10),
        }
    }

    /// Start robust block header subscription with automatic reconnection
    pub async fn start_robust_block_subscription(
        &mut self,
        sender: broadcast::Sender<Header>,
    ) -> Result<()> {
        info!("Starting robust block subscription manager");
        
        loop {
            let current_url = self.get_current_url();
            info!("Attempting connection to: {}", current_url);
            
            match self.try_connect_and_subscribe(&current_url, sender.clone()).await {
                Ok(_) => {
                    info!("Block subscription ended normally");
                    self.reset_reconnection_state();
                    break;
                }
                Err(e) => {
                    error!("Block subscription failed: {}", e);
                    
                    if self.reconnect_attempts >= self.max_reconnect_attempts {
                        error!("Max reconnection attempts reached, giving up");
                        return Err(eyre!("Max reconnection attempts reached"));
                    }
                    
                    self.reconnect_attempts += 1;
                    self.switch_to_next_url();
                    
                    // Exponential backoff with jitter
                    let delay = self.calculate_backoff_delay();
                    warn!("Waiting {} seconds before reconnection attempt {} of {}", 
                          delay.as_secs(), self.reconnect_attempts, self.max_reconnect_attempts);
                    sleep(delay).await;
                }
            }
        }
        
        Ok(())
    }

    /// Try to connect and subscribe to block headers
    async fn try_connect_and_subscribe(
        &self,
        url: &str,
        sender: broadcast::Sender<Header>,
    ) -> Result<()> {
        info!("Connecting to WebSocket: {}", url);
        
        // Create provider with timeout
        let provider = timeout(
            self.connection_timeout,
            self.create_provider(url)
        ).await
        .map_err(|_| eyre!("Connection timeout"))?
        .map_err(|e| eyre!("Failed to create provider: {}", e))?;
        
        info!("Successfully connected, starting block subscription");
        
        // Create subscription with timeout
        let sub = timeout(
            self.connection_timeout,
            provider.subscribe_blocks()
        ).await
        .map_err(|_| eyre!("Subscription timeout"))?
        .map_err(|e| eyre!("Failed to create block subscription: {}", e))?;
        
        let mut stream = sub.into_stream();
        
        // Health check interval
        let mut health_check = interval(self.health_check_interval);
        let mut last_block_time = std::time::Instant::now();
        let mut block_count = 0u64;
        
        info!("Block subscription active, waiting for blocks...");
        
        loop {
            tokio::select! {
                // Handle incoming blocks
                block_result = stream.next() => {
                    match block_result {
                        Some(Ok(block)) => {
                            last_block_time = std::time::Instant::now();
                            block_count += 1;
                            
                            debug!("Received block #{} (hash: {}, total: {})", 
                                   block.number, block.hash, block_count);
                            
                            // Send to subscribers
                            match sender.send(block) {
                                Ok(subscriber_count) => {
                                    if subscriber_count == 0 {
                                        warn!("No subscribers for block updates");
                                    } else {
                                        debug!("Block sent to {} subscribers", subscriber_count);
                                    }
                                }
                                Err(e) => {
                                    warn!("Failed to send block to subscribers: {}", e);
                                    // Continue anyway - subscribers might reconnect
                                }
                            }
                        }
                        Some(Err(e)) => {
                            error!("Error in block stream: {}", e);
                            return Err(eyre!("Block stream error: {}", e));
                        }
                        None => {
                            warn!("Block stream ended unexpectedly");
                            return Err(eyre!("Block stream ended"));
                        }
                    }
                }
                
                // Health check
                _ = health_check.tick() => {
                    let time_since_last_block = last_block_time.elapsed();
                    if time_since_last_block > Duration::from_secs(60) {
                        error!("No blocks received for {} seconds, connection appears stale", 
                              time_since_last_block.as_secs());
                        return Err(eyre!("Connection stale - no blocks for {} seconds", 
                                         time_since_last_block.as_secs()));
                    }
                    
                    info!("Health check passed - {} blocks received, last block {} seconds ago", 
                          block_count, time_since_last_block.as_secs());
                }
            }
        }
    }

    /// Create a new provider from URL
    async fn create_provider(&self, url: &str) -> Result<impl Provider<Ethereum> + Clone> {
        let ws = WsConnect::new(url);
        let provider = ProviderBuilder::new()
            .on_ws(ws)
            .await
            .map_err(|e| eyre!("Failed to create WebSocket provider: {}", e))?;
        
        Ok(provider)
    }

    /// Get the current URL to use
    fn get_current_url(&self) -> String {
        if self.current_url_index == 0 {
            self.primary_url.clone()
        } else {
            self.backup_urls.get(self.current_url_index - 1)
                .cloned()
                .unwrap_or_else(|| self.primary_url.clone())
        }
    }

    /// Switch to the next available URL
    fn switch_to_next_url(&mut self) {
        let total_urls = 1 + self.backup_urls.len();
        self.current_url_index = (self.current_url_index + 1) % total_urls;
        
        let new_url = self.get_current_url();
        info!("Switching to URL: {}", new_url);
    }

    /// Calculate backoff delay with exponential backoff and jitter
    fn calculate_backoff_delay(&self) -> Duration {
        let base_delay = self.reconnect_delay.as_secs();
        let exponential_delay = base_delay * 2_u64.pow(self.reconnect_attempts.min(6) as u32);
        let jitter = rand::random::<u64>() % (exponential_delay / 4 + 1); // Add up to 25% jitter
        
        Duration::from_secs((exponential_delay + jitter).min(300)) // Cap at 5 minutes
    }

    /// Reset reconnection state after successful connection
    fn reset_reconnection_state(&mut self) {
        self.reconnect_attempts = 0;
        self.current_url_index = 0; // Reset to primary URL
        info!("Reconnection state reset - back to primary URL");
    }

    /// Get current connection status
    pub fn get_connection_status(&self) -> ConnectionStatus {
        ConnectionStatus {
            current_url: self.get_current_url(),
            current_url_index: self.current_url_index,
            reconnect_attempts: self.reconnect_attempts,
            max_reconnect_attempts: self.max_reconnect_attempts,
            is_connected: self.reconnect_attempts == 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConnectionStatus {
    pub current_url: String,
    pub current_url_index: usize,
    pub reconnect_attempts: usize,
    pub max_reconnect_attempts: usize,
    pub is_connected: bool,
}

/// Enhanced block subscription worker using the robust manager
pub async fn robust_block_subscription_worker(
    primary_url: String,
    backup_urls: Vec<String>,
    sender: broadcast::Sender<Header>,
) -> Result<()> {
    let mut manager = RobustSubscriptionManager::new(primary_url, backup_urls);
    
    loop {
        match manager.start_robust_block_subscription(sender.clone()).await {
            Ok(_) => {
                info!("Block subscription completed successfully");
                break;
            }
            Err(e) => {
                error!("Block subscription failed permanently: {}", e);
                
                // Wait before trying to restart the entire subscription system
                sleep(Duration::from_secs(60)).await;
                
                // Create a new manager to reset all state
                manager = RobustSubscriptionManager::new(
                    manager.primary_url.clone(),
                    manager.backup_urls.clone()
                );
                
                warn!("Restarting entire subscription system with fresh state");
            }
        }
    }
    
    Ok(())
}