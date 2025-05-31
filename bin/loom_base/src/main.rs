use alloy::providers::Provider;
use eyre::Result;
use tracing::{error, info};

use loom::core::actors::{Accessor, Actor, Consumer, Producer};
use loom::core::router::SwapRouterActor;
use loom::core::topology::{Topology, TopologyConfig};
use loom::defi::health_monitor::{MetricsRecorderActor, StateHealthMonitorActor, StuffingTxMonitorActor};
use loom::evm::db::LoomDBType;
use loom::execution::multicaller::MulticallerSwapEncoder;
use loom::metrics::{InfluxDbConfig, InfluxDbWriterActor};
use loom::strategy::backrun::{BackrunConfig, BackrunConfigSection, StateChangeArbActor};
use loom::strategy::merger::{ArbSwapPathMergerActor, DiffPathMergerActor, SamePathMergerActor};
use loom::types::entities::strategy_config::load_from_file;
use loom::types::events::MarketEvents;
use loom::strategy::simple_arb::SimpleArbFinderActor;

fn initialize_logging() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("debug,tokio_tungstenite=off,tungstenite=off,alloy_rpc_client=off"),
    )
    .format_timestamp_micros()
    .init();

    info!("Starting Loom Base - Combined Arbitrage and Backrunning Bot");
}

/// Helper function to start an actor and handle its result consistently
fn start_actor<T>(
    actor_name: &str, 
    result: std::result::Result<Vec<tokio::task::JoinHandle<Result<String, eyre::Report>>>, T>
) -> Vec<tokio::task::JoinHandle<Result<String, eyre::Report>>> 
where 
    T: std::fmt::Display,
{
    match result {
        Ok(handles) => {
            info!("{} started successfully", actor_name);
            handles
        }
        Err(e) => {
            error!("{} failed to start: {}", actor_name, e);
            // Instead of panicking, we return an empty vector
            Vec::new()
        }
    }
}

async fn load_configuration() -> Result<(TopologyConfig, Option<InfluxDbConfig>)> {
    let topology_config = TopologyConfig::load_from_file("config.toml".to_string()).map_err(Into::into)?;
    let influxdb_config = topology_config.influxdb.clone();
    
    Ok((topology_config, influxdb_config))
}

#[tokio::main]
async fn main() -> Result<()> {
    initialize_logging();
    
    // Load configuration
    let (topology_config, influxdb_config) = load_configuration().await?;

    let encoder = MulticallerSwapEncoder::default();

    // Initialize topology
    let topology =
        Topology::<LoomDBType>::from_config(topology_config).with_swap_encoder(encoder).build_blockchains().start_clients().await.map_err(Into::into)?;

    let mut worker_task_vec = topology.start_actors().await.map_err(Into::into)?;

    // Get blockchain and client for Base network
    let client = topology.get_client(Some("local".to_string()).as_ref()).map_err(Into::into)?;
    let blockchain = topology.get_blockchain(Some("base".to_string()).as_ref()).map_err(Into::into)?;
    let blockchain_state = topology.get_blockchain_state(Some("base".to_string()).as_ref()).map_err(Into::into)?;
    let strategy = topology.get_strategy(Some("base".to_string()).as_ref()).map_err(Into::into)?;

    let tx_signers = topology.get_signers(Some("env_signer".to_string()).as_ref()).map_err(Into::into)?;

    // Load backrun configuration
    let backrun_config: BackrunConfigSection = load_from_file("./config.toml".to_string().into()).await.map_err(Into::into)?;
    let mut backrun_config: BackrunConfig = backrun_config.backrun_strategy;
    
    // Set Base network chain ID (using default for Base network)
    let chain_id = 8453; // Base network chain ID
    info!("Using chain ID: {}", chain_id);
    backrun_config = backrun_config.with_chain_id(chain_id);

    // Retry logic with exponential backoff for get_block_number
    let mut retries = 0;
    let block_nr = loop {
        match client.get_block_number().await {
            Ok(block) => break block,
            Err(e) => {
                if retries >= 5 {
                    return Err(e.into());
                }
                let backoff = 2u64.pow(retries) * 100;
                info!("get_block_number failed, retrying in {} ms: {}", backoff, e);
                tokio::time::sleep(std::time::Duration::from_millis(backoff)).await;
                retries += 1;
            }
        }
    };
    
    info!("Current block: {}", block_nr);

    // Start the backrun actors
    info!("Starting state change arb actor");
    let mut state_change_arb_actor = StateChangeArbActor::new(client.clone(), true, true, backrun_config.clone());
    let result = state_change_arb_actor
        .access(blockchain.mempool())
        .access(blockchain.latest_block())
        .access(blockchain.market())
        .access(blockchain_state.market_state())
        .access(blockchain_state.block_history())
        .consume(blockchain.market_events_channel())
        .consume(blockchain.mempool_events_channel())
        .produce(strategy.swap_compose_channel())
        .produce(blockchain.health_monitor_channel())
        .produce(blockchain.influxdb_write_channel())
        .start();
    
    worker_task_vec.extend(start_actor("State change arb actor", result));

    // Start the simple arbitrage finder actor
    info!("Starting simple arbitrage finder actor");
    let mut simple_arb_finder_actor = SimpleArbFinderActor::new();
    let result = simple_arb_finder_actor
        .access(blockchain.market())
        .consume(blockchain.market_events_channel())
        .produce(strategy.swap_compose_channel())
        .start();
    
    worker_task_vec.extend(start_actor("Simple arbitrage finder actor", result));

    let multicaller_address = topology.get_multicaller_address(None).map_err(Into::into)?;
    info!("Starting swap path encoder actor with multicaller at: {}", multicaller_address);

    // Start the swap router actor
    info!("Starting swap path encoder actor");
    let mut swap_path_encoder_actor = SwapRouterActor::new();
    let result = swap_path_encoder_actor
        .access(tx_signers.clone())
        .access(blockchain.nonce_and_balance())
        .consume(strategy.swap_compose_channel())
        .produce(strategy.swap_compose_channel())
        .produce(blockchain.tx_compose_channel())
        .start();
    
    worker_task_vec.extend(start_actor("Swap path encoder actor", result));

    // Start the merger actors
    info!("Starting swap path merger actor");
    let mut swap_path_merger_actor = ArbSwapPathMergerActor::new(multicaller_address);
    let result = swap_path_merger_actor
        .access(blockchain.latest_block())
        .consume(blockchain.market_events_channel())
        .consume(strategy.swap_compose_channel())
        .produce(strategy.swap_compose_channel())
        .start();
    
    worker_task_vec.extend(start_actor("Swap path merger actor", result));

    let mut same_path_merger_actor = SamePathMergerActor::new(client.clone());
    let result = same_path_merger_actor
        .access(blockchain_state.market_state())
        .access(blockchain.latest_block())
        .consume(blockchain.market_events_channel())
        .consume(strategy.swap_compose_channel())
        .produce(strategy.swap_compose_channel())
        .start();
    
    worker_task_vec.extend(start_actor("Same path merger actor", result));

    let mut diff_path_merger_actor = DiffPathMergerActor::new();
    let result = diff_path_merger_actor
        .consume(blockchain.market_events_channel())
        .consume(strategy.swap_compose_channel())
        .produce(strategy.swap_compose_channel())
        .start();
    
    worker_task_vec.extend(start_actor("Diff path merger actor", result));

    // Start the health monitoring actors
    let mut state_health_monitor_actor = StateHealthMonitorActor::new(client.clone());
    let result = state_health_monitor_actor
        .access(blockchain_state.market_state())
        .consume(blockchain.tx_compose_channel())
        .consume(blockchain.market_events_channel())
        .start();
    
    worker_task_vec.extend(start_actor("State health monitor actor", result));

    let mut stuffing_txs_monitor_actor = StuffingTxMonitorActor::new(client.clone());
    let result = stuffing_txs_monitor_actor
        .access(blockchain.latest_block())
        .consume(blockchain.tx_compose_channel())
        .consume(blockchain.market_events_channel())
        .produce(blockchain.influxdb_write_channel())
        .start();
    
    worker_task_vec.extend(start_actor("Stuffing txs monitor actor", result));

    // Start InfluxDB metrics if configured
    if let Some(influxdb_config) = influxdb_config {
        let mut influxdb_writer_actor = InfluxDbWriterActor::new(influxdb_config.url, influxdb_config.database, influxdb_config.tags);
        let result = influxdb_writer_actor.consume(blockchain.influxdb_write_channel()).start();
        worker_task_vec.extend(start_actor("InfluxDB writer actor", result));

        let mut block_latency_recorder_actor = MetricsRecorderActor::new();
        let result = block_latency_recorder_actor
            .access(blockchain.market())
            .access(blockchain_state.market_state())
            .consume(blockchain.new_block_headers_channel())
            .produce(blockchain.influxdb_write_channel())
            .start();
        
        worker_task_vec.extend(start_actor("Block latency recorder actor", result));
    }

    // Monitor worker tasks
    tokio::task::spawn(async move {
        while !worker_task_vec.is_empty() {
            let (result, _index, remaining_futures) = futures::future::select_all(worker_task_vec).await;
            match result {
                Ok(work_result) => match work_result {
                    Ok(s) => {
                        info!("ActorWorker {_index} finished: {s}")
                    }
                    Err(e) => {
                        error!("ActorWorker {_index} error: {e}")
                    }
                },
                Err(e) => {
                    error!("ActorWorker join error {_index}: {e}")
                }
            }
            worker_task_vec = remaining_futures;
        }
    });

    // Main event loop
    let mut s = blockchain.market_events_channel().subscribe();
    
    // Add a small delay to prevent CPU spinning if messages are processed very quickly
    let throttle_delay = std::time::Duration::from_millis(10);
    
    loop {
        // Use tokio::select to handle both message reception and potential shutdown signals
        tokio::select! {
            msg = s.recv() => {
                if let Ok(msg) = msg {
                    match msg {
                        MarketEvents::BlockTxUpdate { block_number, block_hash } => {
                            info!("New block received {} {}", block_number, block_hash);
                        }
                        MarketEvents::BlockStateUpdate { block_hash } => {
                            info!("New block state received {}", block_hash);
                        }
                        _ => {}
                    }
                } else {
                    // Handle the error case - either break the loop or return with an error
                    error!("Error receiving message from channel");
                    break;
                }
            }
            // Add a small delay to prevent CPU spinning
            _ = tokio::time::sleep(throttle_delay) => {
                // Just a throttle, do nothing
            }
        }
    }
    
    // Return Ok to satisfy the Result<()> return type
    Ok(())
}