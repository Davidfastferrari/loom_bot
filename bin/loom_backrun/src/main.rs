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

#[tokio::main]
async fn main() -> Result<()> {
    env_logger::Builder::from_env(
        env_logger::Env::default().default_filter_or("info,tokio_tungstenite=off,tungstenite=off,alloy_rpc_client=off,alloy_transport_http=off,hyper_util=off"),
    )
    .format_timestamp_micros()
    .init();

    let topology_config = TopologyConfig::load_from_file("config.toml".to_string())?;
    let influxdb_config = topology_config.influxdb.clone();

    let encoder = MulticallerSwapEncoder::default();

    let mut topology =
        Topology::<LoomDBType>::from_config(topology_config).with_swap_encoder(encoder).start_clients().await?;

    // Debug logging to verify clients and default client name
    info!("Clients keys after start_clients: {:?}", topology.clients.keys().collect::<Vec<_>>());
    info!("Default client name after start_clients: {:?}", topology.default_client_name);

    // Set the default client to "local" to match our config
    topology.set_default_client("local")?;

    // Initialize blockchains field with "base" blockchain with chain ID 8453
    let mut chain_id_map = std::collections::HashMap::new();
    chain_id_map.insert("base".to_string(), 8453);
    topology.initialize_blockchains(&chain_id_map)?;
    
    // Initialize signers
    topology.initialize_signers("env_signer")?;
    
    // Set the default blockchain name to "base" to match our config
    topology.set_default_blockchain("base")?;
    
    let mut worker_task_vec = topology.start_actors().await?;

    // The rest of main.rs unchanged...
    // ...
    // (omitted for brevity)
    
    Ok(())
}
