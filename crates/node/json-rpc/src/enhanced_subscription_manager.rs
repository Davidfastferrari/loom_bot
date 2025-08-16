use alloy_network::Ethereum;
use alloy_provider::{Provider, ProviderBuilder};
use alloy_rpc_types::Header;
use eyre::{eyre, Result};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::broadcast;
use tokio::time::{interval, sleep};
use tracing::{debug, error, info, warn};
use url::Url;

/// Enhanced subscription manager that handles WebSocket reconnections and health monitoring
pub struct EnhancedSubscriptionManager<P> {
    provider: P,
    backup_urls: Vec<String>,
    current_url_index: usize,
    reconnect_attempts: usize,
    max_reconnect_attempts: usize,
    reconnect_delay: Duration,
}

impl<P> EnhancedSubscriptionManager<P>
where
    P: Provider<Ethereum> + Send + Sync + Clone + 'static,
{
    pub fn new(provider: P, backup_urls: Vec<String>) -> Self {
        Self {
            provider,
            backup_urls,
            current_url_index: 0,
            reconnect_attempts: 0,
            max_reconnect_attempts: 10,
            reconnect_delay: Duration::from_secs(5),
        }
    }

    /// Start enhanced block header subscription with automatic reconnection
    pub async fn start_block_header_subscription(
        &mut self,
        sender: broadcast::Sender<Header>,
    ) -> Result<()> {
        loop {
            match self.try_subscribe_to_headers(sender.clone()).await {
                Ok(_) => {
                    info!("Block header subscription ended normally");
                    break;
                }
                Err(e) => {
                    error!("Block header subscription failed: {}", e);
                    
                    if self.reconnect_attempts >= self.max_reconnect_attempts {
                        return Err(eyre!("Max reconnection attempts reached"));
                    }
                    
                    self.reconnect_attempts += 1;
                    warn!("Attempting reconnection {} of {}", 
                          self.reconnect_attempts, self.max_reconnect_attempts);
                    
                    // Try next backup URL if available
                    if !self.backup_urls.is_empty() {
                        self.current_url_index = (self.current_url_index + 1) % self.backup_urls.len();
                        info!("Switching to backup URL: {}", self.backup_urls[self.current_url_index]);
                        
                        // Recreate provider with new URL
                        if let Ok(new_provider) = self.create_provider_from_url(&self.backup_urls[self.current_url_index]).await {
                            self.provider = new_provider;
                        }
                    }
                    
                    // Exponential backoff
                    let delay = self.reconnect_delay * 2_u32.pow(self.reconnect_attempts.min(5) as u32);
                    warn!("Waiting {} seconds before reconnection attempt", delay.as_secs());
                    sleep(delay).await;
                }
            }
        }
        
        Ok(())
    }

    /// Try to subscribe to block headers
    async fn try_subscribe_to_headers(
        &self,
        sender: broadcast::Sender<Header>,
    ) -> Result<()> {
        info!("Starting block header subscription");
        
        // Create subscription
        let sub = self.provider.subscribe_blocks().await
            .map_err(|e| eyre!("Failed to create block subscription: {}", e))?;
        
        let mut stream = sub.into_stream();
        
        // Health check interval
        let mut health_check = interval(Duration::from_secs(30));
        let mut last_block_time = std::time::Instant::now();
        
        loop {
            tokio::select! {
                // Handle incoming blocks
                block_result = stream.next() => {
                    match block_result {
                        Some(Ok(block)) => {
                            last_block_time = std::time::Instant::now();
                            debug!("Received block: {} ({})", block.number, block.hash);
                            
                            // Send to subscribers
                            match sender.send(block) {
                                Ok(subscriber_count) => {
                                    debug!("Block sent to {} subscribers", subscriber_count);
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
                            warn!("Block stream ended");
                            return Err(eyre!("Block stream ended unexpectedly"));
                        }
                    }
                }
                
                // Health check
                _ = health_check.tick() => {
                    let time_since_last_block = last_block_time.elapsed();
                    if time_since_last_block > Duration::from_secs(60) {
                        warn!("No blocks received for {} seconds, connection may be stale", 
                              time_since_last_block.as_secs());
                        return Err(eyre!("Connection appears stale - no blocks for {} seconds", 
                                         time_since_last_block.as_secs()));
                    }
                    debug!("Health check passed - last block {} seconds ago", 
                           time_since_last_block.as_secs());
                }
            }
        }
    }

    /// Create a new provider from URL
    async fn create_provider_from_url(&self, url: &str) -> Result<P> {
        // This is a placeholder - in reality you'd need to implement provider creation
        // based on the specific provider type P
        Err(eyre!("Provider recreation not implemented for this type"))
    }

    /// Reset reconnection state after successful connection
    pub fn reset_reconnection_state(&mut self) {
        self.reconnect_attempts = 0;
        info!("Reconnection state reset after successful connection");
    }

    /// Get current connection status
    pub fn get_connection_status(&self) -> ConnectionStatus {
        ConnectionStatus {
            current_url_index: self.current_url_index,
            reconnect_attempts: self.reconnect_attempts,
            max_reconnect_attempts: self.max_reconnect_attempts,
            is_connected: self.reconnect_attempts == 0,
        }
    }
}

#[derive(Debug, Clone)]
pub struct ConnectionStatus {
    pub current_url_index: usize,
    pub reconnect_attempts: usize,
    pub max_reconnect_attempts: usize,
    pub is_connected: bool,
}

/// Enhanced block subscription worker that uses the subscription manager
pub async fn enhanced_block_subscription_worker<P>(
    provider: P,
    backup_urls: Vec<String>,
    sender: broadcast::Sender<Header>,
) -> Result<()>
where
    P: Provider<Ethereum> + Send + Sync + Clone + 'static,
{
    let mut manager = EnhancedSubscriptionManager::new(provider, backup_urls);
    
    loop {
        match manager.start_block_header_subscription(sender.clone()).await {
            Ok(_) => {
                info!("Block subscription completed successfully");
                break;
            }
            Err(e) => {
                error!("Block subscription failed permanently: {}", e);
                // Wait before trying to restart the entire subscription system
                sleep(Duration::from_secs(30)).await;
                
                // Reset and try again
                manager.reset_reconnection_state();
                warn!("Restarting entire subscription system");
            }
        }
    }
    
    Ok(())
}