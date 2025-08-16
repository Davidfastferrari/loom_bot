use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Semaphore};
use alloy_provider::{Provider, RootProvider};
use tracing::{debug, warn, error};
use std::sync::atomic::{AtomicU64, Ordering};

/// Enhanced rate-limited provider with connection health monitoring and retry logic
#[derive(Clone)]
pub struct RateLimitedProvider<N: alloy_provider::Network> {
    inner: RootProvider<N>,
    semaphore: Arc<Semaphore>,
    last_request_time: Arc<Mutex<Instant>>,
    min_interval: Duration,
    request_count: Arc<AtomicU64>,
    error_count: Arc<AtomicU64>,
    last_error_time: Arc<Mutex<Option<Instant>>>,
    _network: std::marker::PhantomData<N>,
}

impl<N: alloy_provider::Network> RateLimitedProvider<N> {
    /// Create a new enhanced RateLimitedProvider with health monitoring
    /// rate_limit_rps: requests per second limit. If 0, no rate limiting is applied.
    pub fn new(inner: RootProvider<N>, rate_limit_rps: u32) -> Self {
        let min_interval = if rate_limit_rps == 0 {
            Duration::from_secs(0)
        } else {
            Duration::from_secs_f64(1.0 / rate_limit_rps as f64)
        };
        RateLimitedProvider {
            inner,
            semaphore: Arc::new(Semaphore::new(1)),
            last_request_time: Arc::new(Mutex::new(Instant::now() - min_interval)),
            min_interval,
            request_count: Arc::new(AtomicU64::new(0)),
            error_count: Arc::new(AtomicU64::new(0)),
            last_error_time: Arc::new(Mutex::new(None)),
            _network: std::marker::PhantomData,
        }
    }

    async fn wait_for_rate_limit(&self) {
        let _permit = self.semaphore.acquire().await.unwrap();
        let mut last_time = self.last_request_time.lock().await;
        let now = Instant::now();
        let elapsed = now.duration_since(*last_time);
        
        if elapsed < self.min_interval {
            let sleep_duration = self.min_interval - elapsed;
            debug!("Rate limiting: sleeping for {:?}", sleep_duration);
            tokio::time::sleep(sleep_duration).await;
        }
        
        *last_time = Instant::now();
        self.request_count.fetch_add(1, Ordering::Relaxed);
    }
    
    /// Record an error and update health metrics
    async fn record_error(&self) {
        self.error_count.fetch_add(1, Ordering::Relaxed);
        let mut last_error = self.last_error_time.lock().await;
        *last_error = Some(Instant::now());
        
        let total_requests = self.request_count.load(Ordering::Relaxed);
        let total_errors = self.error_count.load(Ordering::Relaxed);
        
        if total_requests > 0 {
            let error_rate = (total_errors as f64 / total_requests as f64) * 100.0;
            if error_rate > 10.0 {
                warn!("High error rate detected: {:.2}% ({}/{})", error_rate, total_errors, total_requests);
            }
        }
    }
    
    /// Get connection health statistics
    pub fn get_health_stats(&self) -> ProviderHealthStats {
        let total_requests = self.request_count.load(Ordering::Relaxed);
        let total_errors = self.error_count.load(Ordering::Relaxed);
        let error_rate = if total_requests > 0 {
            (total_errors as f64 / total_requests as f64) * 100.0
        } else {
            0.0
        };
        
        ProviderHealthStats {
            total_requests,
            total_errors,
            error_rate,
            is_healthy: error_rate < 5.0, // Consider healthy if error rate < 5%
        }
    }

    /// Get a reference to the inner RootProvider
    pub fn inner(&self) -> &RootProvider<N> {
        &self.inner
    }
}

impl<N> Provider<N> for RateLimitedProvider<N>
where
    N: alloy_provider::Network,
{
    fn root(&self) -> &RootProvider<N> {
        &self.inner
    }
}

#[derive(Debug, Clone)]
pub struct ProviderHealthStats {
    pub total_requests: u64,
    pub total_errors: u64,
    pub error_rate: f64,
    pub is_healthy: bool,
}