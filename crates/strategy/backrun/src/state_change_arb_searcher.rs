use std::collections::{BTreeMap, HashSet};
use std::sync::Arc;

use alloy_primitives::U256;
#[cfg(not(debug_assertions))]
use chrono::TimeDelta;
use eyre::{eyre, ErrReport, Result};
use influxdb::{Timestamp, WriteQuery};
use rayon::prelude::*;
use rayon::{ThreadPool, ThreadPoolBuilder};
use revm::{DatabaseCommit, DatabaseRef};
use tokio::sync::broadcast::error::RecvError;
#[cfg(not(debug_assertions))]
use tracing::warn;
use tracing::{debug, error, info, trace};

use crate::BackrunConfig;
use crate::profit_calculator::ProfitCalculator;
use crate::SwapCalculator;
use loom_core_actors::{subscribe, Accessor, Actor, ActorResult, Broadcaster, Consumer, Producer, SharedState, WorkerResult};
use loom_core_actors_macros::{Accessor, Consumer, Producer};
use loom_core_blockchain::{Blockchain, Strategy};
use loom_evm_db::DatabaseHelpers;
use loom_types_entities::strategy_config::StrategyConfig;
use loom_types_entities::{Market, PoolWrapper, Swap, SwapDirection, SwapError, SwapLine, SwapPath};
use loom_types_events::{
    BestTxSwapCompose, HealthEvent, Message, MessageHealthEvent, MessageSwapCompose, StateUpdateEvent, SwapComposeData, SwapComposeMessage,
    TxComposeData,
};

async fn state_change_arb_searcher_task<DB: DatabaseRef<Error = ErrReport> + DatabaseCommit + Send + Sync + Clone + Default + 'static>(
    thread_pool: Arc<ThreadPool>,
    backrun_config: BackrunConfig,
    state_update_event: StateUpdateEvent<DB>,
    market: SharedState<Market>,
    swap_request_tx: Broadcaster<MessageSwapCompose<DB>>,
    pool_health_monitor_tx: Broadcaster<MessageHealthEvent>,
    influxdb_write_channel_tx: Broadcaster<WriteQuery>,
) -> Result<()> {
    debug!("Message received {} stuffing : {:?}", state_update_event.origin, state_update_event.stuffing_tx_hash());

    let mut db = state_update_event.market_state().clone();
    DatabaseHelpers::apply_geth_state_update_vec(&mut db, state_update_event.state_update().clone());

    let start_time_utc = chrono::Utc::now();

    let start_time = std::time::Instant::now();
    let mut swap_path_set: HashSet<SwapPath> = HashSet::new();

    let market_guard_read = market.read().await;
    debug!(elapsed = start_time.elapsed().as_micros(), "market_guard market.read acquired");

    for (pool, v) in state_update_event.directions().iter() {
        let pool_paths: Vec<SwapPath> = match market_guard_read.get_pool_paths(&pool.get_pool_id()) {
            Some(paths) => {
                let pool_paths = paths
                    .into_iter()
                    .enumerate()
                    .filter(|(idx, swap_path)| {
                        *idx < 100 || swap_path.score.unwrap_or_default() > 0.97
                        //&& !swap_path.pools.iter().any(|pool| market_guard_read.is_pool_disabled(&pool.get_pool_id()))
                    })
                    .map(|(_, swap_path)| swap_path)
                    .collect::<Vec<_>>();

                // let pool_paths = pool_paths
                //     .into_iter()
                //     .enumerate()
                //     .filter(|(idx, path)| *idx < 100 || path.score.unwrap_or_default() > 0.9)
                //     .map(|(idx, path)| path)
                //     .collect::<Vec<_>>();
                // pool_paths
                pool_paths
            }

            None => {
                let mut pool_direction: BTreeMap<PoolWrapper, Vec<SwapDirection>> = BTreeMap::new();
                pool_direction.insert(pool.clone(), v.clone());
                market_guard_read.build_swap_path_vec(&pool_direction).unwrap_or_default()
            }
        };

        for pool_path in pool_paths {
            swap_path_set.insert(pool_path);
        }
    }
    drop(market_guard_read);
    debug!(elapsed = start_time.elapsed().as_micros(), "market_guard market.read released");

    let swap_path_vec: Vec<SwapPath> = swap_path_set.into_iter().collect();

    if swap_path_vec.is_empty() {
        debug!(
            request=?state_update_event.stuffing_txs_hashes().first().unwrap_or_default(),
            elapsed=start_time.elapsed().as_micros(),
            "No swap path built",

        );
        return Err(eyre!("NO_SWAP_PATHS"));
    }
    info!("Calculation started: swap_path_vec_len={} elapsed={}", swap_path_vec.len(), start_time.elapsed().as_micros());

    let env = state_update_event.evm_env();

    let channel_len = swap_path_vec.len();
    let (swap_path_tx, mut swap_line_rx) = tokio::sync::mpsc::channel(channel_len);

    let market_state_clone = db.clone();
    let swap_path_vec_len = swap_path_vec.len();

    // Clone backrun_config before moving it into the async block
    let backrun_config_clone = backrun_config.clone();
    
    tokio::task::spawn(async move {
        thread_pool.install(|| {
            swap_path_vec.into_par_iter().for_each_with((&swap_path_tx, &market_state_clone, &env), |req, item| {
                let mut mut_item: SwapLine = SwapLine { path: item, ..Default::default() };
                //#[cfg(not(debug_assertions))]
                //let start_time = chrono::Local::now();
                // Use enhanced SwapCalculator with dynamic capital allocation
                let calc_result = SwapCalculator::calculate(&mut mut_item, req.1, req.2.clone());
                //#[cfg(not(debug_assertions))]
                //let took_time = chrono::Local::now() - start_time;

                match calc_result {
                    Ok(_) => {
                        // #[cfg(not(debug_assertions))]
                        // {
                        //     if took_time > TimeDelta::new(0, 50 * 1000000).unwrap() {
                        //         warn!("Took longer than expected {} {}", took_time, mut_item.clone())
                        //     }
                        // }
                        trace!("Calc result received: {}", mut_item);

                        if let Ok(profit) = mut_item.profit() {
                            // Calculate minimum profit threshold based on network
                            let min_profit_threshold = if backrun_config_clone.is_base_network() {
                                backrun_config_clone.min_profit_wei()
                            } else {
                                U256::from(state_update_event.next_base_fee * 100_000)
                            };
                            
                            // Check if profit is positive and exceeds the minimum threshold
                            if profit.is_positive() && mut_item.abs_profit_eth() > min_profit_threshold {
                                // Calculate profit in multiple currencies for logging purposes
                                // This doesn't block the main flow since we're just sending the original swap item
                                let eth_profit = mut_item.abs_profit_eth();
                                
                                // Log the ETH profit for now - multi-currency calculation will be done later
                                info!("Profitable opportunity found! ETH profit: {} wei", eth_profit);
                                
                                if let Err(error) = swap_path_tx.try_send(Ok(mut_item)) {
                                    error!(%error, "swap_path_tx.try_send")
                                }
                            } else {
                                trace!("profit is not enough: got={}, required={}", mut_item.abs_profit_eth(), min_profit_threshold)
                            }
                        }
                    }
                    Err(e) => {
                        // #[cfg(not(debug_assertions))]
                        // {
                        //     if took_time > TimeDelta::new(0, 10 * 5000000).unwrap() {
                        //         warn!("Took longer than expected {:?} {}", e, mut_item.clone())
                        //     }
                        // }
                        trace!("Swap error: {:?}", e);

                        if let Err(error) = swap_path_tx.try_send(Err(e)) {
                            error!(%error, "try_send to swap_path_tx")
                        }
                    }
                }
            });
        });
        debug!(elapsed = start_time.elapsed().as_micros(), "Calculation iteration finished");
    });

    debug!(elapsed = start_time.elapsed().as_micros(), "Calculation results receiver started");

    let swap_request_tx_clone = swap_request_tx.clone();
    let pool_health_monitor_tx_clone = pool_health_monitor_tx.clone();

    let mut answers = 0;

    let mut best_answers = BestTxSwapCompose::new_with_pct(U256::from(9000));

    let mut failed_pools: HashSet<SwapError> = HashSet::new();

    while let Some(swap_line_result) = swap_line_rx.recv().await {
        match swap_line_result {
            Ok(swap_line) => {
                // Clone backrun_config for use in this scope
                let backrun_config_clone = backrun_config.clone();
                
                // Calculate optimized gas price with boost
                let base_gas_price = U256::from(state_update_event.next_base_fee);
                let optimized_gas_price = backrun_config_clone.calculate_gas_price(base_gas_price);
                
                // Calculate priority fee (the part above base fee)
                let priority_fee = u64::try_from(optimized_gas_price.saturating_sub(base_gas_price)).unwrap_or(0);
                
                // Determine if we should use private transactions
                let use_private_tx = backrun_config_clone.private_tx_enabled();
                let private_tx_url = backrun_config_clone.private_tx_url();
                
                // Determine if we should use MEV blocker
                let use_mev_blocker = backrun_config_clone.mev_blocker_enabled();
                
                // Log gas optimization info
                debug!(
                    "Gas optimization: base_fee={}, optimized={}, priority_fee={}, boost={}%, private_tx={}, mev_blocker={}",
                    base_gas_price,
                    optimized_gas_price,
                    priority_fee,
                    backrun_config_clone.gas_boost_percent(),
                    use_private_tx,
                    use_mev_blocker
                );
                
                // Store MEV protection info in a custom field
                let mev_info = if use_private_tx || use_mev_blocker {
                    format!(
                        "{{\"private_tx\":{},\"mev_blocker\":{},\"private_tx_url\":\"{}\"}}",
                        use_private_tx,
                        use_mev_blocker,
                        private_tx_url.unwrap_or_default()
                    )
                } else {
                    String::new()
                };
                
                let prepare_request = SwapComposeMessage::Prepare(SwapComposeData {
                    tx_compose: TxComposeData {
                        eoa: backrun_config_clone.eoa(),
                        next_block_number: state_update_event.next_block_number,
                        next_block_timestamp: state_update_event.next_block_timestamp,
                        next_block_base_fee: state_update_event.next_base_fee,
                        gas: swap_line.gas_used.unwrap_or(300000),
                        priority_gas_fee: priority_fee,
                        stuffing_txs: state_update_event.stuffing_txs.clone(),
                        stuffing_txs_hashes: state_update_event.stuffing_txs_hashes.clone(),
origin: Some(state_update_event.origin.clone() + &mev_info),
                        ..TxComposeData::default()
                    },
                    swap: Swap::BackrunSwapLine(swap_line),
                    tips_pct: Some(state_update_event.tips_pct),
                    poststate: Some(db.clone()),
                    poststate_update: Some(state_update_event.state_update().clone()),
                    ..SwapComposeData::default()
                });

                if !backrun_config_clone.smart() || best_answers.check(&prepare_request) {
                    // Calculate profit in multiple currencies
                    if let Swap::BackrunSwapLine(ref swap_line) = prepare_request.swap {
                        let eth_profit = swap_line.abs_profit_eth();
                        
                        // Get the chain ID from the backrun config
                        let chain_id = Some(backrun_config_clone.chain_id());
                        
                        // Spawn a task to calculate and log multi-currency profits
                        // This won't block the main execution flow
                    let db_clone = db.clone();
                    tokio::spawn(async move {
                        match ProfitCalculator::calculate_multi_currency_profit(eth_profit, &db_clone, chain_id).await {
                            Ok(multi_profit) => {
                                // Log profits in multiple currencies
                                multi_profit.log_profits();
                            },
                            Err(e) => {
                                error!("Failed to calculate multi-currency profit: {}", e);
                            }
                        }
                    });
                    }
                    
                    if let Err(e) = swap_request_tx_clone.send(Message::new(prepare_request)) {
                        error!("swap_request_tx_clone.send {}", e)
                    }
                }
            }
            Err(swap_error) => {
                if failed_pools.insert(swap_error.clone()) {
                    if let Err(e) = pool_health_monitor_tx_clone.send(Message::new(HealthEvent::PoolSwapError(swap_error))) {
                        error!("try_send to pool_health_monitor error : {:?}", e)
                    }
                }
            }
        }

        answers += 1;
    }

    let stuffing_tx_hash = state_update_event.stuffing_tx_hash();
    let elapsed = start_time.elapsed().as_micros();
    info!(
        origin = %state_update_event.origin,
        swap_path_vec_len,
        answers,
        elapsed,
        stuffing_hash = %stuffing_tx_hash,
        "Calculation finished"
    );

    let write_query = WriteQuery::new(Timestamp::from(start_time_utc), "calculations")
        .add_field("calculations", swap_path_vec_len as u64)
        .add_field("answers", answers as u64)
        .add_field("elapsed", elapsed as u64)
        .add_tag("origin", state_update_event.origin)
        .add_tag("stuffing", stuffing_tx_hash.to_string());

    if let Err(e) = influxdb_write_channel_tx.send(write_query) {
        error!("Failed to send block latency to influxdb: {:?}", e);
    }

    Ok(())
}

pub async fn state_change_arb_searcher_worker<
    DB: DatabaseRef<Error = ErrReport> + DatabaseCommit + Send + Sync + Clone + Default + 'static,
>(
    backrun_config: BackrunConfig,
    market: SharedState<Market>,
    search_request_rx: Broadcaster<StateUpdateEvent<DB>>,
    swap_request_tx: Broadcaster<MessageSwapCompose<DB>>,
    pool_health_monitor_tx: Broadcaster<MessageHealthEvent>,
    influxdb_write_channel_tx: Broadcaster<WriteQuery>,
) -> WorkerResult {
    subscribe!(search_request_rx);

    // Use all available CPUs for better performance with arbitrage opportunities
    let cpus = num_cpus::get();
    // Use at least 2 threads, or more based on available CPUs
    let tasks = std::cmp::max(2, cpus);
    info!("Starting state arb searcher cpus={cpus}, tasks={tasks}");
    let thread_pool = Arc::new(ThreadPoolBuilder::new().num_threads(tasks).build()?);

    loop {
        tokio::select! {
                msg = search_request_rx.recv() => {
                let pool_update_msg : Result<StateUpdateEvent<DB>, RecvError> = msg;
                if let Ok(msg) = pool_update_msg {
                    tokio::task::spawn(
                        state_change_arb_searcher_task(
                            thread_pool.clone(),
                            backrun_config.clone(),
                            msg,
                            market.clone(),
                            swap_request_tx.clone(),
                            pool_health_monitor_tx.clone(),
                            influxdb_write_channel_tx.clone(),
                        )
                    );
                }
            }
        }
    }
}

#[derive(Accessor, Consumer, Producer)]
pub struct StateChangeArbSearcherActor<DB: Clone + Send + Sync + 'static> {
    backrun_config: BackrunConfig,
    #[accessor]
    market: Option<SharedState<Market>>,
    #[consumer]
    state_update_rx: Option<Broadcaster<StateUpdateEvent<DB>>>,
    #[producer]
    compose_tx: Option<Broadcaster<MessageSwapCompose<DB>>>,
    #[producer]
    pool_health_monitor_tx: Option<Broadcaster<MessageHealthEvent>>,
    #[producer]
    influxdb_write_channel_tx: Option<Broadcaster<WriteQuery>>,
}

impl<DB: DatabaseRef<Error = ErrReport> + Send + Sync + Clone + 'static> StateChangeArbSearcherActor<DB> {
    pub fn new(backrun_config: BackrunConfig) -> StateChangeArbSearcherActor<DB> {
        StateChangeArbSearcherActor {
            backrun_config,
            market: None,
            state_update_rx: None,
            compose_tx: None,
            pool_health_monitor_tx: None,
            influxdb_write_channel_tx: None,
        }
    }

    pub fn on_bc(self, bc: &Blockchain, strategy: &Strategy<DB>) -> Self {
        Self {
            market: Some(bc.market()),
            pool_health_monitor_tx: Some(bc.health_monitor_channel()),
            compose_tx: Some(strategy.swap_compose_channel()),
            state_update_rx: Some(strategy.state_update_channel()),
            influxdb_write_channel_tx: Some(bc.influxdb_write_channel()),
            ..self
        }
    }
}

impl<DB: DatabaseRef<Error = ErrReport> + DatabaseCommit + Send + Sync + Clone + Default + 'static> Actor
    for StateChangeArbSearcherActor<DB>
{
    fn start(&self) -> ActorResult {
        let task = tokio::task::spawn(state_change_arb_searcher_worker(
            self.backrun_config.clone(),
            self.market.clone().unwrap(),
            self.state_update_rx.clone().unwrap(),
            self.compose_tx.clone().unwrap(),
            self.pool_health_monitor_tx.clone().unwrap(),
            self.influxdb_write_channel_tx.clone().unwrap(),
        ));
        Ok(vec![task])
    }

    fn name(&self) -> &'static str {
        "StateChangeArbSearcherActor"
    }
}
