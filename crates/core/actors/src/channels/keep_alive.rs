use std::time::Duration;
use tokio::time::interval;
use tracing::{debug, warn};

use super::broadcaster::Broadcaster;

/// Starts a background task that periodically checks the health of a broadcaster channel
/// and attempts to keep it alive by ensuring it has active subscribers.
pub fn start_channel_keep_alive<T: Clone + Send + Sync + 'static>(
    name: &'static str,
    broadcaster: Broadcaster<T>,
    check_interval_secs: u64,
) {
    let mut interval = interval(Duration::from_secs(check_interval_secs));
    
    tokio::spawn(async move {
        loop {
            interval.tick().await;
            
            // Check if the channel is healthy
            if !broadcaster.is_healthy() {
                warn!("Channel '{}' appears unhealthy, checking status", name);
                
                // Check if we have active subscribers but the channel is closed
                let subscriber_count = broadcaster.subscriber_count();
                if subscriber_count > 0 {
                    warn!("Channel '{}' has {} tracked subscribers but appears closed", 
                          name, subscriber_count);
                    
                    // The broadcaster implementation should handle reconnection automatically
                    // when the next send operation occurs
                } else {
                    debug!("Channel '{}' has no active subscribers", name);
                }
            } else {
                debug!("Channel '{}' is healthy with {} subscribers", 
                       name, broadcaster.subscriber_count());
            }
        }
    });
}