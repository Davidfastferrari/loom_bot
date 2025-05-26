use alloy::providers::Provider;
use eyre::Result;
use tracing::{error, info};

use loom::core::actors::{Accessor, Actor, Consumer, Producer};
use loom::core::router::SwapRouterActor;
use loom::core::topology::{Topology, TopologyConfig};
use loom::defi::health_monitor::{MetricsRecorderActor, StateHealthMonitorActor, StuffingTxMonitorActor};
use loom::evm::db::LoomDBType;
use loom::execution::multicaller::MulticallerSwapEncoder;
use loom::metrics::InfluxDbWriterActor;
use loom::strategy::backrun::{BackrunConfig, BackrunConfigSection, StateChangeArbActor};
use loom::strategy::merger::{ArbSwapPathMergerActor, DiffPathMergerActor, SamePathMergerActor};
use loom::types::entities::strategy_config::load_from_file;
use loom::types::events::MarketEvents;
use loom::strategy::simple_arb::SimpleArbFinderActor;

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("debug,tokio_tungstenite=off,tungstenite=off,alloy_rpc_client=off"),
    )
    .format_timestamp_micros()
    .init();

    info!("Starting Loom Base - Combined Arbitrage and Backrunning Bot");

    // Load configuration
    let topology_config = TopologyConfig::load_from_file("config.toml".to_string())?;
    let influxdb_config = topology_config.influxdb.clone();

    let encoder = MulticallerSwapEncoder::default();

    // Initialize topology
    let topology =
        Topology::<LoomDBType>::from_config(topology_config).with_swap_encoder(encoder).build_blockchains().start_clients().await?;

    let mut worker_task_vec = topology.start_actors().await?;

    // Get blockchain and client for Base network
    let client = topology.get_client(Some("local".to_string()).as_ref())?;
    let blockchain = topology.get_blockchain(Some("base".to_string()).as_ref())?;
    let blockchain_state = topology.get_blockchain_state(Some("base".to_string()).as_ref())?;
    let strategy = topology.get_strategy(Some("base".to_string()).as_ref())?;

    let tx_signers = topology.get_signers(Some("env_signer".to_string()).as_ref())?;

    // Load backrun configuration
    let backrun_config: BackrunConfigSection = load_from_file("./config.toml".to_string().into()).await?;
    let mut backrun_config: BackrunConfig = backrun_config.backrun_strategy;
    
    // Set Base network chain ID
    backrun_config = backrun_config.with_chain_id(8453);

    let block_nr = client.get_block_number().await?;
    info!("Current block: {}", block_nr);

    // Start the backrun actor
    info!("Starting state change arb actor");
    let mut state_change_arb_actor = StateChangeArbActor::new(client.clone(), true, true, backrun_config.clone());
    match state_change_arb_actor
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
        .start()
    {
        Err(e) => {
            error!("{}", e)
        }
        Ok(r) => {
            worker_task_vec.extend(r);
            info!("State change arb actor started successfully")
        }
    }

    // Start the simple arbitrage finder actor
    info!("Starting simple arbitrage finder actor");
    let mut simple_arb_finder_actor = SimpleArbFinderActor::new();
    match simple_arb_finder_actor
        .access(blockchain.market())
        .consume(blockchain.market_events_channel())
        .produce(strategy.swap_compose_channel())
        .start()
    {
        Err(e) => {
            error!("{}", e)
        }
        Ok(r) => {
            worker_task_vec.extend(r);
            info!("Simple arbitrage finder actor started successfully")
        }
    }

    let multicaller_address = topology.get_multicaller_address(None)?;
    info!("Starting swap path encoder actor with multicaller at: {}", multicaller_address);

    // Start the swap router actor
    let mut swap_path_encoder_actor = SwapRouterActor::new();
    match swap_path_encoder_actor
        .access(tx_signers.clone())
        .access(blockchain.nonce_and_balance())
        .consume(strategy.swap_compose_channel())
        .produce(strategy.swap_compose_channel())
        .produce(blockchain.tx_compose_channel())
        .start()
    {
        Ok(r) => {
            worker_task_vec.extend(r);
            info!("Swap path encoder actor started successfully")
        }
        Err(e) => {
            panic!("ArbSwapPathEncoderActor {}", e)
        }
    }

    // Start the merger actors
    info!("Starting swap path merger actor");
    let mut swap_path_merger_actor = ArbSwapPathMergerActor::new(multicaller_address);
    match swap_path_merger_actor
        .access(blockchain.latest_block())
        .consume(blockchain.market_events_channel())
        .consume(strategy.swap_compose_channel())
        .produce(strategy.swap_compose_channel())
        .start()
    {
        Ok(r) => {
            worker_task_vec.extend(r);
            info!("Swap path merger actor started successfully")
        }
        Err(e) => {
            panic!("{}", e)
        }
    }

    let mut same_path_merger_actor = SamePathMergerActor::new(client.clone());
    match same_path_merger_actor
        .access(blockchain_state.market_state())
        .access(blockchain.latest_block())
        .consume(blockchain.market_events_channel())
        .consume(strategy.swap_compose_channel())
        .produce(strategy.swap_compose_channel())
        .start()
    {
        Ok(r) => {
            worker_task_vec.extend(r);
            info!("Same path merger actor started successfully")
        }
        Err(e) => {
            panic!("{}", e)
        }
    }

    let mut diff_path_merger_actor = DiffPathMergerActor::new();
    match diff_path_merger_actor
        .consume(blockchain.market_events_channel())
        .consume(strategy.swap_compose_channel())
        .produce(strategy.swap_compose_channel())
        .start()
    {
        Ok(r) => {
            worker_task_vec.extend(r);
            info!("Diff path merger actor started successfully")
        }
        Err(e) => {
            panic!("{}", e)
        }
    }

    // Start the health monitoring actors
    let mut state_health_monitor_actor = StateHealthMonitorActor::new(client.clone());
    match state_health_monitor_actor
        .access(blockchain_state.market_state())
        .consume(blockchain.tx_compose_channel())
        .consume(blockchain.market_events_channel())
        .start()
    {
        Err(e) => {
            panic!("State health monitor actor failed: {}", e)
        }
        Ok(r) => {
            worker_task_vec.extend(r);
            info!("State health monitor actor started successfully")
        }
    }

    let mut stuffing_txs_monitor_actor = StuffingTxMonitorActor::new(client.clone());
    match stuffing_txs_monitor_actor
        .access(blockchain.latest_block())
        .consume(blockchain.tx_compose_channel())
        .consume(blockchain.market_events_channel())
        .produce(blockchain.influxdb_write_channel())
        .start()
    {
        Err(e) => {
            panic!("Stuffing txs monitor actor failed: {}", e)
        }
        Ok(r) => {
            worker_task_vec.extend(r);
            info!("Stuffing txs monitor actor started successfully")
        }
    }

    // Start InfluxDB metrics if configured
    if let Some(influxdb_config) = influxdb_config {
        let mut influxdb_writer_actor = InfluxDbWriterActor::new(influxdb_config.url, influxdb_config.database, influxdb_config.tags);
        match influxdb_writer_actor.consume(blockchain.influxdb_write_channel()).start() {
            Err(e) => {
                panic!("InfluxDB writer actor failed: {}", e)
            }
            Ok(r) => {
                worker_task_vec.extend(r);
                info!("InfluxDB writer actor started successfully")
            }
        }

        let mut block_latency_recorder_actor = MetricsRecorderActor::new();
        match block_latency_recorder_actor
            .access(blockchain.market())
            .access(blockchain_state.market_state())
            .consume(blockchain.new_block_headers_channel())
            .produce(blockchain.influxdb_write_channel())
            .start()
        {
            Err(e) => {
                panic!("Block latency recorder actor failed: {}", e)
            }
            Ok(r) => {
                worker_task_vec.extend(r);
                info!("Block latency recorder actor started successfully")
            }
        }
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
    loop {
        let msg = s.recv().await;
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
        }
    }
}