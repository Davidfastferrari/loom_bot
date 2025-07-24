use alloy_transport_ws::WsConnect;

pub mod rate_limited_provider;
pub use rate_limited_provider::RateLimitedProvider;

/// Creates a WebSocket connection with optimized parameters for handling large block data
pub fn create_optimized_ws_connect(url: &str) -> WsConnect {
    let ws_connect = WsConnect::new(url);
    // If your version supports it, set message size and timeout here:
    // ws_connect = ws_connect.max_message_size(100 * 1024 * 1024);
    // ws_connect = ws_connect.request_timeout(Duration::from_secs(60));
    ws_connect
}
