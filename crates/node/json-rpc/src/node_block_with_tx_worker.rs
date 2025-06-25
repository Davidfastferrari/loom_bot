use alloy_network::{primitives::HeaderResponse, Ethereum};
use alloy_provider::Provider;
use alloy_rpc_types::{BlockId, BlockTransactionsKind, Header};
use loom_core_actors::{subscribe, Broadcaster, WorkerResult};
use loom_types_blockchain::fetch_block_with_transactions_chunked;
use loom_types_events::{BlockUpdate, Message, MessageBlock};
use std::time::Duration;
use tracing::{debug, error, info, warn};

// Constants for chunked fetching
const MAX_TX_PER_REQUEST: usize = 50;
const MAX_RETRY_ATTEMPTS: usize = 3;

pub async fn new_block_with_tx_worker<P>(
    client: P,
    block_header_receiver: Broadcaster<Header>,
    sender: Broadcaster<MessageBlock>,
) -> WorkerResult
where
    P: Provider<Ethereum> + Send + Sync + Clone + 'static,
{
    use alloy_rpc_types::{BlockTransactionsKind, BlockTransactions, Block};
    subscribe!(block_header_receiver);

    loop {
        if let Ok(block_header) = block_header_receiver.recv().await {
            let (block_number, block_hash) = (block_header.number, block_header.hash);
            info!("BlockWithTx header received {} {}", block_number, block_hash);

            let mut success = false;
            let mut retry_count = 0;
            
            // Try standard approach first
            while !success && retry_count < MAX_RETRY_ATTEMPTS {
                if retry_count > 0 {
                    let backoff = 100 * (2_u64.pow(retry_count as u32));
                    warn!("Retrying block fetch for block {} (attempt {}/{}) after {}ms", 
                          block_number, retry_count + 1, MAX_RETRY_ATTEMPTS, backoff);
                    tokio::time::sleep(Duration::from_millis(backoff)).await;
                }
                
                let fetch_result = client.get_block_by_hash(block_header.hash(), BlockTransactionsKind::Full).await;
                match fetch_result {
                    Ok(Some(full_block)) => {
                        if let Err(e) = sender.send(Message::new_with_time(BlockUpdate { block: full_block })) {
                            error!("Broadcaster error {}", e);
                        } else {
                            success = true;
                            debug!("BlockWithTx processing finished {} {}", block_number, block_hash);
                        }
                        break;
                    }
                    Ok(None) => {
                        error!("Block data is empty for block {}", block_number);
                        retry_count += 1;
                    }
                    Err(e) => {
                        // Check if error is deserialization error and handle gracefully
                        let err_msg = e.to_string();
                        if err_msg.contains("deserialization error") || err_msg.contains("unknown variant") {
                            error!("Deserialization error fetching full block data for block {}: {}", block_number, err_msg);
                            // Skip this block or continue retrying based on policy
                            // Here, we skip retrying to avoid blocking
                            break;
                        } else {
                            error!("Error fetching full block data: {}", e);
                            retry_count += 1;
                        }
                    }
                }
            }
            
            // If standard approach failed, try chunked approach
            if !success {
                warn!("Falling back to chunked block fetch for block {}", block_number);
                
                let chunked_result = fetch_block_with_transactions_chunked(
                    client.clone(),
                    BlockId::Hash(block_header.hash().into()),
                    MAX_TX_PER_REQUEST
                ).await;
                
                match chunked_result {
                    Ok((header, transactions)) => {
                        // Construct a Block from the header and transactions
                        let block = Block {
                            header,
                            transactions: BlockTransactions::Full(transactions),
                            withdrawals: None,
                            uncles: vec![],
                        };
                        
                        if let Err(e) = sender.send(Message::new_with_time(BlockUpdate { block })) {
                            error!("Broadcaster error with chunked approach: {}", e);
                        } else {
                            info!("BlockWithTx processing finished using chunked approach {} {}", block_number, block_hash);
                        }
                    }
                    Err(e) => {
                        let err_msg = e.to_string();
                        if err_msg.contains("deserialization error") || err_msg.contains("unknown variant") {
                            error!("Deserialization error in chunked block fetch for block {}: {}", block_number, err_msg);
                            // Consider skipping or marking block as failed without panicking
                        } else {
                            error!("Chunked block fetch failed for block {}: {}", block_number, e);
                        }
                    }
                }
            }
        }
    }
}
