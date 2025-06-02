use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Semaphore};
use alloy_transport::{Transport, TransportResult};
use alloy_rpc_client::{ClientBuilder, RpcClient};
use futures::future::BoxFuture;
use futures::FutureExt;
use serde_json::Value;

/// A wrapper around a Transport that enforces a rate limit on requests per second.
#[derive(Clone)]
pub struct RateLimitedProvider<T> {
    inner: T,
    semaphore: Arc<Semaphore>,
    last_request_time: Arc<Mutex<Instant>>,
    min_interval: Duration,
}

impl<T> RateLimitedProvider<T> {
    /// Create a new RateLimitedProvider wrapping the given transport.
    /// rate_limit_rps: requests per second limit. If 0, no rate limiting is applied.
    pub fn new(inner: T, rate_limit_rps: u32) -> Self {
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
}

impl<T> Transport for RateLimitedProvider<T>
where
    T: Transport + Clone + Send + Sync + 'static,
{
    type Error = T::Error;

    fn prepare(&self, method: &str, params: &[Value]) -> TransportResult<(String, Value)> {
        self.inner.prepare(method, params)
    }

    async fn request(&self, req: Value) -> Result<Value, Self::Error> {
        self.wait_for_rate_limit().await;
        self.inner.request(req).await
    }
}
