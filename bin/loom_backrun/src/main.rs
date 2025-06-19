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

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,tokio_tungstenite=off,tungstenite=off,alloy_rpc_client=off,alloy_transport_http=off,hyper_util=off"),
    )
    .format_timestamp_micros()
    .init();

    let topology_config = TopologyConfig::load_from_file("config.toml".to_string())?;
    let influxdb_config = topology_config.influxdb.clone();

    // Parse the multicaller address from config before initializing topology
    let multicaller_address = "0x6E3b634eBd2EbBffb41a49fA6edF6df6bFe8c0Ee".parse().expect("Invalid multicaller address");
    
    // Create a custom encoder with the address set
    let mut encoder = MulticallerSwapEncoder::default();
    encoder.set_address(multicaller_address);
    
    let mut topology =
        Topology::<LoomDBType>::from_config(topology_config).with_swap_encoder(encoder).start_clients().await?;

    // Debug logging to verify clients and default client name
    info!("Clients keys after start_clients: {:?}", topology.get_clients_keys());
    info!("Default client name after start_clients: {:?}", topology.get_default_client_name());

    // Set the default client to "local" to match our config
    topology.set_default_client("local")?;

    // Initialize blockchains field with "base" blockchain with chain ID 8453
    let mut chain_id_map = std::collections::HashMap::new();
    chain_id_map.insert("base".to_string(), 8453);
    topology.initialize_blockchains(&chain_id_map)?;
    
    // Set the default blockchain name to "base" to match our config
    topology.set_default_blockchain("base")?;
    
    // Initialize signers
    topology.initialize_signers("env_signer")?;
    
    // Set the multicaller address using the new public setter methods
    topology.set_multicaller_encoder("multicaller".to_string(), multicaller_address);
    topology.set_default_multicaller_encoder_name(Some("multicaller".to_string()));
    
    let mut worker_task_vec = topology.start_actors().await?;

    let client = topology.get_client(None)?;

    // Load backrun strategy configuration
    let backrun_config = load_from_file::<BackrunConfig>("config.toml".to_string())?;
    info!("Backrun config loaded: {:?}", backrun_config);

    // Get the blockchain for the backrun strategy
    let blockchain = topology.get_blockchain(Some(&"base".to_string()))?;
    let blockchain_state = topology.get_blockchain_state(Some(&"base".to_string()))?;

    // Create and start the backrun strategy actor
    let mut backrun_actor = StateChangeArbActor::new(backrun_config);
    let backrun_tasks = backrun_actor
        .access(blockchain.market())
        .access(blockchain_state.market_state())
        .consume(blockchain.market_events_channel())
        .produce(blockchain.swap_compose_channel())
        .produce(blockchain.health_monitor_channel())
        .produce(blockchain.influxdb_write_channel())
        .start()?;
    
    worker_task_vec.extend(backrun_tasks);
    info!("Backrun actor started successfully");

    // Create and start the merger actors
    let mut same_path_merger = SamePathMergerActor::new();
    let same_path_merger_tasks = same_path_merger
        .consume(blockchain.swap_compose_channel())
        .produce(blockchain.swap_compose_channel())
        .start()?;
    
    worker_task_vec.extend(same_path_merger_tasks);
    info!("Same path merger actor started successfully");

    let mut diff_path_merger = DiffPathMergerActor::new();
    let diff_path_merger_tasks = diff_path_merger
        .consume(blockchain.swap_compose_channel())
        .produce(blockchain.swap_compose_channel())
        .start()?;
    
    worker_task_vec.extend(diff_path_merger_tasks);
    info!("Diff path merger actor started successfully");

    let mut arb_swap_merger = ArbSwapPathMergerActor::new();
    let arb_swap_merger_tasks = arb_swap_merger
        .consume(blockchain.swap_compose_channel())
        .produce(blockchain.swap_compose_channel())
        .start()?;
    
    worker_task_vec.extend(arb_swap_merger_tasks);
    info!("Arb swap merger actor started successfully");

    // Create and start the router actor
    let mut router_actor = SwapRouterActor::new();
    let router_tasks = router_actor
        .consume(blockchain.swap_compose_channel())
        .produce(blockchain.tx_compose_channel())
        .start()?;
    
    worker_task_vec.extend(router_tasks);
    info!("Router actor started successfully");

    // Create and start health monitor actors
    let mut state_health_monitor = StateHealthMonitorActor::new(client.clone());
    let state_health_monitor_tasks = state_health_monitor
        .consume(blockchain.tx_compose_channel())
        .consume(blockchain.market_events_channel())
        .produce(blockchain.influxdb_write_channel())
        .start()?;
    
    worker_task_vec.extend(state_health_monitor_tasks);
    info!("State health monitor actor started successfully");

    let mut stuffing_tx_monitor = StuffingTxMonitorActor::new(client.clone());
    let stuffing_tx_monitor_tasks = stuffing_tx_monitor
        .consume(blockchain.tx_compose_channel())
        .consume(blockchain.market_events_channel())
        .produce(blockchain.influxdb_write_channel())
        .start()?;
    
    worker_task_vec.extend(stuffing_tx_monitor_tasks);
    info!("Stuffing tx monitor actor started successfully");

    // Create and start metrics recorder if InfluxDB is configured
    if let Some(influxdb_config) = influxdb_config {
        let mut metrics_recorder = MetricsRecorderActor::new();
        let metrics_recorder_tasks = metrics_recorder
            .consume(blockchain.health_monitor_channel())
            .produce(blockchain.influxdb_write_channel())
            .start()?;
        
        worker_task_vec.extend(metrics_recorder_tasks);
        info!("Metrics recorder actor started successfully");

        let mut influxdb_writer = InfluxDbWriterActor::new(
            influxdb_config.url,
            influxdb_config.database,
            influxdb_config.tags,
        );
        let influxdb_writer_tasks = influxdb_writer
            .consume(blockchain.influxdb_write_channel())
            .start()?;
        
        worker_task_vec.extend(influxdb_writer_tasks);
        info!("InfluxDB writer actor started successfully");
    } else {
        info!("InfluxDB not configured, skipping metrics recording");
    }

    info!("All actors started successfully. Loom backrun bot is now running.");
    
    // Wait for all tasks to complete (they should run indefinitely)
    for task in worker_task_vec {
        if let Err(e) = task.await {
            error!("Task error: {:?}", e);
        }
    }
    
    Ok(())
}
