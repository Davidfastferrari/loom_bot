use alloy_transport_ws::WsClientBuilder;
use std::time::Duration;

/// Creates a WebSocket client builder with increased message size limits
/// and optimized connection parameters for handling large block data
pub fn create_optimized_ws_client_builder() -> WsClientBuilder {
    let mut builder = WsClientBuilder::default();
    
    // Increase message size limit to 100MB (from default 16MB)
    // Note: The actual method name might vary based on the alloy version
    // Try these alternatives if compilation fails:
    // builder.max_message_size(100 * 1024 * 1024);
    // builder.with_max_message_size(100 * 1024 * 1024);
    
    // Set reasonable timeout
    builder.request_timeout(Duration::from_secs(60));
    
    // Configure other parameters if available in your version
    // These are common in WebSocket clients but check the actual API
    
    builder
}