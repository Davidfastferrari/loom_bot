use std::collections::HashMap;
use std::sync::Arc;

use loom_core_topology_shared::RateLimitedProvider;
use crate::topology_config::TransportType;
use crate::topology_config::{BroadcasterConfig, ClientConfig, EncoderConfig, EstimatorConfig, SignersConfig, TopologyConfig};
use alloy_primitives::Address;
use alloy_provider::network::Ethereum;
use alloy_provider::{Network, Provider, ProviderBuilder, RootProvider};
use alloy_rpc_client::ClientBuilder;
use alloy_transport_ipc::IpcConnect;
use alloy_transport_ws::WsConnect;
use eyre::{eyre, ErrReport, Result};
use url::Url;
use loom_broadcast_accounts::{InitializeSignersOneShotBlockingActor, NonceAndBalanceMonitorActor, TxSignersActor};
use loom_broadcast_broadcaster::FlashbotsBroadcastActor;
use loom_broadcast_flashbots::Flashbots;
use loom_core_actors::{Accessor, Actor, Consumer, Producer, SharedState, WorkerResult};
#[cfg(feature = "loom-core-block-history-actor")]
use loom_core_block_history_actor::BlockHistoryActor;
use loom_core_blockchain::{Blockchain, BlockchainState, Strategy};
#[cfg(not(feature = "with-blockchain"))]
compile_error!("The feature \"with-blockchain\" must be enabled to use loom-core-topology crate because it depends on loom-core-blockchain optionally.");
use loom_core_mempool::MempoolActor;
use loom_defi_health_monitor::PoolHealthMonitorActor;
use loom_defi_market::{HistoryPoolLoaderOneShotActor, NewPoolLoaderActor, PoolLoaderActor, ProtocolPoolLoaderOneShotActor};
use loom_defi_pools::PoolLoadersBuilder;
use loom_defi_preloader::MarketStatePreloadedOneShotActor;
use loom_defi_price::PriceActor;
use loom_evm_db::DatabaseLoomExt;
use loom_execution_estimator::{EvmEstimatorActor, GethEstimatorActor};
use loom_execution_multicaller::MulticallerSwapEncoder;
use loom_node_actor_config::NodeBlockActorConfig;
#[cfg(feature = "db-access")]
use loom_node_db_access::RethDbAccessBlockActor;
use loom_node_grpc::NodeExExGrpcActor;
use loom_node_json_rpc::{NodeBlockActor, NodeMempoolActor};
use loom_types_blockchain::LoomDataTypes;
use loom_types_entities::pool_config::PoolsLoadingConfig;
use loom_types_entities::{BlockHistoryState, MarketState, PoolLoaders, SwapEncoder, TxSigners};
use loom_types_blockchain::loom_data_types_ethereum::LoomDataTypesEthereum;
use revm::{Database, DatabaseCommit, DatabaseRef};
use tokio::task::JoinHandle;
use tracing::{error, info, warn};

pub struct Topology<
    DB: DatabaseRef + Database + DatabaseCommit + BlockHistoryState + DatabaseLoomExt + Clone + Send + Sync + Default + 'static,
    E: Send + Sync + Clone + 'static = MulticallerSwapEncoder,
    P: Provider<N> + Send + Sync + Clone + 'static = RootProvider,
    N: Network = Ethereum,
> {
    config: TopologyConfig,
    clients: HashMap<String, RootProvider<N>>,
    blockchains: HashMap<String, Blockchain<LoomDataTypesEthereum>>,
    blockchain_states: HashMap<String, BlockchainState<DB>>,
    strategies: HashMap<String, Strategy<DB, LoomDataTypesEthereum>>,
    signers: HashMap<String, SharedState<TxSigners>>,
    multicaller_encoders: HashMap<String, Address>,
    default_blockchain_name: Option<String>,
    default_client_name: Option<String>,
    default_multicaller_encoder_name: Option<String>,
    default_signer_name: Option<String>,
    swap_encoder: E,
    pool_loaders: Arc<PoolLoaders<P, N, LoomDataTypesEthereum>>,
}

impl<DB, E, P, N> Topology<DB, E, P, N>
where
    DB: DatabaseRef + Database + DatabaseCommit + BlockHistoryState + DatabaseLoomExt + Clone + Send + Sync + Default + 'static,
    E: Send + Sync + Clone + 'static,
    P: Provider<N> + Send + Sync + Clone + 'static,
    N: Network,
{
    pub fn get_clients_keys(&self) -> Vec<String> {
        self.clients.keys().cloned().collect()
    }

    pub fn get_default_client_name(&self) -> Option<String> {
        self.default_client_name.clone()
    }

    pub fn get_signers_keys(&self) -> Vec<String> {
        self.signers.keys().cloned().collect()
    }

    pub fn get_default_signer_name(&self) -> Option<String> {
        self.default_signer_name.clone()
    }

    // Added setter methods for multicaller fields
    pub fn set_multicaller_encoder(&mut self, name: String, address: Address) {
        self.multicaller_encoders.insert(name, address);
    }

    pub fn set_default_multicaller_encoder_name(&mut self, name: Option<String>) {
        self.default_multicaller_encoder_name = name;
    }
}

impl<
    DB: DatabaseRef + Database + DatabaseCommit + BlockHistoryState + DatabaseLoomExt + Clone + Send + Sync + Default + 'static,
    E: Send + Sync + Clone + 'static,
    P: Provider<N> + Send + Sync + Clone + 'static,
    N: Network,
> Topology<DB, E, P, N> {
    pub fn get_blockchain(&self, name: Option<&String>) -> Result<&Blockchain<LoomDataTypesEthereum>> {
        let name = name.or_else(|| self.default_blockchain_name.as_ref())
            .ok_or_else(|| eyre!("No blockchain name provided and no default blockchain set"))?;
        self.blockchains.get(name)
            .ok_or_else(|| eyre!("Blockchain not found: {}", name))
    }

    pub fn initialize_blockchains(&mut self, chain_id_map: &std::collections::HashMap<String, i64>) -> Result<()> {
        use loom_core_blockchain::Blockchain;
        for (name, chain_id) in chain_id_map.iter() {
            let blockchain = Blockchain::new((*chain_id).try_into().unwrap()); // Convert i64 to u64
            self.blockchains.insert(name.clone(), blockchain);
            
            // Initialize corresponding blockchain state
            let blockchain_state = BlockchainState::<DB>::new();
            self.blockchain_states.insert(name.clone(), blockchain_state);
            
            // Initialize corresponding strategy
            let strategy = Strategy::<DB, LoomDataTypesEthereum>::new();
            self.strategies.insert(name.clone(), strategy);
        }
        
        // Set default blockchain name if not already set
        if self.default_blockchain_name.is_none() && !chain_id_map.is_empty() {
            self.default_blockchain_name = Some(chain_id_map.keys().next().unwrap().clone());
        }
        
        Ok(())
    }
    
    pub fn initialize_signers(&mut self, signer_name: &str) -> Result<()> {
        // Initialize signers
        let signers = SharedState::new(TxSigners::default());
        self.signers.insert(signer_name.to_string(), signers);
        self.default_signer_name = Some(signer_name.to_string());
        Ok(())
    }
    
    pub fn set_default_blockchain(&mut self, blockchain_name: &str) -> Result<()> {
        if !self.blockchains.contains_key(blockchain_name) {
            return Err(eyre!("Blockchain not found: {}", blockchain_name));
        }
        self.default_blockchain_name = Some(blockchain_name.to_string());
        Ok(())
    }
    
    pub fn set_default_client(&mut self, client_name: &str) -> Result<()> {
        if !self.clients.contains_key(client_name) {
            return Err(eyre!("Client not found: {}", client_name));
        }
        // Store the client name in a separate field
        self.default_client_name = Some(client_name.to_string());
        Ok(())
    }

    pub fn get_blockchain_state(&self, name: Option<&String>) -> Result<&BlockchainState<DB>> {
        let name = name.or_else(|| self.default_blockchain_name.as_ref())
            .ok_or_else(|| eyre!("No blockchain name provided and no default blockchain set"))?;
        self.blockchain_states.get(name)
            .ok_or_else(|| eyre!("Blockchain state not found: {}", name))
    }

    pub fn get_strategy(&self, name: Option<&String>) -> Result<&Strategy<DB, LoomDataTypesEthereum>> {
        let name = name.or_else(|| self.default_blockchain_name.as_ref())
            .ok_or_else(|| eyre!("No blockchain name provided and no default blockchain set"))?;
        self.strategies.get(name)
            .ok_or_else(|| eyre!("Strategy not found: {}", name))
    }

    pub fn get_signers(&self, name: Option<&String>) -> Result<SharedState<TxSigners>> {
        let name = name.or_else(|| self.default_signer_name.as_ref())
            .ok_or_else(|| eyre!("No signer name provided and no default signer set"))?;
        self.signers.get(name)
            .cloned()
            .ok_or_else(|| eyre!("Signers not found: {}", name))
    }

    pub fn get_blockchain_mut(&mut self, name: Option<&String>) -> Result<&mut Blockchain<LoomDataTypesEthereum>> {
        let name = name.or_else(|| self.default_blockchain_name.as_ref())
            .ok_or_else(|| eyre!("No blockchain name provided and no default blockchain set"))?;
        self.blockchains.get_mut(name)
            .ok_or_else(|| eyre!("Blockchain not found: {}", name))
    }

    pub fn get_client(&self, name: Option<&String>) -> Result<RootProvider<N>> {
        let name = name.or_else(|| self.default_client_name.as_ref())
            .ok_or_else(|| eyre!("No client name provided and no default client set"))?;
        self.clients.get(name)
            .cloned()
            .ok_or_else(|| eyre!("Client not found: {}", name))
    }

    pub fn get_multicaller_address(&self, name: Option<&String>) -> Result<Address> {
        let name = name.or_else(|| self.default_multicaller_encoder_name.as_ref())
            .ok_or_else(|| eyre!("No multicaller encoder name provided and no default multicaller encoder set"))?;
        self.multicaller_encoders.get(name)
            .cloned()
            .ok_or_else(|| eyre!("Multicaller address not found: {}", name))
    }

    pub fn get_client_config(&self, name: Option<&String>) -> Result<ClientConfig<P, N>> {
        let name = name.or_else(|| self.default_blockchain_name.as_ref())
            .ok_or_else(|| eyre!("No client name provided and no default client set"))?;
        self.config.clients.get(name)
            .map(|dc| dc.clone().into_client_config())
            .ok_or_else(|| eyre!("Client config not found: {}", name))
    }
}

impl<
        DB: Database<Error = ErrReport>
            + DatabaseRef<Error = ErrReport>
            + DatabaseCommit
            + DatabaseLoomExt
            + BlockHistoryState
            + Default
            + Send
            + Sync
            + Clone
            + 'static,
        E: SwapEncoder + Send + Sync + Clone + 'static,
        P: Provider<Ethereum> + Send + Sync + Clone + 'static,
    > Topology<DB, E, P, Ethereum>
{
    pub fn from_config(config: TopologyConfig) -> Topology<DB, MulticallerSwapEncoder> {
        let encoder = MulticallerSwapEncoder::default();
        let pool_loaders = Arc::new(PoolLoadersBuilder::<RootProvider>::new().build());

        Topology::<DB, MulticallerSwapEncoder> {
            config,
            clients: HashMap::new(),
            blockchains: HashMap::new(),
            blockchain_states: HashMap::new(),
            strategies: HashMap::new(),
            signers: HashMap::new(),
            multicaller_encoders: HashMap::new(),
            default_blockchain_name: None,
            default_client_name: None,
            default_multicaller_encoder_name: None,
            default_signer_name: None,
            swap_encoder: encoder,
            pool_loaders,
        }
    }

    pub fn with_swap_encoder<NE: SwapEncoder + Send + Sync + Clone + 'static>(
        self,
        swap_encoder: NE,
    ) -> Topology<DB, NE, P, Ethereum> {
        Topology {
            config: self.config,
            clients: self.clients,
            blockchains: self.blockchains,
            blockchain_states: self.blockchain_states,
            strategies: self.strategies,
            signers: self.signers,
            multicaller_encoders: self.multicaller_encoders,
            default_blockchain_name: self.default_blockchain_name,
            default_client_name: self.default_client_name,
            default_multicaller_encoder_name: self.default_multicaller_encoder_name,
            default_signer_name: self.default_signer_name,
            pool_loaders: self.pool_loaders,
            swap_encoder,
        }
    }

    pub fn with_pool_loaders<NP: Provider + Send + Sync + Clone + 'static>(
        self,
        pool_loaders: PoolLoaders<NP, Ethereum, LoomDataTypesEthereum>,
    ) -> Topology<DB, E, NP, Ethereum> {
        Topology {
            config: self.config,
            clients: self.clients,
            blockchains: self.blockchains,
            blockchain_states: self.blockchain_states,
            strategies: self.strategies,
            signers: self.signers,
            multicaller_encoders: self.multicaller_encoders,
            default_blockchain_name: self.default_blockchain_name,
            default_client_name: self.default_client_name,
            default_multicaller_encoder_name: self.default_multicaller_encoder_name,
            default_signer_name: self.default_signer_name,
            swap_encoder: self.swap_encoder,
            pool_loaders: Arc::new(pool_loaders),
        }
    }

    pub async fn start_clients(mut self) -> Result<Self> {
        let mut clients = HashMap::new();
        for (name, v) in self.config.clients.iter() {
            let config_params = v.clone();

            info!("Connecting to {name} : {v:?}");
            
            // First try to connect with WebSocket for better subscription support
            // If the URL is HTTP, try to convert it to WebSocket
            let ws_url = if config_params.transport == TransportType::Http && config_params.url.starts_with("http") {
                // Convert HTTP to WS
                let ws_url = config_params.url.replace("http://", "ws://").replace("https://", "wss://");
                Some(ws_url)
            } else if config_params.transport == TransportType::Ws {
                Some(config_params.url.clone())
            } else {
                None
            };
            
            // Try WebSocket first if available
            let mut client_result = None;
            if let Some(ws_url) = ws_url {
                info!("Attempting WebSocket connection to {name} at {ws_url}");
                let transport = WsConnect { url: ws_url, auth: None, config: None };
                let ws_client = ClientBuilder::default().ws(transport).await;
                if let Ok(client) = ws_client {
                    info!("Successfully connected to {name} via WebSocket (subscriptions supported)");
                    client_result = Some(Ok(client));
                } else {
                    info!("WebSocket connection failed, falling back to configured transport");
                }
            }
            // If WebSocket failed or wasn't attempted, use the configured transport
            if client_result.is_none() {
                client_result = Some(match config_params.transport {
                    TransportType::Ipc => {
                        info!("Starting IPC connection");
                        let transport = IpcConnect::from(config_params.url);
                        ClientBuilder::default().ipc(transport).await
                    }
                    TransportType::Http => {
                        info!("Starting HTTP connection (subscriptions not supported)");
                        let url = Url::parse(&config_params.url)?;
                        Ok(ClientBuilder::default().http(url))
                    }
                    TransportType::Ws => {
                        info!("Starting WS connection");
                        let transport = WsConnect { url: config_params.url, auth: None, config: None };
                        ClientBuilder::default().ws(transport).await
                    }
                });
            }
            let client = match client_result.unwrap() {
                Ok(client) => client,
                Err(e) => {
                    error!("Error connecting to {name} error : {}", e);
                    continue;
                }
            };
            let provider = ProviderBuilder::<_, _, Ethereum>::new().disable_recommended_fillers().on_client(client);
            clients.insert(name.clone(), provider);
        }
        
        // Initialize blockchains based on config
        let chain_id_map: HashMap<String, i64> = self.config.blockchains.iter()
            .map(|(name, config)| (name.clone(), config.chain_id.unwrap_or_default()))
            .collect();
        self.initialize_blockchains(&chain_id_map)?;
        info!("Initialized {} blockchains", self.blockchains.len());

        // Set default client name if not already set and clients are available
        if self.default_client_name.is_none() && !clients.is_empty() {
            self.default_client_name = Some(clients.keys().next().unwrap().clone());
            info!("Default client name set to {}", self.default_client_name.as_ref().unwrap());
        }
        
        Ok(Topology { clients, ..self })
    }

    pub async fn start_actors(&self) -> Result<Vec<JoinHandle<WorkerResult>>> {
        let mut tasks: Vec<JoinHandle<WorkerResult>> = Vec::new();

        if self.clients.is_empty() {
            return Err(eyre!("NO_CLIENTS_CONNECTED"));
        }

        for (k, _params) in self.config.blockchains.iter() {
            let blockchain = self.get_blockchain(Some(k))?;
            let blockchain_state = self.get_blockchain_state(Some(k))?;
            let client = self.get_client(None)?;

            #[cfg(feature = "loom-core-block-history-actor")]
            {
                info!("Starting block history actor {k}");
                let mut block_history_actor = BlockHistoryActor::new(client);
                match block_history_actor
                    .access(blockchain.latest_block())
                    .access(blockchain_state.market_state())
                    .access(blockchain_state.block_history())
                    .consume(blockchain.new_block_headers_channel())
                    .consume(blockchain.new_block_with_tx_channel())
                    .consume(blockchain.new_block_logs_channel())
                    .consume(blockchain.new_block_state_update_channel())
                    .produce(blockchain.market_events_channel())
                    .start()
                {
                    Ok(r) => {
                        tasks.extend(r);
                        info!("Block history actor started successfully");
                    }
                    Err(e) => {
                        panic!("{}", e)
                    }
                }
            }

            info!("Starting mempool actor {k}");
            let mut mempool_actor = MempoolActor::new();
            match mempool_actor
                .access(blockchain.mempool())
                .consume(blockchain.new_mempool_tx_channel())
                .consume(blockchain.new_block_headers_channel())
                .consume(blockchain.new_block_with_tx_channel())
                .produce(blockchain.mempool_events_channel())
                .produce(blockchain.influxdb_write_channel())
                .start()
            {
                Ok(r) => {
                    tasks.extend(r);
                    info!("Mempool actor started successfully");
                }
                Err(e) => {
                    panic!("{}", e)
                }
            }

            info!("Starting pool health monitor actor {k}");
            let mut new_pool_health_monitor_actor = PoolHealthMonitorActor::new();
            match new_pool_health_monitor_actor
                .access(blockchain.market())
                .consume(blockchain.health_monitor_channel())
                .produce(blockchain.influxdb_write_channel())
                .start()
            {
                Ok(r) => {
                    tasks.extend(r);
                    info!("Pool monitor monitor actor started");
                }
                Err(e) => {
                    panic!("PoolHealthMonitorActor error {}", e)
                }
            }
        }

        for (name, params) in self.config.signers.iter() {
            let signers = self.get_signers(Some(name))?;
            match params {
                SignersConfig::Env(params) => {
                    info!("Starting initialize env signers actor {name}");
                    let blockchain = self.get_blockchain(params.blockchain.as_ref())?;

                    let initialize_signers_actor = InitializeSignersOneShotBlockingActor::new_from_encrypted_env();
                    match initialize_signers_actor?.access(signers.clone()).access(blockchain.nonce_and_balance()).start_and_wait() {
                        Ok(_) => {
                            info!("Signers have been initialized");
                        }
                        Err(e) => {
                            panic!("Cannot initialize signers {}", e);
                        }
                    }

                    let mut signers_actor = TxSignersActor::<LoomDataTypesEthereum>::new();
                    match signers_actor.consume(blockchain.tx_compose_channel()).produce(blockchain.tx_compose_channel()).start() {
                        Ok(r) => {
                            tasks.extend(r);
                            info!("Signers actor has been started");
                        }
                        Err(e) => {
                            panic!("Cannot start signers actor {}", e)
                        }
                    }
                }
            }
        }

        if let Some(preloader_actors) = &self.config.preloaders {
            for (name, params) in preloader_actors {
                info!("Starting market state preload actor {name}");

                let blockchain_state = self.get_blockchain_state(params.blockchain.as_ref())?;
                let client = self.get_client(params.client.as_ref())?;
                let signers = self.get_signers(params.signers.as_ref())?;

                let mut market_state_preload_actor = MarketStatePreloadedOneShotActor::new(client)
                    .with_signers(signers.clone())
                    .with_copied_account(self.get_multicaller_address(None)?);
                match market_state_preload_actor.access(blockchain_state.market_state()).start_and_wait() {
                    Ok(_) => {
                        info!("Market state preload actor executed successfully");
                    }
                    Err(e) => {
                        panic!("MarketStatePreloadedOneShotActor : {}", e)
                    }
                }
            }
        } else {
            info!("No preloader in config, creating default preloader");
            // Create a default preloader for each blockchain
            for (blockchain_name, _) in self.config.blockchains.iter() {
                let blockchain = self.get_blockchain(Some(blockchain_name))?;
                let blockchain_state = self.get_blockchain_state(Some(blockchain_name))?;
                let client = self.get_client(None)?;
                info!("Creating default preloader for blockchain {}", blockchain_name);
                let mut market_state_preload_actor = MarketStatePreloadedOneShotActor::new(client.clone());
                // Try to get a multicaller address, but don't fail if not available
                if let Ok(multicaller_address) = self.get_multicaller_address(None) {
                    market_state_preload_actor = market_state_preload_actor.with_copied_account(multicaller_address);
                }
                match market_state_preload_actor
                    .access(blockchain_state.market_state())
                    .start_and_wait()
                {
                    Ok(_) => {
                        info!("Default preloader for {} completed successfully", blockchain_name);
                    }
                    Err(e) => {
                        warn!("Default preloader for {} failed: {}", blockchain_name, e);
                        // Don't panic, just warn and continue
                    }
                }
            }
        }

        if let Some(node_exex_actors) = &self.config.actors.node_exex {
            for (name, params) in node_exex_actors {
                let blockchain = self.get_blockchain(params.blockchain.as_ref())?;
                let url = params.url.clone().unwrap_or("http://[::1]:10000".to_string());
                info!("Starting node actor {name}");
                let mut node_exex_block_actor = NodeExExGrpcActor::new(url);
                match node_exex_block_actor
                    .produce(blockchain.new_block_headers_channel())
                    .produce(blockchain.new_block_with_tx_channel())
                    .produce(blockchain.new_block_logs_channel())
                    .produce(blockchain.new_block_state_update_channel())
                    .produce(blockchain.new_mempool_tx_channel())
                    .start()
                {
                    Ok(r) => {
                        tasks.extend(r);
                        info!("Node ExEx actor started successfully for : {} @ {}", name, blockchain.chain_id());
                    }
                    Err(e) => {
                        panic!("{}", e)
                    }
                }
            }
        }

        if let Some(node_block_actors) = &self.config.actors.node {
            for (name, params) in node_block_actors {
                let client = self.get_client(params.client.as_ref())?;
                let blockchain = self.get_blockchain(params.blockchain.as_ref())?;
                let client_config = self.get_client_config(params.client.as_ref())?;
                info!("Starting node actor {name}");
                #[cfg(feature = "db-access")]
                if client_config.db_path.is_some() {
                    let mut node_block_actor = RethDbAccessBlockActor::new(
                        client.clone(),
                        NodeBlockActorConfig::all_enabled(),
                        client_config.db_path.clone().unwrap_or_default(),
                    );
                    match node_block_actor
                        .produce(blockchain.new_block_headers_channel())
                        .produce(blockchain.new_block_with_tx_channel())
                        .produce(blockchain.new_block_logs_channel())
                        .produce(blockchain.new_block_state_update_channel())
                        .start()
                    {
                        Ok(r) => {
                            tasks.extend(r);
                            info!("Reth db access node actor started successfully for : {} @ {}", name, blockchain.chain_id());
                        }
                        Err(e) => {
                            panic!("{}", e)
                        }
                    }
                }
                if client_config.db_path.is_none() {
                    let mut node_block_actor = NodeBlockActor::new(client, NodeBlockActorConfig::all_enabled());
                    match node_block_actor
                        .produce(blockchain.new_block_headers_channel())
                        .produce(blockchain.new_block_with_tx_channel())
                        .produce(blockchain.new_block_logs_channel())
                        .produce(blockchain.new_block_state_update_channel())
                        .start()
                    {
                        Ok(r) => {
                            tasks.extend(r);
                            info!("Node actor started successfully for : {} @ {}", name, blockchain.chain_id());
                        }
                        Err(e) => {
                            panic!("{}", e)
                        }
                    }
                }
            }
        }

        if let Some(node_mempool_actors) = &self.config.actors.mempool {
            for (name, params) in node_mempool_actors {
                let blockchain = self.get_blockchain(params.blockchain.as_ref())?;
                match self.get_client(params.client.as_ref()) {
                    Ok(client) => {
                        info!("Starting node mempool actor {name}");
                        let mut node_mempool_actor = NodeMempoolActor::new(client).with_name(name.clone());
                        match node_mempool_actor.produce(blockchain.new_mempool_tx_channel()).start() {
                            Ok(r) => {
                                tasks.extend(r);
                                info!("Node mempool actor started successfully {name}");
                            }
                            Err(e) => {
                                panic!("{}", e)
                            }
                        }
                    }
                    Err(e) => {
                        error!("Skipping mempool actor for {} @ {} : {}", name, blockchain.chain_id(), e)
                    }
                }
            }
        }

        if let Some(price_actors) = &self.config.actors.price {
            for (name, c) in price_actors {
                let client = self.get_client(c.client.as_ref())?;
                let blockchain = self.get_blockchain(c.blockchain.as_ref())?;
                info!("Starting price actor");
                let mut price_actor = PriceActor::new(client);
                match price_actor.access(blockchain.market()).start() {
                    Ok(r) => {
                        tasks.extend(r);
                        info!("Price actor has been initialized : {}", name);
                    }
                    Err(e) => {
                        panic!("Cannot initialize price actor {} : {}", name, e);
                    }
                }
            }
        } else {
            warn!("No price actor in config");
        }

        if let Some(node_balance_actors) = &self.config.actors.noncebalance {
            for (name, c) in node_balance_actors {
                let client = self.get_client(c.client.as_ref())?;
                let blockchain = self.get_blockchain(c.blockchain.as_ref())?;
                info!("Starting nonce and balance monitor actor {name}");
                let mut nonce_and_balance_monitor = NonceAndBalanceMonitorActor::new(client);
                match nonce_and_balance_monitor
                    .access(blockchain.nonce_and_balance())
                    .access(blockchain.latest_block())
                    .consume(blockchain.market_events_channel())
                    .start()
                {
                    Ok(r) => {
                        tasks.extend(r);
                        info!("Nonce monitor has been initialized {name} for {}", blockchain.chain_id());
                    }
                    Err(e) => {
                        panic!("Cannot initialize nonce and balance monitor {} : {}", name, e);
                    }
                }
            }
        } else {
            warn!("No nonce and balance actors in config");
        }

        if let Some(broadcaster_actors) = &self.config.actors.broadcaster {
            for (name, params) in broadcaster_actors {
                match params {
                    BroadcasterConfig::Flashbots(params) => {
                        let client = self.get_client(params.client.as_ref())?;
                        let blockchain = self.get_blockchain(params.blockchain.as_ref())?;
                        let flashbots_client = Flashbots::new(client, "https://relay.flashbots.net", None).with_default_relays();
                        let mut flashbots_actor = FlashbotsBroadcastActor::new(flashbots_client.into(), true);
                        match flashbots_actor.consume(blockchain.tx_compose_channel()).start() {
                            Ok(r) => {
                                tasks.extend(r);
                                info!("Flashbots broadcaster actor {name} started successfully for {}", blockchain.chain_id());
                            }
                            Err(e) => {
                                panic!("Error starting flashbots broadcaster actor {name} for {} : {}", blockchain.chain_id(), e)
                            }
                        }
                    }
                }
            }
        } else {
            warn!("No broadcaster actors in config");
        }

        if let Some(pool_actors) = &self.config.actors.pools {
            let mut blockchains = HashMap::new();
            for (name, params) in pool_actors {
                let client = self.get_client(params.client.as_ref())?;
                let blockchain = self.get_blockchain(params.blockchain.as_ref())?;
                let blockchain_state = self.get_blockchain_state(params.blockchain.as_ref())?;
                let pool_loaders = self.pool_loaders.clone();
                blockchains.insert(blockchain.chain_id(), blockchain);
                if params.history {
                    info!("Starting history pools loader {name}");
                    let mut history_pools_loader_actor = HistoryPoolLoaderOneShotActor::new(client.clone(), pool_loaders.clone());
                    match history_pools_loader_actor.produce(blockchain.tasks_channel()).start() {
                        Ok(r) => {
                            tasks.extend(r);
                            info!("History pool loader actor started successfully {name}");
                        }
                        Err(e) => {
                            panic!("HistoryPoolLoaderOneShotActor : {}", e)
                        }
                    }
                }
                if params.protocol {
                    info!("Starting protocol pools loader {name}");
                    let mut protocol_pools_loader_actor = ProtocolPoolLoaderOneShotActor::new(client.clone(), pool_loaders.clone());
                    match protocol_pools_loader_actor.produce(blockchain.tasks_channel()).start() {
                        Ok(r) => {
                            tasks.extend(r);
                            info!("Protocol pool loader actor started successfully {name}");
                        }
                        Err(e) => {
                            panic!("ProtocolPoolLoaderOneShotActor : {}", e)
                        }
                    }
                }
                if params.new {
                    info!("Starting new pool loader actor {name}");
                    let mut new_pool_actor = NewPoolLoaderActor::new(pool_loaders.clone());
                    match new_pool_actor.consume(blockchain.new_block_logs_channel()).produce(blockchain.tasks_channel()).start() {
                        Ok(r) => {
                            tasks.extend(r);
                            info!("New pool actor started successfully {name}");
                        }
                        Err(e) => {
                            panic!("NewPoolLoaderActor : {}", e)
                        }
                    }
                }
                info!("Starting pool loader actor {name}");
                let mut pool_loader_actor = PoolLoaderActor::new(client.clone(), pool_loaders.clone(), PoolsLoadingConfig::new());
                match pool_loader_actor
                    .access(blockchain.market())
                    .access(blockchain_state.market_state())
                    .consume(blockchain.tasks_channel())
                    .produce(blockchain.market_events_channel())
                    .start()
                {
                    Ok(r) => {
                        tasks.extend(r);
                        info!("Pool loader actor started successfully {name}");
                    }
                    Err(e) => {
                        panic!("PoolLoaderActor : {}", e)
                    }
                }
            }
        } else {
            warn!("No pool loader actors in config");
        }

        if let Some(estimator_actors) = &self.config.actors.estimator {
            for (name, params) in estimator_actors {
                match params {
                    EstimatorConfig::Evm(params) => {
                        let client = params.client.as_ref().map(|x| self.get_client(Some(x))).transpose()?;
                        let blockchain = self.get_blockchain(params.blockchain.as_ref())?;
                        let strategy = self.get_strategy(params.blockchain.as_ref())?;
                        let multicaller_address = self.get_multicaller_address(params.encoder.as_ref())?;
                        let mut encoder = self.swap_encoder.clone();
                        encoder.set_address(multicaller_address);
                        let mut evm_estimator_actor = EvmEstimatorActor::new_with_provider(encoder, client);
                        match evm_estimator_actor
                            .consume(strategy.swap_compose_channel())
                            .produce(strategy.swap_compose_channel())
                            .produce(blockchain.health_monitor_channel())
                            .produce(blockchain.influxdb_write_channel())
                            .start()
                        {
                            Ok(r) => {
                                tasks.extend(r);
                                info!("EVM estimator actor started successfully {name} @ {}", blockchain.chain_id());
                            }
                            Err(e) => {
                                panic!("Error starting EVM estimator actor {name} @ {} : {}", blockchain.chain_id(), e)
                            }
                        }
                    }
                    EstimatorConfig::Geth(params) => {
                        let client = self.get_client(params.client.as_ref())?;
                        let blockchain = self.get_blockchain(params.blockchain.as_ref())?;
                        let strategy = self.get_strategy(params.blockchain.as_ref())?;
                        let multicaller_address = self.get_multicaller_address(params.encoder.as_ref())?;
                        let mut encoder = self.swap_encoder.clone();
                        encoder.set_address(multicaller_address);
                        let flashbots_client = Arc::new(Flashbots::new(client, "https://relay.flashbots.net", None).with_default_relays());
                        let mut geth_estimator_actor = GethEstimatorActor::new(flashbots_client, encoder);
                        match geth_estimator_actor.consume(strategy.swap_compose_channel()).produce(strategy.swap_compose_channel()).start() {
                            Ok(r) => {
                                tasks.extend(r);
                                info!("Geth estimator actor started successfully {name} @ {}", blockchain.chain_id());
                            }
                            Err(e) => {
                                panic!("Error starting Geth estimator actor for {name} @ {} : {}", blockchain.chain_id(), e)
                            }
                        }
                    }
                }
            }
        } else {
            warn!("No estimator actors in config");
        }

        Ok(tasks)
    }
}

            