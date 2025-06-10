use std::time::Duration;
use tracing::{info, warn};

/// Configures DNS resolution settings for the application
pub fn configure_dns_settings() {
    // Set DNS timeout environment variables if not already set
    if std::env::var("RESOLVE_TIMEOUT_MS").is_err() {
        std::env::set_var("RESOLVE_TIMEOUT_MS", "10000");
        info!("Set DNS resolution timeout to 10 seconds");
    }
    
    // Set DNS cache TTL if not already set
    if std::env::var("RESOLVE_CACHE_TTL_SECONDS").is_err() {
        std::env::set_var("RESOLVE_CACHE_TTL_SECONDS", "300");
        info!("Set DNS cache TTL to 300 seconds");
    }
    
    // Configure DNS settings for reqwest
    std::env::set_var("REQWEST_CONNECT_TIMEOUT", "30000");
    std::env::set_var("REQWEST_TIMEOUT", "60000");
    
    // Log DNS configuration
    warn!("DNS resolution settings configured");
}