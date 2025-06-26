
use alloy_network::Ethereum;
use alloy_primitives::{Address, B256, U256};
use alloy_provider::{Provider, RootProvider};
use axum::Router;
use eyre::{eyre, ErrReport, Result};
use loom_broadcast_accounts::{InitializeSignersOneShotBlockingActor, NonceAndBalanceMonitorActor, TxSignersActor};
use loom_broadcast_broadcaster::FlashbotsBroadcastActor;
use loom_broadcast_flashbots::client::RelayConfig;
use loom_broadcast_flashbots::Flashbots;
use loom_core_actors::{Actor, ActorsManager, SharedState};
use loom_core_block_history_actor::BlockHistoryActor;
use loom_core_blockchain::{Blockchain, BlockchainState, Strategy};
use loom_core_mempool::MempoolActor;
use loom_core_router::SwapRouterActor;
use loom_defi_address_book::TokenAddressEth;
use loom_defi_health_monitor::{MetricsRecorderActor, PoolHealthMonitorActor, StuffingTxMonitorActor};
use loom_defi_market::{
    HistoryPoolLoaderOneShotActor, NewPoolLoaderActor, PoolLoaderActor, ProtocolPoolLoaderOneShotActor, RequiredPoolLoaderActor,
};
use loom_defi_pools::{PoolLoadersBuilder, PoolsLoadingConfig};
use loom_defi_preloader::MarketStatePreloadedOneShotActor;
use loom_defi_price::PriceActor;
use loom_evm_db::DatabaseLoomExt;
use loom_evm_utils::NWETH;
use loom_execution_estimator::{EvmEstimatorActor, GethEstimatorActor};
use loom_execution_multicaller::MulticallerSwapEncoder;
use loom_metrics::InfluxDbWriterActor;
use loom_node_actor_config::NodeBlockActorConfig;
#[cfg(feature = "db-access")]
use loom_node_db_access::RethDbAccessBlockActor;
use loom_node_debug_provider::DebugProviderExt;
use loom_node_grpc::NodeExExGrpcActor;
use loom_node_json_rpc::{NodeBlockActor, NodeMempoolActor, WaitForNodeSyncOneShotBlockingActor};
use loom_rpc_handler::WebServerActor;
use loom_storage_db::DbPool;
use loom_strategy_backrun::{
    BackrunConfig, BlockStateChangeProcessorActor, PendingTxStateChangeProcessorActor, StateChangeArbSearcherActor,
};
use loom_strategy_merger::{ArbSwapPathMergerActor, DiffPathMergerActor, SamePathMergerActor};
use loom_types_entities::required_state::RequiredState;
use loom_types_entities::{BlockHistoryState, PoolClass, SwapEncoder, TxSigners};
use loom_types_blockchain::loom_data_types_ethereum::LoomDataTypesEthereum;
use revm::{Database, DatabaseCommit, DatabaseRef};
use std::collections::HashMap;
use std::sync::Arc;
use tokio_util::sync::CancellationToken;

pub struct BlockchainActors<P, DB: Clone + Send + Sync + 'static, E: Clone = MulticallerSwapEncoder> {
    provider: P,
    bc: Blockchain,
    state: BlockchainState<DB>,
    strategy: Strategy<DB>,
    pub signers: SharedState<TxSigners>,
    actor_manager: ActorsManager,
    encoder: Option<E>,
    has_mempool: bool,
    has_state_update: bool,
    has_signers: bool,
    mutlicaller_address: Option<Address>,
    relays: Vec<RelayConfig>,
}

impl<P, DB, E> BlockchainActors<P, DB, E>
where
    P: Provider<Ethereum> + DebugProviderExt<Ethereum> + Send + Sync + Clone + 'static,
    DB: DatabaseRef<Error = ErrReport>
        + Database<Error = ErrReport>
        + DatabaseCommit
        + DatabaseLoomExt
        + BlockHistoryState
        + Send
        + Sync
        + Clone
        + Default
        + 'static,
    E: SwapEncoder + Send + Sync + Clone + 'static,
{
    pub fn new(
        provider: P,
        encoder: E,
        bc: Blockchain,
        state: BlockchainState<DB>,
        strategy: Strategy<DB>,
        relays: Vec<RelayConfig>,
    ) -> Self {
        Self {
            provider,
            bc,
            state,
            strategy,
            signers: SharedState::new(TxSigners::new()),
            actor_manager: ActorsManager::new(),
            encoder: Some(encoder),
            has_mempool: false,
            has_state_update: false,
            has_signers: false,
            mutlicaller_address: None,
            relays,
        }
    }

    pub async fn wait(self) {
        self.actor_manager.wait().await
    }

    /// Start a custom actor
    pub fn start(&mut self, actor: impl Actor + 'static) -> Result<&mut Self> {
        self.actor_manager.start(actor)?;
        Ok(self)
    }

    /// Start a custom actor and wait for it to finish
    pub fn start_and_wait(&mut self, actor: impl Actor + Send + Sync + 'static) -> Result<&mut Self> {
        self.actor_manager.start_and_wait(actor)?;
        Ok(self)
    }

    /// Initialize signers with the default anvil Private Key
    pub fn initialize_signers_with_anvil(&mut self) -> Result<&mut Self> {
        let key: B256 = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80".parse()?;

        self.actor_manager.start_and_wait(
            InitializeSignersOneShotBlockingActor::new(Some(key.to_vec())).with_signers(self.signers.clone()),
        )?;
        self.with_signers()?;
        Ok(self)
    }

    /// Initialize signers with the private key. Random key generated if param in None
    pub fn initialize_signers_with_key(&mut self, key: Option<Vec<u8>>) -> Result<&mut Self> {
        self.actor_manager
            .start_and_wait(InitializeSignersOneShotBlockingActor::new(key).with_signers(self.signers.clone()))?;
        self.with_signers()?;
        Ok(self)
    }

    /// Initialize signers with multiple private keys
    pub fn initialize_signers_with_keys(&mut self, keys: Vec<Vec<u8>>) -> Result<&mut Self> {
        for key in keys {
            self.actor_manager
                .start_and_wait(InitializeSignersOneShotBlockingActor::new(Some(key)).with_signers(self.signers.clone()))?;
        }
        self.with_signers()?;
        Ok(self)
    }

    /// Initialize signers with encrypted private key
    pub fn initialize_signers_with_encrypted_key(&mut self, key: Vec<u8>) -> Result<&mut Self> {
        self.actor_manager.start_and_wait(
            InitializeSignersOneShotBlockingActor::new_from_encrypted_key(key)?.with_signers(self.signers.clone()),
        )?;
        self.with_signers()?;
        Ok(self)
    }

    /// Initializes signers with encrypted key form DATA env var
    pub fn initialize_signers_with_env(&mut self) -> Result<&mut Self> {
        self.actor_manager.start_and_wait(
            InitializeSignersOneShotBlockingActor::new_from_encrypted_env()?.with_signers(self.signers.clone()),
        )?;
        self.with_signers()?;
        Ok(self)
    }

    /// Starts signer actor
    pub fn with_signers(&mut self) -> Result<&mut Self> {
        if !self.has_signers {
            self.has_signers = true;
            let signers = self.signers.clone();
            self.actor_manager.start(move || Box::new(TxSignersActor::<LoomDataTypesEthereum>::new().with_signers(signers)))?;
        }
        Ok(self)
    }

    /// Initializes encoder and start encoder actor
    pub fn with_swap_encoder(&mut self, swap_encoder: E) -> Result<&mut Self> {
        self.mutlicaller_address = Some(swap_encoder.address());
        self.encoder = Some(swap_encoder);
        let signers = self.signers.clone();
        let bc = self.bc.clone();
        let strategy = self.strategy.clone();
        self.actor_manager.start(move || Box::new(SwapRouterActor::<DB>::new().with_signers(signers).on_bc(&bc, &strategy)))?;
        Ok(self)
    }

    /// Starts market state preloader
    pub fn with_market_state_preloader(&mut self) -> Result<&mut Self> {
        let mut address_vec = self.signers.inner().try_read()?.get_address_vec();

        if let Some(loom_multicaller) = self.mutlicaller_address {
            address_vec.push(loom_multicaller);
        }

        let provider = self.provider.clone();
        let bc = self.bc.clone();
        let state = self.state.clone();

        self.actor_manager.start_and_wait(move || Box::new(MarketStatePreloadedOneShotActor::new(provider).with_copied_accounts(address_vec).on_bc(&bc, &state)))?;
        Ok(self)
    }

    /// Starts preloaded virtual artefacts
    pub fn with_market_state_preloader_virtual(&mut self, address_to_copy: Vec<Address>) -> Result<&mut Self> {
        let address_vec = self.signers.inner().try_read()?.get_address_vec();

        let mut market_state_preloader = MarketStatePreloadedOneShotActor::new(self.provider.clone());

        for address in address_vec {
            //            market_state_preloader = market_state_preloader.with_new_account(address, 0, NWETH::from_float(10.0), None);
            market_state_preloader = market_state_preloader.with_copied_account(address).with_token_balance(
                TokenAddressEth::ETH_NATIVE,
                address,
                NWETH::from_float(10.0),
            );
        }

        market_state_preloader = market_state_preloader.with_copied_accounts(address_to_copy);

        market_state_preloader = market_state_preloader.with_new_account(
            loom_execution_multicaller::DEFAULT_VIRTUAL_ADDRESS,
            0,
            U256::ZERO,
            loom_execution_multicaller::MulticallerDeployer::new().account_info().code,
        );

        market_state_preloader = market_state_preloader.with_token_balance(
            TokenAddressEth::WETH,
            loom_execution_multicaller::DEFAULT_VIRTUAL_ADDRESS,
            NWETH::from_float(10.0),
        );

        self.mutlicaller_address = Some(loom_execution_multicaller::DEFAULT_VIRTUAL_ADDRESS);

        let bc = self.bc.clone();
        let state = self.state.clone();
        self.actor_manager.start_and_wait(move || Box::new(market_state_preloader.on_bc(&bc, &state)))?;
        Ok(self)
    }

    /// Starts nonce and balance monitor
    pub fn with_nonce_and_balance_monitor(&mut self) -> Result<&mut Self> {
        let provider = self.provider.clone();
        self.actor_manager.start(move || Box::new(NonceAndBalanceMonitorActor::new(provider)))?;
        Ok(self)
    }

    pub fn with_nonce_and_balance_monitor_only_events(&mut self) -> Result<&mut Self> {
        let provider = self.provider.clone();
        self.actor_manager.start(move || Box::new(NonceAndBalanceMonitorActor::new(provider).only_once()))?;
        Ok(self)
    }

    /// Starts block history actor
    pub fn with_block_history(&mut self) -> Result<&mut Self> {
        let provider = self.provider.clone();
        let bc = self.bc.clone();
        let state = self.state.clone();
        self.actor_manager.start(move || Box::new(BlockHistoryActor::new(provider).on_bc(&bc, &state)))?;
        Ok(self)
    }

    /// Starts token price calculator
    pub fn with_price_station(&mut self) -> Result<&mut Self> {
        let provider = self.provider.clone();
        let bc = self.bc.clone();
        self.actor_manager.start(move || Box::new(PriceActor::new(provider).on_bc(&bc)))?;
        Ok(self)
    }

    /// Starts receiving blocks events through RPC
    pub fn with_block_events(&mut self, config: NodeBlockActorConfig) -> Result<&mut Self> {
        let provider = self.provider.clone();
        let bc = self.bc.clone();
        self.actor_manager.start(move || Box::new(NodeBlockActor::new(provider, config).on_bc(&bc)))?;
        Ok(self)
    }

    /// Starts receiving blocks events through direct Reth DB access
    #[cfg(feature = "db-access")]
    pub fn reth_node_with_blocks(&mut self, db_path: String, config: NodeBlockActorConfig) -> Result<&mut Self> {
        let provider = self.provider.clone();
        let bc = self.bc.clone();
        self.actor_manager.start(move || Box::new(RethDbAccessBlockActor::new(provider, config, db_path).on_bc(&bc)))?;
        Ok(self)
    }

    /// Starts receiving blocks and mempool events through ExEx GRPC
    pub fn with_exex_events(&mut self) -> Result<&mut Self> {
        self.mempool()?;
        let bc = self.bc.clone();
        self.actor_manager.start(move || Box::new(NodeExExGrpcActor::new("http://[::1]:10000".to_string()).on_bc(&bc)))?;
        Ok(self)
    }

    /// Starts mempool actor collecting pending txes from all mempools and pulling new tx hashes in mempool_events channel
    pub fn mempool(&mut self) -> Result<&mut Self> {
        if !self.has_mempool {
            self.has_mempool = true;
            let bc = self.bc.clone();
            self.actor_manager.start(move || Box::new(MempoolActor::new().on_bc(&bc)))?;
        }
        Ok(self)
    }

    /// Starts local node pending tx provider
    pub fn with_local_mempool_events(&mut self) -> Result<&mut Self> {
        self.mempool()?;
        let provider = self.provider.clone();
        let bc = self.bc.clone();
        self.actor_manager.start(move || Box::new(NodeMempoolActor::new(provider).on_bc(&bc)))?;
        Ok(self)
    }

    /// Starts remote node pending tx provider
    pub fn with_remote_mempool<PM>(&mut self, provider: PM) -> Result<&mut Self>
    where
        PM: Provider<Ethereum> + Send + Sync + Clone + 'static,
    {
        self.mempool()?;
        let bc = self.bc.clone();
        self.actor_manager.start(move || Box::new(NodeMempoolActor::new(provider).on_bc(&bc)))?;
        Ok(self)
    }

    /// Starts flashbots broadcaster
    pub fn with_flashbots_broadcaster(&mut self, allow_broadcast: bool) -> Result<&mut Self> {
        let provider = self.provider.clone();
        let relays = self.relays.clone();
        let flashbots = match relays.is_empty() {
            true => Flashbots::new(provider.clone(), "https://relay.flashbots.net", None).with_default_relays(),
            false => Flashbots::new(provider.clone(), "https://relay.flashbots.net", None).with_relays(relays),
        };

        self.actor_manager.start(move || Box::new(FlashbotsBroadcastActor::new(flashbots, allow_broadcast)))?;
        Ok(self)
    }

    /// Start composer : estimator, signer and broadcaster
    pub fn with_composers(&mut self, allow_broadcast: bool) -> Result<&mut Self> {
        self.with_evm_estimator()?.with_signers()?.with_flashbots_broadcaster(allow_broadcast)
    }

    /// Starts pool health monitor
    pub fn with_health_monitor_pools(&mut self) -> Result<&mut Self> {
        let bc = self.bc.clone();
        self.actor_manager.start(move || Box::new(PoolHealthMonitorActor::new().on_bc(&bc)))?;
        Ok(self)
    }

    //TODO : Move out of Blockchain
    /*
    /// Starts state health monitor
    pub fn with_health_monitor_state(&mut self) -> Result<&mut Self> {
        self.actor_manager.start(|| Box::new(StateHealthMonitorActor::new(self.provider.clone()).on_bc(&self.bc)))?;
        Ok(self)
    }

     */

    /// Starts stuffing tx monitor
    pub fn with_health_monitor_stuffing_tx(&mut self) -> Result<&mut Self> {
        let provider = self.provider.clone();
        let bc = self.bc.clone();
        self.actor_manager.start(move || Box::new(StuffingTxMonitorActor::new(provider).on_bc(&bc)))?;
        Ok(self)
    }

    /// Start pool loader from new block events
    pub fn with_new_pool_loader(&mut self, pools_config: PoolsLoadingConfig) -> Result<&mut Self> {
        let pool_loader = Arc::new(PoolLoadersBuilder::default_pool_loaders(self.provider.clone(), pools_config));
        let pool_loader_clone = pool_loader.clone();
        let bc = self.bc.clone();
        self.actor_manager.start(move || Box::new(NewPoolLoaderActor::new(pool_loader_clone).on_bc(&bc)))?;
        Ok(self)
    }

    /// Start pool loader for last 10000 blocks
    pub fn with_pool_history_loader(&mut self, pools_config: PoolsLoadingConfig) -> Result<&mut Self> {
        let pool_loaders = Arc::new(PoolLoadersBuilder::default_pool_loaders(self.provider.clone(), pools_config));
        let provider = self.provider.clone();
        let pool_loaders_clone = pool_loaders.clone();
        let bc = self.bc.clone();
        self.actor_manager.start(move || Box::new(HistoryPoolLoaderOneShotActor::new(provider.clone(), pool_loaders_clone.clone()).on_bc(&bc)))?;
        Ok(self)
    }

    /// Start pool loader from new block events
    pub fn with_pool_loader(&mut self, pools_config: PoolsLoadingConfig) -> Result<&mut Self> {
        let pool_loaders = Arc::new(PoolLoadersBuilder::default_pool_loaders(self.provider.clone(), pools_config.clone()));
        let provider = self.provider.clone();
        let pool_loaders_clone = pool_loaders.clone();
        let pools_config_clone = pools_config.clone();
        let bc = self.bc.clone();
        let state = self.state.clone();
        self.actor_manager.start(move || Box::new(PoolLoaderActor::new(provider.clone(), pool_loaders_clone.clone(), pools_config_clone.clone()).on_bc(&bc, &state)))?;
        Ok(self)
    }

    /// Start pool loader for curve + steth + wsteth
    pub fn with_curve_pool_protocol_loader(&mut self, pools_config: PoolsLoadingConfig) -> Result<&mut Self> {
        let pool_loaders = Arc::new(PoolLoadersBuilder::default_pool_loaders(self.provider.clone(), pools_config));
        let pool_loaders_clone = pool_loaders.clone();
        let provider = self.provider.clone();
        let bc = self.bc.clone();
        self.actor_manager.start(move || {
            let pool_loaders = pool_loaders_clone.clone();
            Box::new(ProtocolPoolLoaderOneShotActor::new(provider, pool_loaders).on_bc(&bc))
        })?;
        Ok(self)
    }

    /// Start all pool loaders
    pub fn with_pool_loaders(&mut self, pools_config: PoolsLoadingConfig) -> Result<&mut Self> {
        if pools_config.is_enabled(PoolClass::Curve) {
            self.with_new_pool_loader(pools_config.clone())?
                .with_pool_history_loader(pools_config.clone())?
                .with_curve_pool_protocol_loader(pools_config.clone())?
                .with_pool_loader(pools_config)
        } else {
            self.with_new_pool_loader(pools_config.clone())?.with_pool_history_loader(pools_config.clone())?.with_pool_loader(pools_config)
        }
    }

    //
    pub fn with_preloaded_state(&mut self, pools: Vec<(Address, PoolClass)>, state_required: Option<RequiredState>) -> Result<&mut Self> {
        let pool_loaders = Arc::new(PoolLoadersBuilder::default_pool_loaders(self.provider.clone(), PoolsLoadingConfig::default()));
        let mut actor = RequiredPoolLoaderActor::new(self.provider.clone(), pool_loaders);

        for (pool_address, pool_class) in pools {
            actor = actor.with_pool_address(pool_address, pool_class);
        }

        if let Some(state_required) = state_required {
            actor = actor.with_required_state(state_required);
        }

        let bc = self.bc.clone();
        let state = self.state.clone();
        self.actor_manager.start_and_wait(move || Box::new(actor.on_bc(&bc, &state)))?;
        Ok(self)
    }

    pub fn with_geth_estimator(&mut self) -> Result<&mut Self> {
        let flashbots = Flashbots::new(self.provider.clone(), "https://relay.flashbots.net", None).with_default_relays();
        let strategy = self.strategy.clone();
        let encoder = self.encoder.clone().unwrap();
        self.actor_manager.start(move || Box::new(GethEstimatorActor::new(Arc::new(flashbots), encoder).on_bc(&strategy)))?;
        Ok(self)
    }

    /// Starts EVM gas estimator and tips filler
    pub fn with_evm_estimator(&mut self) -> Result<&mut Self> {
        let bc = self.bc.clone();
        let strategy = self.strategy.clone();
        let encoder = self.encoder.clone().unwrap();
        self.actor_manager.start(move || Box::new(
            EvmEstimatorActor::<RootProvider, Ethereum, E, DB>::new(encoder).on_bc(&bc, &strategy),
        ))?;
        Ok(self)
    }

    /// Starts EVM gas estimator and tips filler
    pub fn with_evm_estimator_and_provider(&mut self) -> Result<&mut Self> {
        let bc = self.bc.clone();
        let strategy = self.strategy.clone();
        let encoder = self.encoder.clone().unwrap();
        let provider = self.provider.clone();
        self.actor_manager.start(move || Box::new(
            EvmEstimatorActor::new_with_provider(encoder, Some(provider))
                .on_bc(&bc, &strategy),
        ))?;
        Ok(self)
    }

    /// Start swap path merger
    pub fn with_swap_path_merger(&mut self) -> Result<&mut Self> {
        let mutlicaller_address = self.encoder.clone().ok_or(eyre!("NO_ENCODER"))?.address();
        let bc = self.bc.clone();
        let strategy = self.strategy.clone();

        self.actor_manager.start(move || Box::new(ArbSwapPathMergerActor::new(mutlicaller_address).on_bc(&bc, &strategy)))?;
        Ok(self)
    }

    /// Start same path merger
    pub fn with_same_path_merger(&mut self) -> Result<&mut Self> {
        let provider = self.provider.clone();
        let bc = self.bc.clone();
        let state = self.state.clone();
        let strategy = self.strategy.clone();
        let provider2 = provider.clone();
        let bc2 = bc.clone();
        let state2 = state.clone();
        let strategy2 = strategy.clone();
        self.actor_manager.start(move || Box::new(SamePathMergerActor::new(provider.clone()).on_bc(&bc, &state, &strategy)))?;
        self.actor_manager.start(move || Box::new(SamePathMergerActor::new(provider2.clone()).on_bc(&bc2, &state2, &strategy2)))?;
        Ok(self)
    }

    /// Start diff path merger
    pub fn with_diff_path_merger(&mut self) -> Result<&mut Self> {
        let bc = self.bc.clone();
        self.actor_manager.start(move || Box::new(DiffPathMergerActor::<DB>::new().on_bc(&bc)))?;
        Ok(self)
    }

    /// Start all mergers
    pub fn with_mergers(&mut self) -> Result<&mut Self> {
        self.with_swap_path_merger()?.with_same_path_merger()?.with_diff_path_merger()
    }

    /// Start backrun on block
    pub fn with_backrun_block(&mut self, backrun_config: BackrunConfig) -> Result<&mut Self> {
        if !self.has_state_update {
            let bc = self.bc.clone();
            let strategy = self.strategy.clone();
            let backrun_config1 = backrun_config.clone();
            self.actor_manager.start(move || Box::new(StateChangeArbSearcherActor::new(backrun_config1).on_bc(&bc, &strategy)))?;
            self.has_state_update = true;
        }
        let bc = self.bc.clone();
        let state = self.state.clone();
        let strategy = self.strategy.clone();
        self.actor_manager.start(move || Box::new(BlockStateChangeProcessorActor::new().on_bc(&bc, &state, &strategy)))?;
        Ok(self)
    }

    /// Start backrun for pending txs
    pub fn with_backrun_mempool(&mut self, backrun_config: BackrunConfig) -> Result<&mut Self> {
        if !self.has_state_update {
            let bc = self.bc.clone();
            let strategy = self.strategy.clone();
            let backrun_config1 = backrun_config.clone();
            self.actor_manager.start(move || Box::new(StateChangeArbSearcherActor::new(backrun_config1).on_bc(&bc, &strategy)))?;
            self.has_state_update = true;
        }
        let provider = self.provider.clone();
        let bc = self.bc.clone();
        let state = self.state.clone();
        let strategy = self.strategy.clone();
        self.actor_manager.start(move || Box::new(PendingTxStateChangeProcessorActor::new(provider).on_bc(
            &bc,
            &state,
            &strategy,
        )))?;
        Ok(self)
    }

    /// Start backrun for blocks and pending txs
    pub async fn with_backrun(&mut self, backrun_config: BackrunConfig) -> Result<&mut Self> {
        self.with_backrun_block(backrun_config.clone())?.with_backrun_mempool(backrun_config)
    }

    /// Start influxdb writer
    pub fn with_influxdb_writer(&mut self, url: String, database: String, tags: HashMap<String, String>) -> Result<&mut Self> {
        let url = url.clone();
        let database = database.clone();
        let tags = tags.clone();
        let bc = self.bc.clone();
        self.actor_manager.start(move || Box::new(InfluxDbWriterActor::new(url, database, tags).on_bc(&bc)))?;
        Ok(self)
    }

    /// Start block latency recorder
    pub fn with_block_latency_recorder(&mut self) -> Result<&mut Self> {
        let bc = self.bc.clone();
        let state = self.state.clone();
        self.actor_manager.start(move || Box::new(MetricsRecorderActor::new().on_bc(&bc, &state)))?;
        Ok(self)
    }

    /// Start web server
    pub fn with_web_server<S>(&mut self, host: String, router: Router<S>, db_pool: DbPool) -> Result<&mut Self>
    where
        S: Clone + Send + Sync + 'static,
        Router: From<Router<S>>,
    {
        use std::sync::Arc;
        let host = Arc::new(host);
        let router = Arc::new(router);
        let db_pool = Arc::new(db_pool);
        let bc = Arc::new(self.bc.clone());
        let state = Arc::new(self.state.clone());
        self.actor_manager.start({
            let host = host.clone();
            let router = router.clone();
            let db_pool = db_pool.clone();
            let bc = bc.clone();
            let state = state.clone();
            move || Box::new(WebServerActor::new((*host).clone(), (*router).clone(), (*db_pool).clone(), CancellationToken::new()).on_bc(&bc, &state))
        })?;
        Ok(self)
    }
// <<<<<<< SEARCH
    pub fn with_influxdb_writer(&mut self, url: String, database: String, tags: HashMap<String, String>) -> Result<&mut Self> {
        let url = url.clone();
        let database = database.clone();
        let tags = tags.clone();
        let bc = self.bc.clone();
        self.actor_manager.start(move || Box::new(InfluxDbWriterActor::new(url, database, tags).on_bc(&bc)))?;
        Ok(self)
    }
// =======
    pub fn with_influxdb_writer(&mut self, url: String, database: String, tags: HashMap<String, String>) -> Result<&mut Self> {
        use std::sync::Arc;
        let url = Arc::new(url);
        let database = Arc::new(database);
        let tags = Arc::new(tags);
        let bc = Arc::new(self.bc.clone());
        self.actor_manager.start({
            let url = url.clone();
            let database = database.clone();
            let tags = tags.clone();
            let bc = bc.clone();
            move || Box::new(InfluxDbWriterActor::new((*url).clone(), (*database).clone(), (*tags).clone()).on_bc(&bc))
        })?;
        Ok(self)
    }
// <<<<<<< SEARCH
    pub fn with_backrun_mempool(&mut self, backrun_config: BackrunConfig) -> Result<&mut Self> {
        if !self.has_state_update {
            let bc = self.bc.clone();
            let strategy = self.strategy.clone();
            let backrun_config1 = backrun_config.clone();
            self.actor_manager.start(move || Box::new(StateChangeArbSearcherActor::new(backrun_config1).on_bc(&bc, &strategy)))?;
            self.has_state_update = true;
        }
        let provider = self.provider.clone();
        let bc = self.bc.clone();
        let state = self.state.clone();
        let strategy = self.strategy.clone();
        self.actor_manager.start(move || Box::new(PendingTxStateChangeProcessorActor::new(provider).on_bc(
            &bc,
            &state,
            &strategy,
        )))?;
        Ok(self)
    }
// =======
    pub fn with_backrun_mempool(&mut self, backrun_config: BackrunConfig) -> Result<&mut Self> {
        use std::sync::Arc;
        if !self.has_state_update {
            let bc = Arc::new(self.bc.clone());
            let strategy = Arc::new(self.strategy.clone());
            let backrun_config1 = Arc::new(backrun_config.clone());
            self.actor_manager.start({
                let bc = bc.clone();
                let strategy = strategy.clone();
                let backrun_config1 = backrun_config1.clone();
                move || Box::new(StateChangeArbSearcherActor::new((*backrun_config1).clone()).on_bc(&bc, &strategy))
            })?;
            self.has_state_update = true;
        }
        let provider = Arc::new(self.provider.clone());
        let bc = Arc::new(self.bc.clone());
        let state = Arc::new(self.state.clone());
        let strategy = Arc::new(self.strategy.clone());
        self.actor_manager.start({
            let provider = provider.clone();
            let bc = bc.clone();
            let state = state.clone();
            let strategy = strategy.clone();
            move || Box::new(PendingTxStateChangeProcessorActor::new((*provider).clone()).on_bc(&bc, &state, &strategy))
        })?;
        Ok(self)
    }
// <<<<<<< SEARCH
    pub fn with_backrun_block(&mut self, backrun_config: BackrunConfig) -> Result<&mut Self> {
        if !self.has_state_update {
            let bc = self.bc.clone();
            let strategy = self.strategy.clone();
            let backrun_config1 = backrun_config.clone();
            self.actor_manager.start(move || Box::new(StateChangeArbSearcherActor::new(backrun_config1).on_bc(&bc, &strategy)))?;
            self.has_state_update = true;
        }
        let bc = self.bc.clone();
        let state = self.state.clone();
        let strategy = self.strategy.clone();
        self.actor_manager.start(move || Box::new(BlockStateChangeProcessorActor::new().on_bc(&bc, &state, &strategy)))?;
        Ok(self)
    }
// =======
    pub fn with_backrun_block(&mut self, backrun_config: BackrunConfig) -> Result<&mut Self> {
        use std::sync::Arc;
        if !self.has_state_update {
            let bc = Arc::new(self.bc.clone());
            let strategy = Arc::new(self.strategy.clone());
            let backrun_config1 = Arc::new(backrun_config.clone());
            self.actor_manager.start({
                let bc = bc.clone();
                let strategy = strategy.clone();
                let backrun_config1 = backrun_config1.clone();
                move || Box::new(StateChangeArbSearcherActor::new((*backrun_config1).clone()).on_bc(&bc, &strategy))
            })?;
            self.has_state_update = true;
        }
        let bc = Arc::new(self.bc.clone());
        let state = Arc::new(self.state.clone());
        let strategy = Arc::new(self.strategy.clone());
        self.actor_manager.start({
            let bc = bc.clone();
            let state = state.clone();
            let strategy = strategy.clone();
            move || Box::new(BlockStateChangeProcessorActor::new().on_bc(&bc, &state, &strategy))
        })?;
        Ok(self)
    }
// <<<<<<< SEARCH
    pub fn with_same_path_merger(&mut self) -> Result<&mut Self> {
        let provider = self.provider.clone();
        let bc = self.bc.clone();
        let state = self.state.clone();
        let strategy = self.strategy.clone();
        let provider2 = provider.clone();
        let bc2 = bc.clone();
        let state2 = state.clone();
        let strategy2 = strategy.clone();
        self.actor_manager.start(move || Box::new(SamePathMergerActor::new(provider.clone()).on_bc(&bc, &state, &strategy)))?;
        self.actor_manager.start(move || Box::new(SamePathMergerActor::new(provider2.clone()).on_bc(&bc2, &state2, &strategy2)))?;
        Ok(self)
    }
// =======
    pub fn with_same_path_merger(&mut self) -> Result<&mut Self> {
        use std::sync::Arc;
        let provider = Arc::new(self.provider.clone());
        let bc = Arc::new(self.bc.clone());
        let state = Arc::new(self.state.clone());
        let strategy = Arc::new(self.strategy.clone());
        let provider2 = provider.clone();
        let bc2 = bc.clone();
        let state2 = state.clone();
        let strategy2 = strategy.clone();
        self.actor_manager.start({
            let provider = provider.clone();
            let bc = bc.clone();
            let state = state.clone();
            let strategy = strategy.clone();
            move || Box::new(SamePathMergerActor::new((*provider).clone()).on_bc(&bc, &state, &strategy))
        })?;
        self.actor_manager.start({
            let provider2 = provider2.clone();
            let bc2 = bc2.clone();
            let state2 = state2.clone();
            let strategy2 = strategy2.clone();
            move || Box::new(SamePathMergerActor::new((*provider2).clone()).on_bc(&bc2, &state2, &strategy2))
        })?;
        Ok(self)
    }
// <<<<<<< SEARCH
    pub fn with_pool_loader(&mut self, pools_config: PoolsLoadingConfig) -> Result<&mut Self> {
        let pool_loaders = Arc::new(PoolLoadersBuilder::default_pool_loaders(self.provider.clone(), pools_config.clone()));
        let provider = self.provider.clone();
        let pool_loaders_clone = pool_loaders.clone();
        let pools_config_clone = pools_config.clone();
        let bc = self.bc.clone();
        let state = self.state.clone();
        self.actor_manager.start(move || Box::new(PoolLoaderActor::new(provider.clone(), pool_loaders_clone.clone(), pools_config_clone.clone()).on_bc(&bc, &state)))?;
        Ok(self)
    }
// =======
    pub fn with_pool_loader(&mut self, pools_config: PoolsLoadingConfig) -> Result<&mut Self> {
        use std::sync::Arc;
        let pool_loaders = Arc::new(PoolLoadersBuilder::default_pool_loaders(self.provider.clone(), pools_config.clone()));
        let provider = Arc::new(self.provider.clone());
        let pool_loaders_clone = pool_loaders.clone();
        let pools_config_clone = Arc::new(pools_config.clone());
        let bc = Arc::new(self.bc.clone());
        let state = Arc::new(self.state.clone());
        self.actor_manager.start({
            let provider = provider.clone();
            let pool_loaders_clone = pool_loaders_clone.clone();
            let pools_config_clone = pools_config_clone.clone();
            let bc = bc.clone();
            let state = state.clone();
            move || Box::new(PoolLoaderActor::new((*provider).clone(), pool_loaders_clone.clone(), (*pools_config_clone).clone()).on_bc(&bc, &state))
        })?;
        Ok(self)
    }
// <<<<<<< SEARCH
    pub fn with_new_pool_loader(&mut self, pools_config: PoolsLoadingConfig) -> Result<&mut Self> {
        let pool_loader = Arc::new(PoolLoadersBuilder::default_pool_loaders(self.provider.clone(), pools_config));
        let pool_loader_clone = pool_loader.clone();
        let bc = self.bc.clone();
        self.actor_manager.start(move || Box::new(NewPoolLoaderActor::new(pool_loader_clone).on_bc(&bc)))?;
        Ok(self)
    }
// =======
    pub fn with_new_pool_loader(&mut self, pools_config: PoolsLoadingConfig) -> Result<&mut Self> {
        use std::sync::Arc;
        let pool_loader = Arc::new(PoolLoadersBuilder::default_pool_loaders(self.provider.clone(), pools_config));
        let pool_loader_clone = pool_loader.clone();
        let bc = Arc::new(self.bc.clone());
        self.actor_manager.start({
            let pool_loader_clone = pool_loader_clone.clone();
            let bc = bc.clone();
            move || Box::new(NewPoolLoaderActor::new(pool_loader_clone).on_bc(&bc))
        })?;
        Ok(self)
    }
// <<<<<<< SEARCH
    pub fn with_pool_history_loader(&mut self, pools_config: PoolsLoadingConfig) -> Result<&mut Self> {
        let pool_loaders = Arc::new(PoolLoadersBuilder::default_pool_loaders(self.provider.clone(), pools_config));
        let provider = self.provider.clone();
        let pool_loaders_clone = pool_loaders.clone();
        let bc = self.bc.clone();
        self.actor_manager.start(move || Box::new(HistoryPoolLoaderOneShotActor::new(provider.clone(), pool_loaders_clone.clone()).on_bc(&bc)))?;
        Ok(self)
    }
// =======
    pub fn with_pool_history_loader(&mut self, pools_config: PoolsLoadingConfig) -> Result<&mut Self> {
        use std::sync::Arc;
        let pool_loaders = Arc::new(PoolLoadersBuilder::default_pool_loaders(self.provider.clone(), pools_config));
        let provider = Arc::new(self.provider.clone());
        let pool_loaders_clone = pool_loaders.clone();
        let bc = Arc::new(self.bc.clone());
        self.actor_manager.start({
            let provider = provider.clone();
            let pool_loaders_clone = pool_loaders_clone.clone();
            let bc = bc.clone();
            move || Box::new(HistoryPoolLoaderOneShotActor::new((*provider).clone(), pool_loaders_clone.clone()).on_bc(&bc))
        })?;
        Ok(self)
    }
// <<<<<<< SEARCH
    pub fn with_curve_pool_protocol_loader(&mut self, pools_config: PoolsLoadingConfig) -> Result<&mut Self> {
        let pool_loaders = Arc::new(PoolLoadersBuilder::default_pool_loaders(self.provider.clone(), pools_config));
        let pool_loaders_clone = pool_loaders.clone();
        let provider = self.provider.clone();
        let bc = self.bc.clone();
        self.actor_manager.start(move || {
            let pool_loaders = pool_loaders_clone.clone();
            Box::new(ProtocolPoolLoaderOneShotActor::new(provider, pool_loaders).on_bc(&bc))
        })?;
        Ok(self)
    }
// =======
    pub fn with_curve_pool_protocol_loader(&mut self, pools_config: PoolsLoadingConfig) -> Result<&mut Self> {
        use std::sync::Arc;
        let pool_loaders = Arc::new(PoolLoadersBuilder::default_pool_loaders(self.provider.clone(), pools_config));
        let pool_loaders_clone = pool_loaders.clone();
        let provider = Arc::new(self.provider.clone());
        let bc = Arc::new(self.bc.clone());
        self.actor_manager.start({
            let pool_loaders = pool_loaders_clone.clone();
            let provider = provider.clone();
            let bc = bc.clone();
            move || Box::new(ProtocolPoolLoaderOneShotActor::new((*provider).clone(), pool_loaders).on_bc(&bc))
        })?;
        Ok(self)
    }
// <<<<<<< SEARCH
    pub fn with_nonce_and_balance_monitor(&mut self) -> Result<&mut Self> {
        let provider = self.provider.clone();
        self.actor_manager.start(move || Box::new(NonceAndBalanceMonitorActor::new(provider)))?;
        Ok(self)
    }
// =======
    pub fn with_nonce_and_balance_monitor(&mut self) -> Result<&mut Self> {
        use std::sync::Arc;
        let provider = Arc::new(self.provider.clone());
        self.actor_manager.start({
            let provider = provider.clone();
            move || Box::new(NonceAndBalanceMonitorActor::new((*provider).clone()))
        })?;
        Ok(self)
    }
// <<<<<<< SEARCH
    pub fn with_nonce_and_balance_monitor_only_events(&mut self) -> Result<&mut Self> {
        let provider = self.provider.clone();
        self.actor_manager.start(move || Box::new(NonceAndBalanceMonitorActor::new(provider).only_once()))?;
        Ok(self)
    }
// =======
    pub fn with_nonce_and_balance_monitor_only_events(&mut self) -> Result<&mut Self> {
        use std::sync::Arc;
        let provider = Arc::new(self.provider.clone());
        self.actor_manager.start({
            let provider = provider.clone();
            move || Box::new(NonceAndBalanceMonitorActor::new((*provider).clone()).only_once())
        })?;
        Ok(self)
    }
// <<<<<<< SEARCH
    pub fn with_block_history(&mut self) -> Result<&mut Self> {
        let provider = self.provider.clone();
        let bc = self.bc.clone();
        let state = self.state.clone();
        self.actor_manager.start(move || Box::new(BlockHistoryActor::new(provider).on_bc(&bc, &state)))?;
        Ok(self)
    }
// =======
    pub fn with_block_history(&mut self) -> Result<&mut Self> {
        use std::sync::Arc;
        let provider = Arc::new(self.provider.clone());
        let bc = Arc::new(self.bc.clone());
        let state = Arc::new(self.state.clone());
        self.actor_manager.start({
            let provider = provider.clone();
            let bc = bc.clone();
            let state = state.clone();
            move || Box::new(BlockHistoryActor::new((*provider).clone()).on_bc(&bc, &state))
        })?;
        Ok(self)
    }
// <<<<<<< SEARCH
    pub fn with_price_station(&mut self) -> Result<&mut Self> {
        let provider = self.provider.clone();
        let bc = self.bc.clone();
        self.actor_manager.start(move || Box::new(PriceActor::new(provider).on_bc(&bc)))?;
        Ok(self)
    }
// =======
    pub fn with_price_station(&mut self) -> Result<&mut Self> {
        use std::sync::Arc;
        let provider = Arc::new(self.provider.clone());
        let bc = Arc::new(self.bc.clone());
        self.actor_manager.start({
            let provider = provider.clone();
            let bc = bc.clone();
            move || Box::new(PriceActor::new((*provider).clone()).on_bc(&bc))
        })?;
        Ok(self)
    }
// <<<<<<< SEARCH
    pub fn with_block_events(&mut self, config: NodeBlockActorConfig) -> Result<&mut Self> {
        let provider = self.provider.clone();
        let bc = self.bc.clone();
        self.actor_manager.start(move || Box::new(NodeBlockActor::new(provider, config).on_bc(&bc)))?;
        Ok(self)
    }
// =======
    pub fn with_block_events(&mut self, config: NodeBlockActorConfig) -> Result<&mut Self> {
        use std::sync::Arc;
        let provider = Arc::new(self.provider.clone());
        let bc = Arc::new(self.bc.clone());
        self.actor_manager.start({
            let provider = provider.clone();
            let bc = bc.clone();
            let config = config.clone();
            move || Box::new(NodeBlockActor::new((*provider).clone(), config).on_bc(&bc))
        })?;
        Ok(self)
    }
// <<<<<<< SEARCH
    pub fn with_local_mempool_events(&mut self) -> Result<&mut Self> {
        self.mempool()?;
        let provider = self.provider.clone();
        let bc = self.bc.clone();
        self.actor_manager.start(move || Box::new(NodeMempoolActor::new(provider).on_bc(&bc)))?;
        Ok(self)
    }
// =======
    pub fn with_local_mempool_events(&mut self) -> Result<&mut Self> {
        self.mempool()?;
        use std::sync::Arc;
        let provider = Arc::new(self.provider.clone());
        let bc = Arc::new(self.bc.clone());
        self.actor_manager.start({
            let provider = provider.clone();
            let bc = bc.clone();
            move || Box::new(NodeMempoolActor::new((*provider).clone()).on_bc(&bc))
        })?;
        Ok(self)
    }
// <<<<<<< SEARCH
    pub fn with_remote_mempool<PM>(&mut self, provider: PM) -> Result<&mut Self>
    where
        PM: Provider<Ethereum> + Send + Sync + Clone + 'static,
    {
        self.mempool()?;
        let bc = self.bc.clone();
        self.actor_manager.start(move || Box::new(NodeMempoolActor::new(provider).on_bc(&bc)))?;
        Ok(self)
    }
// =======
    pub fn with_remote_mempool<PM>(&mut self, provider: PM) -> Result<&mut Self>
    where
        PM: Provider<Ethereum> + Send + Sync + Clone + 'static,
    {
        self.mempool()?;
        use std::sync::Arc;
        let bc = Arc::new(self.bc.clone());
        self.actor_manager.start({
            let bc = bc.clone();
            let provider = Arc::new(provider);
            let provider = provider.clone();
            move || Box::new(NodeMempoolActor::new((*provider).clone()).on_bc(&bc))
        })?;
        Ok(self)
    }
// <<<<<<< SEARCH
    pub fn with_flashbots_broadcaster(&mut self, allow_broadcast: bool) -> Result<&mut Self> {
        let provider = self.provider.clone();
        let relays = self.relays.clone();
        let flashbots = match relays.is_empty() {
            true => Flashbots::new(provider.clone(), "https://relay.flashbots.net", None).with_default_relays(),
            false => Flashbots::new(provider.clone(), "https://relay.flashbots.net", None).with_relays(relays),
        };

        self.actor_manager.start(move || Box::new(FlashbotsBroadcastActor::new(flashbots, allow_broadcast)))?;
        Ok(self)
    }
// =======
    pub fn with_flashbots_broadcaster(&mut self, allow_broadcast: bool) -> Result<&mut Self> {
        use std::sync::Arc;
        let provider = Arc::new(self.provider.clone());
        let relays = Arc::new(self.relays.clone());
        let flashbots = match (*relays).is_empty() {
            true => Flashbots::new((*provider).clone(), "https://relay.flashbots.net", None).with_default_relays(),
            false => Flashbots::new((*provider).clone(), "https://relay.flashbots.net", None).with_relays((*relays).clone()),
        };

        self.actor_manager.start({
            let flashbots = Arc::new(flashbots);
            let flashbots = flashbots.clone();
            move || Box::new(FlashbotsBroadcastActor::new((*flashbots).clone(), allow_broadcast))
        })?;
        Ok(self)
    }
// <<<<<<< SEARCH
    pub fn with_swap_encoder(&mut self, swap_encoder: E) -> Result<&mut Self> {
        self.mutlicaller_address = Some(swap_encoder.address());
        self.encoder = Some(swap_encoder);
        let signers = self.signers.clone();
        let bc = self.bc.clone();
        let strategy = self.strategy.clone();
        self.actor_manager.start(move || Box::new(SwapRouterActor::<DB>::new().with_signers(signers).on_bc(&bc, &strategy)))?;
        Ok(self)
    }
// =======
    pub fn with_swap_encoder(&mut self, swap_encoder: E) -> Result<&mut Self> {
        use std::sync::Arc;
        self.mutlicaller_address = Some(swap_encoder.address());
        self.encoder = Some(swap_encoder);
        let signers = Arc::new(self.signers.clone());
        let bc = Arc::new(self.bc.clone());
        let strategy = Arc::new(self.strategy.clone());
        self.actor_manager.start({
            let signers = signers.clone();
            let bc = bc.clone();
            let strategy = strategy.clone();
            move || Box::new(SwapRouterActor::<DB>::new().with_signers((*signers).clone()).on_bc(&bc, &strategy))
        })?;
        Ok(self)

    /// Wait for node sync
    pub fn with_wait_for_node_sync(&mut self) -> Result<&mut Self> {
        let provider = self.provider.clone();
        self.actor_manager.start_and_wait(move || Box::new(WaitForNodeSyncOneShotBlockingActor::new(provider)))?;
        Ok(self)
    }
}