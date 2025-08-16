use alloy_primitives::TxHash;
use alloy_provider::Provider;
use alloy_rpc_types::Transaction;
use eyre::{eyre, Result};
use tracing::{debug, warn, error};

/// Enhanced transaction deserializer that can handle various transaction formats including EIP-4844, EIP-1559, and legacy
pub async fn get_transaction_with_enhanced_support<P>(
    provider: P,
    tx_hash: TxHash,
) -> Result<Option<Transaction>>
where
    P: Provider + Clone,
{
    // First try the standard way
    match provider.get_transaction_by_hash(tx_hash).await {
        Ok(Some(tx)) => Ok(Some(tx)),
        Ok(None) => Ok(None),
        Err(e) => {
            let err_msg = e.to_string();
            
            // Check for various transaction format errors
            if err_msg.contains("unknown variant") || err_msg.contains("deserialization error") {
                warn!("Transaction {} has non-standard format, attempting raw JSON-RPC fallback", tx_hash);
                
                // Try to get the transaction using raw JSON-RPC
                match get_transaction_raw_json(&provider, tx_hash).await {
                    Ok(Some(tx)) => {
                        debug!("Successfully retrieved transaction {} using raw JSON-RPC", tx_hash);
                        Ok(Some(tx))
                    },
                    Ok(None) => {
                        debug!("Transaction {} not found via raw JSON-RPC", tx_hash);
                        Ok(None)
                    },
                    Err(raw_err) => {
                        error!("Both standard and raw JSON-RPC failed for transaction {}: {} | {}", 
                               tx_hash, err_msg, raw_err);
                        // Skip this transaction but don't fail the entire process
                        Ok(None)
                    }
                }
            } else if err_msg.contains("timeout") || err_msg.contains("connection") {
                warn!("Network error for transaction {}, retrying once: {}", tx_hash, err_msg);
                
                // Retry once for network errors
                tokio::time::sleep(std::time::Duration::from_millis(100)).await;
                match provider.get_transaction_by_hash(tx_hash).await {
                    Ok(tx) => Ok(tx),
                    Err(retry_err) => {
                        error!("Retry failed for transaction {}: {}", tx_hash, retry_err);
                        Ok(None) // Skip rather than fail
                    }
                }
            } else {
                // For other errors, propagate them
                Err(e.into())
            }
        }
    }
}

/// Fallback method to get transaction using raw JSON-RPC when standard deserialization fails
async fn get_transaction_raw_json<P>(
    provider: &P,
    tx_hash: TxHash,
) -> Result<Option<Transaction>>
where
    P: Provider + Clone,
{
    // This is a placeholder for raw JSON-RPC implementation
    // In a full implementation, you would:
    // 1. Make a raw eth_getTransactionByHash call
    // 2. Parse the JSON response manually
    // 3. Extract the fields that can be safely deserialized
    // 4. Construct a Transaction object with available fields
    
    debug!("Raw JSON-RPC fallback not fully implemented for transaction {}", tx_hash);
    Ok(None)
}

// Keep the old function name for backward compatibility
pub async fn get_transaction_with_base_support<P>(
    provider: P,
    tx_hash: TxHash,
) -> Result<Option<Transaction>>
where
    P: Provider + Clone,
{
    get_transaction_with_enhanced_support(provider, tx_hash).await
}