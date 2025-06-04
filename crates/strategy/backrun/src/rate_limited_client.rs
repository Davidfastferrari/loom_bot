use async_trait::async_trait;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::{Mutex, Semaphore};
use futures_util::future::BoxFuture;
use futures_util::FutureExt;
use std::borrow::Cow;
use alloy_provider::{Provider, RootProvider};
use alloy::rpc::json_rpc::{RpcRecv, RpcSend};
use alloy_transport::TransportResult;

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

#[async_trait]
impl<P, N> Provider<N> for RateLimitedClient<P>
where
    P: Provider<N> + Clone + Send + Sync + 'static,
    N: alloy_provider::Network,
{
    fn root(&self) -> &RootProvider<N> {
        self.inner.root()
    }

    async fn raw_request<P2, R>(&self, method: Cow<'static, str>, params: P2) -> TransportResult<R>
    where
        P2: RpcSend + Send + Sync + 'static,
        R: RpcRecv + Send + Sync + 'static,
    {
        self.wait_for_rate_limit().await;
        self.inner.raw_request(method, params).await
    }
}

use loom_node_debug_provider::DebugProviderExt;
use alloy::eips::BlockId;
use alloy::rpc::types::trace::geth::{GethDebugTracingCallOptions, GethDebugTracingOptions, GethTrace, TraceResult};
use alloy::rpc::types::{BlockNumberOrTag, TransactionRequest};
use alloy::primitives::BlockHash;
use alloy_transport::TransportResult;

#[async_trait]
impl<P, N> DebugProviderExt<N> for RateLimitedClient<P>
where
    P: DebugProviderExt<N> + Provider<N> + Clone + Send + Sync + 'static,
    N: alloy_provider::Network,
{
    async fn geth_debug_trace_call(
        &self,
        tx: TransactionRequest,
        block: BlockId,
        trace_options: GethDebugTracingCallOptions,
    ) -> TransportResult<GethTrace> {
        self.wait_for_rate_limit().await;
        self.inner.geth_debug_trace_call(tx, block, trace_options).await
    }

    async fn geth_debug_trace_block_by_number(
        &self,
        block: BlockNumberOrTag,
        trace_options: GethDebugTracingOptions,
    ) -> TransportResult<Vec<TraceResult>> {
        self.wait_for_rate_limit().await;
        self.inner.geth_debug_trace_block_by_number(block, trace_options).await
    }

    async fn geth_debug_trace_block_by_hash(
        &self,
        block: BlockHash,
        trace_options: GethDebugTracingOptions,
    ) -> TransportResult<Vec<TraceResult>> {
        self.wait_for_rate_limit().await;
        self.inner.geth_debug_trace_block_by_hash(block, trace_options).await
    }
}
