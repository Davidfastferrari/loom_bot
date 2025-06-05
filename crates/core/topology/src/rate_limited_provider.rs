use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Semaphore};
use alloy_rpc_client::RpcClient;
use alloy_transport::Transport;

/// A wrapper around an RpcClient that enforces a rate limit on requests per second.
#[derive(Clone)]
pub struct RateLimitedProvider {
    inner: RpcClient,
    semaphore: Arc<Semaphore>,
    last_request_time: Arc<Mutex<Instant>>,
    min_interval: Duration,
}

impl RateLimitedProvider
{
    /// Create a new RateLimitedProvider wrapping the given RpcClient.
    /// rate_limit_rps: requests per second limit. If 0, no rate limiting is applied.
    pub fn new(inner: RpcClient, rate_limit_rps: u32) -> Self {
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

    /// Get a reference to the inner RpcClient
    pub fn inner(&self) -> &RpcClient {
        &self.inner
    }
}

use alloy_provider::Provider;
use futures::future::BoxFuture;
use futures::FutureExt;
use serde_json::Value;

impl Provider for RateLimitedProvider
{
    type Future<R> = BoxFuture<'static, Result<R, anyhow::Error>>;

    fn request<R>(&self, method: &str, params: Value) -> Self::Future<R>
    where
        R: serde::de::DeserializeOwned + Send + 'static,
    {
        let inner = self.inner.clone();
        let method = method.to_string();
        let params = params.clone();
        let this = self.clone();

        async move {
            this.wait_for_rate_limit().await;
            inner.request(&method, params).await.map_err(|e| anyhow::anyhow!(e))
        }
        .boxed()
    }
}
