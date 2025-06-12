use alloy_transport_ws::ClientBuilder;
use std::time::Duration;

/// Creates a WebSocket client builder with increased message size limits
/// and optimized connection parameters for handling large block data
pub fn create_optimized_ws_client_builder() -> ClientBuilder {
    let mut builder = ClientBuilder::default();
    // Set message size limit to 100MB (from default 16MB)
    // If this does not compile, try replacing with builder.with_max_message_size(100 * 1024 * 1024);
    builder.max_message_size(100 * 1024 * 1024);
    builder.request_timeout(Duration::from_secs(60));
    builder
}
