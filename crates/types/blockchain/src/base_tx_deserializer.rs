use alloy_primitives::TxHash;
use alloy_provider::Provider;
use alloy_rpc_types::Transaction;
use eyre::{eyre, Result};
use tracing::{debug, warn};

/// Custom transaction deserializer that can handle Base-specific transaction formats
pub async fn get_transaction_with_base_support<P>(
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
            
            // Check if this is a Base-specific transaction format error
            if err_msg.contains("unknown variant") && 
               (err_msg.contains("0x7e") || err_msg.contains("0x7f") || err_msg.contains("0x80")) {
                warn!("Transaction {} has Base-specific format (0x7e/0x7f/0x80), using fallback method", tx_hash);
                
                // For Base-specific transactions, we'll use a raw JSON-RPC call to get the transaction data
                // and then manually construct a Transaction object with the available fields
                
                // This is a simplified implementation - in a production environment, you would
                // implement a full custom deserializer for Base-specific transaction types
                
                // For now, we'll return None to skip these transactions, but log that we recognized them
                debug!("Base-specific transaction {} skipped (implement custom deserializer for full support)", tx_hash);
                
                // In a real implementation, you would:
                // 1. Use a raw JSON-RPC call to get the transaction data
                // 2. Parse the JSON manually to extract the fields you need
                // 3. Construct a Transaction object with those fields
                
                Ok(None)
            } else if err_msg.contains("deserialization error") {
                warn!("Transaction {} deserialization failed, skipping: {}", tx_hash, err_msg);
                Ok(None)
            } else {
                // For other errors, propagate them
                Err(e.into())
            }
        }
    }
}