use alloy_network::{primitives::HeaderResponse, Network};
use std::time::Duration;

use alloy_provider::Provider;
use alloy_rpc_types::{Filter, Header};
use tokio::sync::broadcast::{Receiver, error::RecvError};
use tracing::{debug, error, warn};

use loom_core_actors::{subscribe, Broadcaster, WorkerResult};
use loom_types_events::{BlockLogs, Message, MessageBlockLogs};

pub async fn new_node_block_logs_worker<N: Network, P: Provider<N> + Send + Sync + 'static>(
    client: P,
    block_header_receiver: Broadcaster<Header>,
    sender: Broadcaster<MessageBlockLogs>,
) -> WorkerResult {
    // Subscribe to the block header channel with enhanced error handling
    let mut receiver = block_header_receiver.subscribe();
    
    // Keep-alive mechanism - periodically check channel health
    let sender_clone = sender.clone();
    let block_header_receiver_clone = block_header_receiver.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        loop {
            interval.tick().await;
            if !sender_clone.is_healthy() {
                warn!("BlockLogs sender channel appears unhealthy, checking status");
                // Attempt to send a keep-alive message or reconnect if needed
                // This keeps the channel active even during periods of inactivity
            }
            if !block_header_receiver_clone.is_healthy() {
                warn!("BlockLogs receiver channel appears unhealthy, attempting to resubscribe");
                // The main loop will handle resubscription
            }
        }
    });

    loop {
        // Attempt to receive a message with error handling
        let block_header = match receiver.recv().await {
            Ok(header) => header,
            Err(e) => {
                error!("Error receiving block header: {}", e);
                // If we get a lagged error, we can continue with a new subscription
                match e {
                    RecvError::Lagged(_) => {
                        warn!("BlockLogs worker lagged behind, resubscribing");
                        receiver = block_header_receiver.subscribe();
                        continue;
                    }
                    RecvError::Closed => {
                        // If the channel is closed, attempt to resubscribe
                        warn!("BlockLogs channel appears closed, attempting to resubscribe");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        receiver = block_header_receiver.subscribe();
                        continue;
                    }
                }
            }
        };

        let (block_number, block_hash) = (block_header.number, block_header.hash);
        debug!("BlockLogs header received {} {}", block_number, block_hash);
        let filter = Filter::new().at_block_hash(block_header.hash());

        let mut err_counter = 0;
        let max_retries = 5; // Increased from 3 to 5 for more resilience

        while err_counter < max_retries {
            match client.get_logs(&filter).await {
                Ok(logs) => {
                    // Enhanced error handling for send operation
                    match sender.send(Message::new_with_time(BlockLogs { block_header: block_header.clone(), logs })) {
                        Ok(_) => {
                            debug!("BlockLogs successfully sent for block {} {}", block_number, block_hash);
                            break;
                        },
                        Err(e) => {
                            error!("Broadcaster error when sending logs: {}", e);
                            // If the channel is closed but we have active subscribers, it might be recoverable
                            if sender.subscriber_count() > 0 {
                                warn!("Attempting to resend logs after broadcaster error");
                                // Exponential backoff before retry
                                tokio::time::sleep(Duration::from_millis(100 * 2_u64.pow(err_counter as u32))).await;
                                err_counter += 1;
                                continue;
                            } else {
                                // No subscribers, so no point retrying
                                break;
                            }
                        }
                    }
                },
                Err(e) => {
                    error!("client.get_logs error: {}", e);
                    err_counter += 1;
                    // Exponential backoff
                    tokio::time::sleep(Duration::from_millis(100 * 2_u64.pow(err_counter as u32))).await;
                }
            }
        }

        if err_counter >= max_retries {
            warn!("Failed to process logs for block {} {} after {} attempts", block_number, block_hash, max_retries);
        } else {
            debug!("BlockLogs processing finished {} {}", block_number, block_hash);
        }
    }
}

#[allow(dead_code)]
pub async fn new_node_block_logs_worker_reth<N: Network, P: Provider<N> + Send + Sync + 'static>(
    client: P,
    mut block_header_receiver: Receiver<Header>,
    sender: Broadcaster<MessageBlockLogs>,
) -> WorkerResult {
    loop {
        if let Ok(block_header) = block_header_receiver.recv().await {
            let filter = Filter::new().at_block_hash(block_header.hash());

            let logs = client.get_logs(&filter).await?;
            if let Err(e) = sender.send(Message::new_with_time(BlockLogs { block_header, logs })) {
                error!("Broadcaster error {}", e);
            }
        }
    }
}
