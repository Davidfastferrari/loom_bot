use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Semaphore};
use alloy_provider::Provider;
use futures_util::future::BoxFuture;
use futures_util::FutureExt;
use serde_json::Value;

/// A wrapper around a Provider that enforces a rate limit on requests per second.
#[derive(Clone)]
pub struct RateLimitedClient<P> {
    inner: P,
    semaphore: Arc<Semaphore>,
    last_request_time: Arc<Mutex<Instant>>,
    min_interval: Duration,
}

impl<P> RateLimitedClient<P> {
    /// Create a new RateLimitedClient wrapping the given Provider.
    /// rate_limit_rps: requests per second limit. If 0, no rate limiting is applied.
    pub fn new(inner: P, rate_limit_rps: u32) -> Self {
        let min_interval = if rate_limit_rps == 0 {
            Duration::from_secs(0)
        } else {
            Duration::from_secs_f64(1.0 / rate_limit_rps as f64)
        };
        RateLimitedClient {
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

use std::borrow::Cow;
use alloy_provider::{Provider, RpcRecv, RpcSend, TransportResult, RootProvider};
use futures_util::future::BoxFuture;
use serde_json::Value;

impl<P, N> Provider<N> for RateLimitedClient<P>
where
    P: Provider<N> + Clone + Send + Sync + 'static,
    N: alloy_provider::Network,
{
    fn root(&self) -> &RootProvider<N> {
        self.inner.root()
    }

    fn raw_request<P2, R>(&self, method: Cow<'static, str>, params: P2) -> TransportResult<R>
    where
        P2: RpcSend,
        R: RpcRecv,
    {
        let inner = self.inner.clone();
        let this = self.clone();

        async move {
            this.wait_for_rate_limit().await;
            inner.raw_request(method, params).await
        }
    }
}
