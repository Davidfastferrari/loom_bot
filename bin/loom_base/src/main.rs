use alloy::providers::Provider;
use eyre::Result;
use tracing::{error, info};

use loom::core::actors::{Accessor, Actor, Consumer, Producer};
use loom::core::router::SwapRouterActor;
use loom::core::topology::{Topology, TopologyConfig};
use loom::defi::health_monitor::{MetricsRecorderActor, StateHealthMonitorActor, StuffingTxMonitorActor};
use loom::evm::db::LoomDBType;
use loom::execution::multicaller::MulticallerSwapEncoder;
use loom_core_topology::InfluxDbConfig;
use loom::metrics::InfluxDbWriterActor;
use loom::strategy::backrun::{BackrunConfig, BackrunConfigSection, StateChangeArbActor};
use loom::strategy::merger::{ArbSwapPathMergerActor, DiffPathMergerActor, SamePathMergerActor};
use loom::types::entities::strategy_config::load_from_file;
use loom::types::events::MarketEvents;
use loom::strategy::simple_arb::SimpleArbFinderActor;
use loom::broadcast::accounts::TxSignersActor;
use loom::broadcast::broadcaster::FlashbotsBroadcastActor;
use loom::broadcast::flashbots::Flashbots;
use loom::execution::estimator::EvmEstimatorActor;

fn initialize_logging() {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,tokio_tungstenite=off,tungstenite=off,alloy_rpc_client=off"),
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
    let topology_config = TopologyConfig::load_from_file("config.toml".to_string())?;
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
        Topology::<LoomDBType>::from_config(topology_config).with_swap_encoder(encoder).start_clients().await?;

    let mut worker_task_vec = topology.start_actors().await.map_err(Into::<eyre::Report>::into)?;

    // Get blockchain and client for Base network
    let client = topology.get_client(Some("local".to_string()).as_ref()).map_err(Into::<eyre::Report>::into)?;
    let blockchain = topology.get_blockchain(Some("base".to_string()).as_ref()).map_err(Into::<eyre::Report>::into)?;
    let blockchain_state = topology.get_blockchain_state(Some("base".to_string()).as_ref()).map_err(Into::<eyre::Report>::into)?;
    let strategy = topology.get_strategy(Some("base".to_string()).as_ref()).map_err(Into::<eyre::Report>::into)?;

    let tx_signers = topology.get_signers(Some("env_signer".to_string()).as_ref())?;

    // Load backrun configuration
    let backrun_config: BackrunConfigSection = load_from_file("./config.toml".to_string().into()).await?;
    let mut backrun_config: BackrunConfig = backrun_config.backrun_strategy;
    
    // Use the chain ID from the backrun config
    let chain_id = backrun_config.chain_id();
    info!("Using chain ID from config: {}", chain_id);
    // No need to set chain_id again as it's already in the config

    // Retry logic with exponential backoff for get_block_number
    let mut retries = 0;
    let block_nr = loop {
        match client.get_block_number().await {
            Ok(block) => break block,
            Err(e) => {
                if retries >= 5 {
                    return Err(eyre::eyre!("Failed to get block number after 5 retries: {}", e));
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

    let multicaller_address = topology.get_multicaller_address(None)?;
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

    // Start the EVM estimator actor (critical for converting Prepare -> Estimate -> Ready)
    info!("Starting EVM estimator actor");
    // Build encoder
    let multicaller_encoder = MulticallerSwapEncoder::default_with_address(multicaller_address);
    let mut evm_estimator_actor = EvmEstimatorActor::new_with_provider(multicaller_encoder, Some(client.clone()));
    let result = evm_estimator_actor
        .consume(strategy.swap_compose_channel())
        .produce(strategy.swap_compose_channel())
        .produce(blockchain.health_monitor_channel())
        .produce(blockchain.influxdb_write_channel())
        .start();
    
    worker_task_vec.extend(start_actor("EVM estimator actor", result));

    // Start the signers actor (critical for converting Sign -> Broadcast)
    info!("Starting signers actor");
    let mut signers_actor = TxSignersActor::new();
    let result = signers_actor
        .consume(blockchain.tx_compose_channel())
        .produce(blockchain.tx_compose_channel())
        .start();
    
    worker_task_vec.extend(start_actor("Signers actor", result));

    // Start the flashbots broadcaster actor (critical for actually broadcasting transactions)
    info!("Starting flashbots broadcaster actor");
    // Initialize Flashbots client with default relays
    let flashbots = Flashbots::new(client.clone(), "https://relay.flashbots.net", None).with_default_relays();
    let mut flashbots_broadcaster_actor = FlashbotsBroadcastActor::new(flashbots.into(), true); // true = allow broadcast
    let result = flashbots_broadcaster_actor
        .consume(blockchain.tx_compose_channel())
        .start();
    
    worker_task_vec.extend(start_actor("Flashbots broadcaster actor", result));

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

    // Set up graceful shutdown handling
    let (shutdown_sender, mut shutdown_receiver) = tokio::sync::mpsc::channel::<()>(1);
    let shutdown_sender_clone = shutdown_sender.clone();
    
    // Handle Ctrl+C signal for graceful shutdown
    tokio::spawn(async move {
        match tokio::signal::ctrl_c().await {
            Ok(()) => {
                info!("Received shutdown signal, initiating graceful shutdown...");
                let _ = shutdown_sender_clone.send(()).await;
            }
            Err(err) => {
                error!("Failed to listen for shutdown signal: {}", err);
            }
        }
    });
    
    // Main event loop
    let mut s = blockchain.market_events_channel().subscribe();
    
    // Add a small delay to prevent CPU spinning if messages are processed very quickly
    let throttle_delay = std::time::Duration::from_millis(10);
    
    // Keep track of active tasks for proper shutdown
    let mut active_tasks = worker_task_vec.len();
    info!("Started {} worker tasks", active_tasks);
    
    // Create a channel for worker task completion notifications
    let (task_complete_tx, mut task_complete_rx) = tokio::sync::mpsc::channel::<()>(active_tasks);
    
    // Spawn a task to monitor worker tasks and notify when they complete
    let task_monitor = tokio::spawn(async move {
        while !worker_task_vec.is_empty() {
            let (result, index, remaining_futures) = futures::future::select_all(worker_task_vec).await;
            match result {
                Ok(work_result) => match work_result {
                    Ok(s) => {
                        info!("ActorWorker {index} finished: {s}");
                        let _ = task_complete_tx.send(()).await;
                    }
                    Err(e) => {
                        error!("ActorWorker {index} error: {e}");
                        let _ = task_complete_tx.send(()).await;
                    }
                },
                Err(e) => {
                    error!("ActorWorker join error {index}: {e}");
                    let _ = task_complete_tx.send(()).await;
                }
            }
            worker_task_vec = remaining_futures;
        }
    });
    
    // Main event loop with proper shutdown handling
    let mut shutdown_initiated = false;
    let mut shutdown_complete = false;
    
    loop {
        // Use tokio::select to handle message reception, task completion, and shutdown signals
        tokio::select! {
            msg = s.recv(), if !shutdown_initiated => {
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
                    // Handle the error case - channel closed
                    error!("Market events channel closed, attempting to resubscribe");
                    // Try to resubscribe
                    s = blockchain.market_events_channel().subscribe();
                }
            }
            
            // Monitor task completion for shutdown coordination
            Some(_) = task_complete_rx.recv(), if shutdown_initiated => {
                active_tasks -= 1;
                info!("Worker task completed during shutdown, {} remaining", active_tasks);
                if active_tasks == 0 {
                    info!("All worker tasks completed, shutdown complete");
                    shutdown_complete = true;
                    break;
                }
            }
            
            // Handle shutdown signal
            Some(_) = shutdown_receiver.recv() => {
                if !shutdown_initiated {
                    info!("Initiating graceful shutdown sequence");
                    shutdown_initiated = true;
                    
                    // Here you would send shutdown signals to all actors
                    // For example, you could have a shutdown channel for each actor
                    
                    // Wait for a maximum of 10 seconds for graceful shutdown
                    tokio::spawn(async move {
                        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
                        info!("Shutdown timeout reached, forcing exit");
                        std::process::exit(0);
                    });
                }
            }
            
            // Add a small delay to prevent CPU spinning
            _ = tokio::time::sleep(throttle_delay), if !shutdown_initiated => {
                // Just a throttle, do nothing
            }
            
            // If shutdown is initiated but no tasks are completing, periodically check status
            _ = tokio::time::sleep(std::time::Duration::from_secs(1)), if shutdown_initiated && !shutdown_complete => {
                info!("Waiting for {} worker tasks to complete...", active_tasks);
            }
        }
        
        // Break the loop if shutdown is complete
        if shutdown_complete {
            break;
        }
    }
    
    // Cancel the task monitor if we're exiting the loop
    task_monitor.abort();
    
    info!("Loom Base shutting down gracefully");
    
    // Return Ok to satisfy the Result<()> return type
    Ok(())
}