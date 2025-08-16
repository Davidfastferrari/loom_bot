use eyre::{eyre, Result};
use std::sync::{Arc, RwLock};
use tokio::sync::broadcast;
use tokio::sync::broadcast::error::{RecvError, SendError};
use tokio::sync::broadcast::Receiver;
use tracing::{debug, error, warn};

/// A wrapper around Receiver that tracks active subscribers
pub struct TrackedReceiver<T> {
    receiver: Receiver<T>,
    active_subscribers: Arc<RwLock<usize>>,
}

impl<T: Clone> TrackedReceiver<T> {
    pub async fn recv(&mut self) -> Result<T, RecvError> {
        self.receiver.recv().await
    }
    
    pub fn try_recv(&mut self) -> Result<T, tokio::sync::broadcast::error::TryRecvError> {
        self.receiver.try_recv()
    }
}

impl<T> Drop for TrackedReceiver<T> {
    fn drop(&mut self) {
        let mut count = self.active_subscribers.write().unwrap();
        if *count > 0 {
            *count -= 1;
            debug!("Subscriber dropped, remaining active: {}", *count);
        }
    }
}

/// Enhanced Broadcaster with reconnection capability and keep-alive mechanism
#[derive(Clone)]
pub struct Broadcaster<T>
where
    T: Clone + Send + Sync + 'static,
{
    sender: Arc<RwLock<broadcast::Sender<T>>>,
    // Track active subscribers to prevent channel closure
    active_subscribers: Arc<RwLock<usize>>,
    capacity: usize,
}

impl<T: Clone + Send + Sync + 'static> Broadcaster<T> {
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);
        Self { 
            sender: Arc::new(RwLock::new(sender)),
            active_subscribers: Arc::new(RwLock::new(0)),
            capacity,
        }
    }

    /// Send a message through the broadcast channel with automatic reconnection
    pub fn send(&self, value: T) -> Result<usize, SendError<T>> {
        // Check if we need to recreate the channel
        let subscriber_count = self.sender.read().unwrap().receiver_count();
        if subscriber_count == 0 {
            // No active subscribers, but we have tracked subscribers
            // This indicates the channel might have been closed
            let active_count = *self.active_subscribers.read().unwrap();
            if active_count > 0 {
                warn!("Channel appears closed but has {} tracked subscribers. Attempting to recreate.", active_count);
                // Recreate the channel
                let (new_sender, _) = broadcast::channel(self.capacity);
                // Replace the sender
                *self.sender.write().unwrap() = new_sender;
                debug!("Broadcast channel recreated successfully");
            }
        }
        
        // Attempt to send the message
        match self.sender.read().unwrap().send(value.clone()) {
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
                    *self.sender.write().unwrap() = new_sender;
                    // Try sending again
                    match self.sender.read().unwrap().send(value) {
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
    pub fn subscribe(&self) -> TrackedReceiver<T> {
        // Increment the active subscriber count
        {
            let mut count = self.active_subscribers.write().unwrap();
            *count += 1;
            debug!("New subscriber added, total active: {}", *count);
        }
        
        // Create a wrapped receiver that will decrement the count when dropped
        let receiver = self.sender.read().unwrap().subscribe();
        let active_subscribers = self.active_subscribers.clone();
        
        // Return a tracked receiver that decrements count on drop
        TrackedReceiver {
            receiver,
            active_subscribers,
        }
    }
    
    /// Get the current number of active subscribers
    pub fn subscriber_count(&self) -> usize {
        *self.active_subscribers.read().unwrap()
    }
    
    /// Check if the channel is healthy (has subscribers and is not closed)
    pub fn is_healthy(&self) -> bool {
        self.sender.read().unwrap().receiver_count() > 0
    }
}
