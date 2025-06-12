use alloy_network::Ethereum;
use alloy_provider::Provider;
use alloy_rpc_types::{BlockId, Header};
use alloy_rpc_types_trace::common::TraceResult;
use alloy_rpc_types_trace::geth::{GethTrace, PreStateFrame};
use std::time::Duration;
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
    subscribe!(block_header_receiver);

    loop {
        if let Ok(block_header) = block_header_receiver.recv().await {
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
                        if let Err(e) = sender.send(Message::new_with_time(BlockStateUpdate { 
                            block_header: block_header.clone(), 
                            state_update: post 
                        })) {
                            error!("Broadcaster error {}", e);
                        } else {
                            success = true;
                            debug!("BlockState processing finished {} {}", block_number, block_hash);
                        }
                    }
                    Err(e) => {
                        error!("Standard debug_trace_block error: {}", e);
                        retry_count += 1;
                    }
                }
            }
            
            // If standard approach failed, try chunked approach
            if !success {
                warn!("Falling back to chunked block trace for block {}", block_number);
                
                match fetch_block_trace_chunked(
                    client.clone(), 
                    BlockId::Hash(block_header.hash.into()),
                    CHUNK_SIZE
                ).await {
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
                            if let Err(e) = sender.send(Message::new_with_time(BlockStateUpdate { 
                                block_header: block_header.clone(), 
                                state_update: post_state 
                            })) {
                                error!("Broadcaster error with chunked approach: {}", e);
                            } else {
                                info!("BlockState processing finished using chunked approach {} {}", block_number, block_hash);
                            }
                        } else {
                            error!("No post state found in chunked trace results for block {}", block_number);
                        }
                    }
                    Err(e) => {
                        error!("Chunked debug_trace_block error: {}", e);
                    }
                }
            }
        }
    }
}
