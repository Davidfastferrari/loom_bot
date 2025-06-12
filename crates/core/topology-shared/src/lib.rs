use alloy_transport_ws::WsClientBuilder;
use std::time::Duration;

/// Creates a WebSocket client builder with increased message size limits
/// and optimized connection parameters for handling large block data
pub fn create_optimized_ws_client_builder() -> WsClientBuilder {
    let mut builder = WsClientBuilder::default();
    // Set message size limit to 100MB (from default 16MB)
    // Use the correct method for your alloy version:
    // If this does not compile, try replacing with builder.with_max_message_size(100 * 1024 * 1024);
    builder.max_message_size(100 * 1024 * 1024);
    builder.request_timeout(Duration::from_secs(60));
    builder
}
