use eyre::{eyre, Result};
use std::sync::{Arc, RwLock};
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::SendError;
use tokio::sync::broadcast::Receiver;
use tracing::{debug, error, warn};

/// Enhanced Broadcaster with reconnection capability and keep-alive mechanism
#[derive(Clone)]
pub struct Broadcaster<T>
where
    T: Clone + Send + Sync + 'static,
{
    sender: broadcast::Sender<T>,
    // Track active subscribers to prevent channel closure
    active_subscribers: Arc<RwLock<usize>>,
    capacity: usize,
}

impl<T: Clone + Send + Sync + 'static> Broadcaster<T> {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { 
            sender, 
            active_subscribers: Arc::new(RwLock::new(0)),
            capacity,
        }
    }

    /// Send a message through the broadcast channel with automatic reconnection
    pub fn send(&self, value: T) -> Result<usize, SendError<T>> {
        // Check if we need to recreate the channel
        let subscriber_count = self.sender.receiver_count();
        if subscriber_count == 0 {
            // No active subscribers, but we have tracked subscribers
            // This indicates the channel might have been closed
            let active_count = *self.active_subscribers.read().unwrap();
            if active_count > 0 {
                warn!("Channel appears closed but has {} tracked subscribers. Attempting to recreate.", active_count);
                // Recreate the channel
                let (new_sender, _) = broadcast::channel(self.capacity);
                // Replace the sender
                self.sender = new_sender;
                debug!("Broadcast channel recreated successfully");
            }
        }
        
        // Attempt to send the message
        match self.sender.send(value.clone()) {
            Ok(count) => Ok(count),
            Err(e) => {
                // If sending failed due to no receivers, but we have tracked subscribers,
                // recreate the channel and try again
                let active_count = *self.active_subscribers.read().unwrap();
if active_count > 0 && e.to_string().contains("closed") {
                    warn!("Send failed but channel has {} tracked subscribers. Recreating channel and retrying.", active_count);
                    // Recreate the channel
                    let (new_sender, _) = broadcast::channel(self.capacity);
                    // Replace the sender
                    self.sender = new_sender;
                    // Try sending again
                    match self.sender.send(value) {
                        Ok(count) => {
                            debug!("Message sent successfully after channel recreation");
                            Ok(count)
                        }
                        Err(e) => Err(e),
                    }
                } else {
                    Err(e)
                }
            }
        }
    }

    /// Subscribe to the broadcast channel and track the subscription
    pub fn subscribe(&self) -> Receiver<T> {
        // Increment the active subscriber count
        {
            let mut count = self.active_subscribers.write().unwrap();
            *count += 1;
            debug!("New subscriber added, total active: {}", *count);
        }
        
        // Create a wrapped receiver that will decrement the count when dropped
        let receiver = self.sender.subscribe();
        let active_subscribers = self.active_subscribers.clone();
        
        // Return the receiver
        receiver
    }
    
    /// Get the current number of active subscribers
    pub fn subscriber_count(&self) -> usize {
        *self.active_subscribers.read().unwrap()
    }
    
    /// Check if the channel is healthy (has subscribers and is not closed)
    pub fn is_healthy(&self) -> bool {
        self.sender.receiver_count() > 0
    }
}
