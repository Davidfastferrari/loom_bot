use std::marker::PhantomData;

use alloy_network::Network;
use alloy_provider::Provider;
use eyre::ErrReport;
use influxdb::WriteQuery;
use revm::{Database, DatabaseCommit, DatabaseRef};
use tokio::task::JoinHandle;
use tracing::info;

use loom_core_actors::{Accessor, Actor, ActorResult, Broadcaster, Consumer, Producer, SharedState, WorkerResult};
use loom_core_actors_macros::{Accessor, Consumer, Producer};
use loom_node_debug_provider::DebugProviderExt;
use loom_types_blockchain::Mempool;
use loom_types_entities::{BlockHistory, LatestBlock, Market, MarketState};
use loom_types_events::{MarketEvents, MempoolEvents, MessageHealthEvent, MessageSwapCompose};

use super::{PendingTxStateChangeProcessorActor, StateChangeArbSearcherActor};
use crate::block_state_change_processor::BlockStateChangeProcessorActor;
use crate::BackrunConfig;
use crate::rate_limited_client::RateLimitedClient;

#[derive(Accessor, Consumer, Producer)]
pub struct StateChangeArbActor<P, N, DB: Clone + Send + Sync + 'static> {
    backrun_config: BackrunConfig,
    client: P,
    use_blocks: bool,
    use_mempool: bool,
    #[accessor]
    market: Option<SharedState<Market>>,
    #[accessor]
    mempool: Option<SharedState<Mempool>>,
    #[accessor]
    latest_block: Option<SharedState<LatestBlock>>,
    #[accessor]
    market_state: Option<SharedState<MarketState<DB>>>,
    #[accessor]
    block_history: Option<SharedState<BlockHistory<DB>>>,
    #[consumer]
    mempool_events_tx: Option<Broadcaster<MempoolEvents>>,
    #[consumer]
    market_events_tx: Option<Broadcaster<MarketEvents>>,
    #[producer]
    compose_channel_tx: Option<Broadcaster<MessageSwapCompose<DB>>>,
    #[producer]
    pool_health_monitor_tx: Option<Broadcaster<MessageHealthEvent>>,
    #[producer]
    influxdb_write_channel_tx: Option<Broadcaster<WriteQuery>>,

    _n: PhantomData<N>,
}

impl<P, N, DB> StateChangeArbActor<P, N, DB>
where
    N: Network,
    P: Provider<N> + DebugProviderExt<N> + Send + Sync + Clone + 'static,
    DB: DatabaseRef + Send + Sync + Clone + 'static,
{
    pub fn new(client: P, use_blocks: bool, use_mempool: bool, backrun_config: BackrunConfig) -> StateChangeArbActor<P, N, DB> {
        StateChangeArbActor {
            backrun_config,
            client,
            use_blocks,
            use_mempool,
            market: None,
            mempool: None,
            latest_block: None,
            block_history: None,
            market_state: None,
            mempool_events_tx: None,
            market_events_tx: None,
            compose_channel_tx: None,
            pool_health_monitor_tx: None,
            influxdb_write_channel_tx: None,
            _n: PhantomData,
        }
    }
}

impl<P, N, DB> Actor for StateChangeArbActor<P, N, DB>
where
    N: Network,
    P: Provider<N> + DebugProviderExt<N> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = ErrReport> + Database<Error = ErrReport> + DatabaseCommit + Send + Sync + Clone + Default + 'static,
{
    fn start(&self) -> ActorResult {
        let searcher_pool_update_channel = Broadcaster::new(100);
        let mut tasks: Vec<JoinHandle<WorkerResult>> = Vec::new();

        let mut state_update_searcher = StateChangeArbSearcherActor::new(self.backrun_config.clone());

        // Check required fields before unwrap
        let market = match &self.market {
            Some(m) => m.clone(),
            None => {
                return Err(eyre::eyre!("StateChangeArbActor: market is None"));
            }
        };
        let compose_channel_tx = match &self.compose_channel_tx {
            Some(tx) => tx.clone(),
            None => {
                return Err(eyre::eyre!("StateChangeArbActor: compose_channel_tx is None"));
            }
        };
        let pool_health_monitor_tx = match &self.pool_health_monitor_tx {
            Some(tx) => tx.clone(),
            None => {
                return Err(eyre::eyre!("StateChangeArbActor: pool_health_monitor_tx is None"));
            }
        };
        let influxdb_write_channel_tx = match &self.influxdb_write_channel_tx {
            Some(tx) => tx.clone(),
            None => {
                return Err(eyre::eyre!("StateChangeArbActor: influxdb_write_channel_tx is None"));
            }
        };

        match state_update_searcher
            .access(market)
            .consume(searcher_pool_update_channel.clone())
            .produce(compose_channel_tx)
            .produce(pool_health_monitor_tx)
            .produce(influxdb_write_channel_tx)
            .start()
        {
            Err(e) => {
                panic!("{}", e)
            }
            Ok(r) => {
                tasks.extend(r);
                info!("State change searcher actor started successfully")
            }
        }

        if self.mempool_events_tx.is_some() && self.use_mempool {
            let mempool_events_tx = self.mempool_events_tx.clone().unwrap();
            let market_events_tx = self.market_events_tx.clone().unwrap();

            let mempool = match &self.mempool {
                Some(m) => m.clone(),
                None => {
                    return Err(eyre::eyre!("StateChangeArbActor: mempool is None"));
                }
            };
            let latest_block = match &self.latest_block {
                Some(lb) => lb.clone(),
                None => {
                    return Err(eyre::eyre!("StateChangeArbActor: latest_block is None"));
                }
            };
            let market = match &self.market {
                Some(m) => m.clone(),
                None => {
                    return Err(eyre::eyre!("StateChangeArbActor: market is None"));
                }
            };
            let market_state = match &self.market_state {
                Some(ms) => ms.clone(),
                None => {
                    return Err(eyre::eyre!("StateChangeArbActor: market_state is None"));
                }
            };

            let rate_limit_rps = self.backrun_config.rate_limit_rps.unwrap_or(0);
            let client = RateLimitedClient::new(self.client.clone(), rate_limit_rps);
            let mut pending_tx_state_processor = PendingTxStateChangeProcessorActor::new(client);
            match pending_tx_state_processor
                .access(mempool)
                .access(latest_block)
                .access(market)
                .access(market_state)
                .consume(mempool_events_tx)
                .consume(market_events_tx)
                .produce(searcher_pool_update_channel.clone())
                .start()
            {
                Err(e) => {
                    panic!("{}", e)
                }
                Ok(r) => {
                    tasks.extend(r);
                    info!("Pending tx state actor started successfully")
                }
            }
        }

        if self.market_events_tx.is_some() && self.use_blocks {
            let market_events_tx = self.market_events_tx.clone().unwrap();

            let market = match &self.market {
                Some(m) => m.clone(),
                None => {
                    return Err(eyre::eyre!("StateChangeArbActor: market is None"));
                }
            };
            let block_history = match &self.block_history {
                Some(bh) => bh.clone(),
                None => {
                    return Err(eyre::eyre!("StateChangeArbActor: block_history is None"));
                }
            };

            let mut block_state_processor = BlockStateChangeProcessorActor::new();
            match block_state_processor
                .access(market)
                .access(block_history)
                .consume(market_events_tx)
                .produce(searcher_pool_update_channel.clone())
                .start()
            {
                Err(e) => {
                    panic!("{}", e)
                }
                Ok(r) => {
                    tasks.extend(r);
                    info!("Block change state actor started successfully")
                }
            }
        }

        Ok(tasks)
    }

    fn name(&self) -> &'static str {
        "StateChangeArbActor"
    }
}
