use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Semaphore};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Semaphore};
use alloy_provider::{Provider, RootProvider};

/// A wrapper around a RootProvider that enforces a rate limit on requests per second.
#[derive(Clone)]
pub struct RateLimitedProvider<N: alloy_provider::Network> {
    inner: RootProvider<N>,
    semaphore: Arc<Semaphore>,
    last_request_time: Arc<Mutex<Instant>>,
    min_interval: Duration,
    _network: std::marker::PhantomData<N>,
}

impl<N: alloy_provider::Network> RateLimitedProvider<N> {
    /// Create a new RateLimitedProvider wrapping the given RootProvider.
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
            _network: std::marker::PhantomData,
        }
    }

    async fn wait_for_rate_limit(&self) {
        let _permit = self.semaphore.acquire().await.unwrap();
        let mut last_time = self.last_request_time.lock().await;
        let now = Instant::now();
        let elapsed = now.duration_since(*last_time);
        if elapsed < self.min_interval {
            tokio::time::sleep(self.min_interval - elapsed).await;
        }
        *last_time = Instant::now();
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
