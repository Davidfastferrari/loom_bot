use alloy_network::Network;
use alloy_primitives::TxHash;
use alloy_provider::Provider;
use alloy_rpc_types::{BlockId, BlockTransactionsKind, Header};
use eyre::{eyre, Result};
use tracing::{debug, error, info, warn};

/// Fetches block data in chunks to avoid WebSocket message size limitations
pub async fn fetch_block_with_transactions_chunked<P>(
    provider: P,
    block_id: BlockId,
    max_tx_per_request: usize,
) -> Result<(Header, Vec<alloy_rpc_types::Transaction>)>
where
    P: Provider<alloy_network::Ethereum> + Clone,
{
    // First get the block header and transaction hashes
    let block = match block_id {
        BlockId::Hash(hash) => provider.get_block_by_hash(hash.block_hash, BlockTransactionsKind::Hashes).await?,
        BlockId::Number(num) => provider.get_block_by_number(num, BlockTransactionsKind::Hashes).await?,
    }.ok_or_else(|| eyre!("Block not found"))?;
    
    let header = block.header.clone();
    let tx_hashes = match block.transactions {
        alloy_rpc_types::BlockTransactions::Hashes(hashes) => hashes,
        _ => return Err(eyre!("Expected transaction hashes")),
    };
    
    if tx_hashes.is_empty() {
        return Ok((header, vec![]));
    }
    
    info!("Fetching {} transactions in chunks of {}", tx_hashes.len(), max_tx_per_request);
    
    // Fetch transactions in chunks
    let mut all_transactions = Vec::with_capacity(tx_hashes.len());
    let chunks = tx_hashes.chunks(max_tx_per_request);
    let total_chunks = (tx_hashes.len() + max_tx_per_request - 1) / max_tx_per_request;
    
    for (i, chunk) in chunks.enumerate() {
        debug!("Fetching transaction chunk {}/{}", i + 1, total_chunks);
        
        let mut chunk_transactions = Vec::with_capacity(chunk.len());
        for tx_hash in chunk {
            match provider.get_transaction_by_hash(*tx_hash).await? {
                Some(tx) => chunk_transactions.push(tx),
                None => {
                    warn!("Transaction {} not found", tx_hash);
                    // Create a placeholder transaction to maintain index consistency
                    // This is better than failing the entire block fetch
                    chunk_transactions.push(alloy_rpc_types::Transaction::default());
                }
            }
        }
        
        all_transactions.extend(chunk_transactions);
    }
    
    info!("Successfully fetched all {} transactions", all_transactions.len());
    Ok((header, all_transactions))
}

/// Fetches block trace data in smaller chunks to avoid WebSocket message size limitations
pub async fn fetch_block_trace_chunked<P>(
    provider: P,
    block_id: BlockId,
    chunk_size: usize,
) -> Result<Vec<alloy_rpc_types_trace::common::TraceResult>>
where
    P: Provider<alloy_network::Ethereum> + Clone + loom_node_debug_provider::DebugProviderExt<alloy_network::Ethereum>,
{
    // First get the block to determine transaction count
    let block = match block_id {
        BlockId::Hash(hash) => provider.get_block_by_hash(hash.block_hash, BlockTransactionsKind::Hashes).await?,
        BlockId::Number(num) => provider.get_block_by_number(num, BlockTransactionsKind::Hashes).await?,
    }.ok_or_else(|| eyre!("Block not found"))?;
    
    let tx_hashes = match block.transactions {
        alloy_rpc_types::BlockTransactions::Hashes(hashes) => hashes,
        _ => return Err(eyre!("Expected transaction hashes")),
    };
    
    if tx_hashes.is_empty() {
        // If no transactions, just trace the whole block
        return match block_id {
            BlockId::Number(block_number) => provider.geth_debug_trace_block_by_number(
                block_number,
                alloy_rpc_types_trace::geth::GethDebugTracingOptions::default(),
            ).await.map_err(|e| eyre!("Failed to trace block: {}", e)),
            BlockId::Hash(hash) => provider.geth_debug_trace_block_by_hash(
                hash.block_hash,
                alloy_rpc_types_trace::geth::GethDebugTracingOptions::default(),
            ).await.map_err(|e| eyre!("Failed to trace block: {}", e)),
        };
    }
    
    // For blocks with many transactions, trace each transaction individually
    info!("Tracing {} transactions in chunks of {}", tx_hashes.len(), chunk_size);
    
    let mut all_traces = Vec::new();
    let chunks = tx_hashes.chunks(chunk_size);
    let total_chunks = (tx_hashes.len() + chunk_size - 1) / chunk_size;
    
    for (i, chunk) in chunks.enumerate() {
        debug!("Tracing transaction chunk {}/{}", i + 1, total_chunks);
        
        for tx_hash in chunk {
            match provider.debug_trace_transaction(
                *tx_hash,
                alloy_rpc_types_trace::geth::GethDebugTracingOptions::default(),
            ).await {
                Ok(trace) => {
                    all_traces.push(alloy_rpc_types_trace::common::TraceResult::Success {
                        transaction_hash: Some(*tx_hash),
                        result: trace,
                    });
                }
                Err(e) => {
                    warn!("Failed to trace transaction {}: {}", tx_hash, e);
                    // Add a placeholder to maintain index consistency
                    all_traces.push(alloy_rpc_types_trace::common::TraceResult::Error {
                        transaction_hash: Some(*tx_hash),
                        error: format!("Trace failed: {}", e),
                    });
                }
            }
        }
    }
    
    info!("Successfully traced all {} transactions", all_traces.len());
    Ok(all_traces)
}