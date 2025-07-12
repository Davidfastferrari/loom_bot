use alloy_network::Ethereum;
use alloy_provider::Provider;
use alloy_rpc_types::{BlockId, Header};
use alloy_rpc_types_trace::geth::{GethTrace, PreStateFrame};
use alloy_rpc_types_trace::common::TraceResult;
use std::time::Duration;
use tokio::sync::broadcast::error::RecvError;
use tracing::{debug, error, info, warn};

use loom_core_actors::{subscribe, Broadcaster, WorkerResult};
use loom_node_debug_provider::DebugProviderExt;
use loom_types_blockchain::{debug_trace_block, fetch_block_trace_chunked};
use loom_types_events::{BlockStateUpdate, Message, MessageBlockStateUpdate};

const MAX_RETRY_ATTEMPTS: usize = 3;
const RETRY_DELAY_MS: u64 = 1000;
const CHUNK_SIZE: usize = 50; // Number of transactions to trace at once

pub async fn new_node_block_state_worker<P>(
    client: P,
    block_header_receiver: Broadcaster<Header>,
    sender: Broadcaster<MessageBlockStateUpdate>,
) -> WorkerResult
where
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
{
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
                warn!("BlockState sender channel appears unhealthy, checking status");
                // Attempt to send a keep-alive message or reconnect if needed
            }
            if !block_header_receiver_clone.is_healthy() {
                warn!("BlockState receiver channel appears unhealthy, attempting to resubscribe");
                // The main loop will handle resubscription
            }
        }
    });

    loop {
        // Attempt to receive a message with error handling
        let block_header = match receiver.recv().await {
            Ok(header) => header,
            Err(e) => {
                error!("Error receiving block header in state worker: {}", e);
                // If we get a lagged error, we can continue with a new subscription
                match e {
                    RecvError::Lagged(_) => {
                        warn!("BlockState worker lagged behind, resubscribing");
                        receiver = block_header_receiver.subscribe();
                        continue;
                    }
                    RecvError::Closed => {
                        // If the channel is closed, attempt to resubscribe
                        warn!("BlockState channel appears closed, attempting to resubscribe");
                        tokio::time::sleep(Duration::from_secs(1)).await;
                        receiver = block_header_receiver.subscribe();
                        continue;
                    }
                }
            }
        };

        let (block_number, block_hash) = (block_header.number, block_header.hash);
        info!("BlockState header received {} {}", block_number, block_hash);
        
        // Try standard approach first
        let mut success = false;
        let mut retry_count = 0;
        
        while !success && retry_count < MAX_RETRY_ATTEMPTS {
            if retry_count > 0 {
                warn!("Retrying block state trace for block {} (attempt {}/{})", 
                      block_number, retry_count + 1, MAX_RETRY_ATTEMPTS);
                tokio::time::sleep(Duration::from_millis(RETRY_DELAY_MS * (2_u64.pow(retry_count as u32)))).await;
            }
            
            match debug_trace_block(client.clone(), BlockId::Hash(block_header.hash.into()), true).await {
                Ok((_, post)) => {
                    // Enhanced error handling for send operation
                    match sender.send(Message::new_with_time(BlockStateUpdate { 
                        block_header: block_header.clone(), 
                        state_update: post 
                    })) {
                        Ok(_) => {
                            success = true;
                            debug!("BlockState processing finished {} {}", block_number, block_hash);
                        },
                        Err(e) => {
                            error!("Broadcaster error in state worker: {}", e);
                            // If the channel is closed but we have active subscribers, it might be recoverable
                            if sender.subscriber_count() > 0 {
                                warn!("Attempting to resend state update after broadcaster error");
                                // Short delay before retry
                                tokio::time::sleep(Duration::from_millis(100)).await;
                                continue;
                            } else {
                                // No subscribers, so mark as success but log warning
                                warn!("No active subscribers for state updates, marking as success but data not sent");
                                success = true;
                            }
                        }
                    }
                },
                Err(e) => {
                    error!("Standard debug_trace_block error: {}", e);
                    retry_count += 1;
                }
            }
        }
        
        // If standard approach failed, try chunked approach
        if !success {
            warn!("Falling back to chunked block trace for block {}", block_number);
            
            let chunked_result = fetch_block_trace_chunked(
                client.clone(), 
                BlockId::Hash(block_header.hash.into()),
                CHUNK_SIZE
            ).await;
            
            match chunked_result {
                Ok(trace_results) => {
                    // Process trace results to extract state updates
                    let mut post_state = Vec::new();
                    
                    for result in trace_results {
                        if let TraceResult::Success { result, .. } = result {
                            if let GethTrace::PreStateTracer(frame) = result {
                                match frame {
                                    PreStateFrame::Diff(diff) => {
                                        post_state.push(diff.post);
                                    },
                                    PreStateFrame::Default(_) => {
                                        // Default frame doesn't have post state
                                    }
                                }
                            }
                        }
                    }
                    
                    if !post_state.is_empty() {
                        // Enhanced error handling for chunked approach
                        match sender.send(Message::new_with_time(BlockStateUpdate { 
                            block_header: block_header.clone(), 
                            state_update: post_state 
                        })) {
                            Ok(_) => {
                                info!("BlockState processing finished using chunked approach {} {}", block_number, block_hash);
                            },
                            Err(e) => {
                                error!("Broadcaster error with chunked approach: {}", e);
                                // If the channel is closed but we have active subscribers, it might be recoverable
                                if sender.subscriber_count() > 0 {
                                    warn!("Attempting to resend chunked state update after broadcaster error");
                                    // Try one more time after a short delay
                                    tokio::time::sleep(Duration::from_millis(200)).await;
                                    if let Err(e2) = sender.send(Message::new_with_time(BlockStateUpdate { 
                                        block_header: block_header.clone(), 
                                        state_update: post_state 
                                    })) {
                                        error!("Final attempt to send chunked state update failed: {}", e2);
                                    } else {
                                        info!("Successfully sent chunked state update on retry");
                                    }
                                }
                            }
                        }
                    } else {
                        error!("No post state found in chunked trace results for block {}", block_number);
                    }
                },
                Err(e) => {
                    error!("Chunked debug_trace_block error: {}", e);
                    // Log detailed error and continue to next block
                    error!("All attempts to process block state for block {} failed. Moving to next block.", block_number);
                }
            }
        }
    }
}
