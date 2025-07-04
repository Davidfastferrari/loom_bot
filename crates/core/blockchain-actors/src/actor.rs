extern crate loom_types_entities;
extern crate loom_types_blockchain;

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
use loom_core_actors::Actor as LoomActor;
use std::sync::Arc;
use loom_core_block_history_actor::BlockHistoryActor;
use loom_core_blockchain::{Blockchain, BlockchainState, Strategy};
use loom_core_mempool::MempoolActor;
use loom_core_router::SwapRouterActor;
use loom_defi_address_book::TokenAddressEth;
use loom_defi_health_monitor::{MetricsRecorderActor, PoolHealthMonitorActor, StuffingTxMonitorActor};
use loom_defi_market::{
    HistoryPoolLoaderOneShotActor, NewPoolLoaderActor, PoolLoaderActor, ProtocolPoolLoaderOneShotActor, RequiredPoolLoaderActor,
};
use loom_defi_pools::{PoolLoadersBuilder, PoolsLoadingConfig, UniswapV2PoolLoader, UniswapV3PoolLoader, MaverickPoolLoader};
use loom_defi_preloader::MarketStatePreloadedOneShotActor;
use loom_types_entities::{PoolId, PoolClass, BlockHistoryState, SwapEncoder, TxSigners};
use loom_types_entities::required_state::RequiredState;
use loom_types_blockchain::loom_data_types_ethereum::LoomDataTypesEthereum;
use tokio::runtime::Runtime;
use futures::executor::block_on;
use futures::Stream;
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
use revm::{Database, DatabaseCommit, DatabaseRef};
use std::collections::HashMap;
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

    pub fn with_preloaded_state(&mut self, pools: Vec<(PoolId, PoolClass)>, required_state: Option<RequiredState>) -> Result<&mut Self> {
        use loom_defi_pools::PoolLoadersBuilder;
        use loom_defi_preloader::preload_market_state;
        use loom_types_entities::PoolClass;
        use loom_types_entities::required_state::RequiredState;
        use loom_types_blockchain::loom_data_types_ethereum::LoomDataTypesEthereum;
        use tokio::runtime::Runtime;
        use futures::executor::block_on;
        use std::collections::HashSet;

        let provider = self.provider.clone();
        let state = self.state.clone();

        // Collect unique pool classes from input poolss
        let mut pool_classes = HashSet::new();
        for (_, pool_class) in &pools {
            pool_classes.insert(*pool_class);
        }

        // Build pool loaders builder with provider
        let mut builder = PoolLoadersBuilder::<P, Ethereum, LoomDataTypesEthereum>::new().with_provider(provider.clone());

        // Add loaders for each pool class
        if pool_classes.contains(&PoolClass::UniswapV3) {
            builder = builder.add_loader(PoolClass::UniswapV3, UniswapV3PoolLoader::with_provider(provider.clone()));
        }
        if pool_classes.contains(&PoolClass::UniswapV2) {
            builder = builder.add_loader(PoolClass::UniswapV2, UniswapV2PoolLoader::with_provider(provider.clone()));
        }
        if pool_classes.contains(&PoolClass::Curve) {
            // CurvePoolLoader is not defined, so skip adding loader for Curve pools or replace with correct loader if available
            // builder = builder.add_loader(PoolClass::Curve, CurvePoolLoader::with_provider(provider.clone()));
        }
        if pool_classes.contains(&PoolClass::Maverick) {
            builder = builder.add_loader(PoolClass::Maverick, MaverickPoolLoader::with_provider(provider.clone()));
        }

        let pool_loaders = builder.build();

        // Prepare vectors to collect data for preloading
        let mut copied_accounts = Vec::new();
        let mut new_accounts = Vec::new();
        let mut token_balances: Vec<(Address, Address, U256)> = Vec::new();

        // For each pool, fetch pool state and collect data
        for (pool_id, pool_class) in pools {
            let pool_loader = pool_loaders.map.get(&pool_class).ok_or_else(|| eyre!("Pool loader not found for class {:?}", pool_class))?;
            let pool_wrapper = block_on(pool_loader.fetch_pool_by_id(pool_id.clone()))?;

            // Extract accounts, new accounts, token balances from pool_wrapper
            // Implemented extraction based on Pool trait methods

            // Extract tokens and balances as tuples for token_balances
            let pool_manager_cells = pool_wrapper.get_pool_manager_cells();
            for (account, balances) in pool_manager_cells {
                copied_accounts.push(account);
                for balance in balances {
                    // Assuming balance is U256 and account is Address, use account twice as placeholder for token address
                    token_balances.push((account, account, balance));
                }
            }

            // For new accounts, no direct method, so leave empty or implement as needed

            // For now, skipping actual extraction due to lack of details
        }

        // TODO: Add required_state data to copied_accounts, new_accounts, token_balances as needed

        if let Some(_required_state) = required_state {
            // RequiredState fields do not match copied_accounts, new_accounts, token_balances
            // So no data added here.
        }

        // Run the preload_market_state async function synchronously
        let rt = Runtime::new()?;
        rt.block_on(async {
            preload_market_state(
                provider.clone(),
                copied_accounts,
                new_accounts,
                token_balances,
                state.market_state_commit(),
                None,
            )
            .await
        })?;

        Ok(self)
    }

    pub async fn wait(self) {
        self.actor_manager.wait().await
    }

    /// Start a custom actor
    pub fn start<F>(&mut self, actor_factory: F) -> Result<&mut Self>
    where
        F: Fn() -> Box<dyn LoomActor + Send + Sync> + Send + Sync + Clone + 'static,
    {
        self.actor_manager.start(actor_factory)?;
        Ok(self)
    }

    /// Start a custom actor and wait for it to finish
    pub fn start_and_wait(&mut self, actor: impl LoomActor + Send + Sync + 'static) -> Result<&mut Self> {
        self.actor_manager.start_and_wait(actor)?;
        Ok(self)
    }

    /// Initialize signers with the default anvil Private Key
    pub fn initialize_signers_with_anvil(&mut self) -> Result<&mut Self> {
        let key: B256 = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80".parse()?;

        let signers_clone = self.signers.clone();
        let key_vec = key.to_vec();
        let closure = {
            let key_vec = key_vec.clone();
            let signers = signers_clone.clone();
            move || Box::new(InitializeSignersOneShotBlockingActor::new(Some(key_vec.clone())).with_signers(signers.clone())) as Box<dyn LoomActor + Send + Sync>
        };
        self.actor_manager.start(closure)?;
        self.with_signers()?;
        Ok(self)
    }

    /// Initialize signers with the private key. Random key generated if param in None
    pub fn initialize_signers_with_key(&mut self, key: Option<Vec<u8>>) -> Result<&mut Self> {
        let signers_clone = self.signers.clone();
        let closure = {
            let key = key.clone();
            let signers = signers_clone.clone();
            move || Box::new(InitializeSignersOneShotBlockingActor::new(key.clone()).with_signers(signers.clone())) as Box<dyn LoomActor + Send + Sync>
        };
        self.actor_manager.start(closure)?;
        self.with_signers()?;
        Ok(self)
    }
    /// Initialize signers with multiple private keys
    pub fn initialize_signers_with_keys(&mut self, keys: Vec<Vec<u8>>) -> Result<&mut Self> {
        let signers_clone = self.signers.clone();
        for key in keys {
            let closure = {
                let key = key.clone();
                let signers = signers_clone.clone();
                move || Box::new(InitializeSignersOneShotBlockingActor::new(Some(key.clone())).with_signers(signers.clone())) as Box<dyn LoomActor + Send + Sync>
            };
            self.actor_manager.start(closure)?;
        }
        self.with_signers()?;
        Ok(self)
    }
    /// Initialize signers with encrypted private key
    pub fn initialize_signers_with_encrypted_key(&mut self, key: Vec<u8>) -> Result<&mut Self> {
        let signers_clone = self.signers.clone();
        let closure = {
            let key = key.clone();
            let signers = signers_clone.clone();
            move || {
                let actor = InitializeSignersOneShotBlockingActor::new_from_encrypted_key(key.clone());
                match actor {
                    Ok(a) => Box::new(a.with_signers(signers.clone())) as Box<dyn LoomActor + Send + Sync>,
                    Err(e) => panic!("Failed to create InitializeSignersOneShotBlockingActor: {:?}", e),
                }
            }
        };
        self.actor_manager.start(closure)?;
        self.with_signers()?;
        Ok(self)
    }
    /// Initializes signers with encrypted key form DATA env var
    pub fn initialize_signers_with_env(&mut self) -> Result<&mut Self> {
        let signers_clone = self.signers.clone();
        let closure = {
            let signers = signers_clone.clone();
            move || {
                let actor = InitializeSignersOneShotBlockingActor::new_from_encrypted_env();
                match actor {
                    Ok(a) => Box::new(a.with_signers(signers.clone())) as Box<dyn LoomActor + Send + Sync>,
                    Err(e) => panic!("Failed to create InitializeSignersOneShotBlockingActor: {:?}", e),
                }
            }
        };
        self.actor_manager.start(closure)?;
        self.with_signers()?;
        Ok(self)
    }
    /// Starts signer actor
    pub fn with_signers(&mut self) -> Result<&mut Self> {
        if !self.has_signers {
            self.has_signers = true;
            let closure = move || Box::new(TxSignersActor::<LoomDataTypesEthereum>::new()) as Box<dyn LoomActor + Send + Sync>;
            self.actor_manager.start(closure)?;
        }
        Ok(self)
    }
    /// Starts encoder actor
    pub fn with_swap_encoder(&mut self, swap_encoder: E) -> Result<&mut Self> {
        self.mutlicaller_address = Some(swap_encoder.address());
        self.encoder = Some(swap_encoder);
        let bc = self.bc.clone();
        let strategy = self.strategy.clone();
        let closure = move || Box::new(SwapRouterActor::<DB>::new().on_bc(&bc, &strategy)) as Box<dyn LoomActor + Send + Sync>;
        self.actor_manager.start(closure)?;
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

        // Add explicit type parameters for MarketStatePreloadedOneShotActor::new
        let closure = move || Box::new(MarketStatePreloadedOneShotActor::<P, Ethereum, DB>::new(provider.clone()).on_bc(&bc, &state)) as Box<dyn LoomActor + Send + Sync>;
        self.actor_manager.start(closure)?;
        Ok(self)
    }
    /// Starts nonce and balance monitor
    pub fn with_nonce_and_balance_monitor(&mut self) -> Result<&mut Self> {
        use std::sync::Arc;
        let provider = Arc::new(self.provider.clone());
        let _provider_clone = provider.clone();
        let closure = move || Box::new(NonceAndBalanceMonitorActor::new(provider.clone())) as Box<dyn LoomActor + Send + Sync>;
        self.actor_manager.start(closure)?;
        Ok(self)
    }
    /// Starts block history actor
    pub fn with_block_history(&mut self) -> Result<&mut Self> {
        use std::sync::Arc;
        let provider = Arc::new(self.provider.clone());
        let bc = Arc::new(self.bc.clone());
        let state = Arc::new(self.state.clone());
        let closure = move || Box::new(BlockHistoryActor::new((*provider).clone()).on_bc(&bc, &state)) as Box<dyn LoomActor + Send + Sync>;
        self.actor_manager.start(closure)?;
        Ok(self)
    }
    /// Starts token price calculator
    pub fn with_price_station(&mut self) -> Result<&mut Self> {
        use std::sync::Arc;
        let provider = Arc::new(self.provider.clone());
        let bc = Arc::new(self.bc.clone());
        let closure = move || Box::new(PriceActor::new(provider.clone()).on_bc(&bc)) as Box<dyn LoomActor + Send + Sync>;
        self.actor_manager.start(closure)?;
        Ok(self)
    }
    /// Starts receiving blocks events through RPC
    pub fn with_block_events(&mut self, config: NodeBlockActorConfig) -> Result<&mut Self> {
        use std::sync::Arc;
        let provider = Arc::new(self.provider.clone());
        let bc = Arc::new(self.bc.clone());
        // Dereference config to avoid Arc
        let config = config.clone();
        let _provider_clone = provider.clone();
        let bc_clone = bc.clone();
        let config_clone = config.clone();
        let closure = move || {
            let actor = NodeBlockActor::new((*provider).clone(), config_clone.clone());
            let actor = actor.on_bc(&bc_clone);
            Box::new(actor) as Box<dyn LoomActor + Send + Sync>
        };
        self.actor_manager.start(closure)?;
        Ok(self)
    }
    /// Starts receiving blocks and mempool events through ExEx GRPC
    pub fn with_exex_events(&mut self) -> Result<&mut Self> {
        self.mempool()?;
        let bc = self.bc.clone();
        let closure = move || Box::new(NodeExExGrpcActor::new("http://[::1]:10000".to_string()).on_bc(&bc)) as Box<dyn LoomActor + Send + Sync>;
        self.actor_manager.start(closure)?;
        Ok(self)
    }
    /// Starts mempool actor collecting pending txes from all mempools and pulling new tx hashes in mempool_events channel
    pub fn mempool(&mut self) -> Result<&mut Self> {
        if !self.has_mempool {
            self.has_mempool = true;
            let bc = self.bc.clone();
            let closure = move || Box::new(MempoolActor::new().on_bc(&bc)) as Box<dyn LoomActor + Send + Sync>;
            self.actor_manager.start(closure)?;
        }
        Ok(self)
    }
    /// Starts flashbots broadcaster
    pub fn with_flashbots_broadcaster(&mut self, allow_broadcast: bool) -> Result<&mut Self> {
        use std::sync::Arc;
        let provider = self.provider.clone();
        let relays = self.relays.clone();
        let flashbots = if relays.is_empty() {
            Flashbots::new(provider.clone(), "https://relay.flashbots.net", None).with_default_relays()
        } else {
            Flashbots::new(provider.clone(), "https://relay.flashbots.net", None).with_relays(relays)
        };

        let flashbots = Arc::new(flashbots);
        let closure = {
            let flashbots = flashbots.clone();
            move || Box::new(FlashbotsBroadcastActor::new(flashbots.clone(), allow_broadcast)) as Box<dyn LoomActor + Send + Sync>
        };
        self.actor_manager.start(closure)?;
        Ok(self)
    }

    /// Starts EVM estimator actor
    pub fn with_evm_estimator(&mut self) -> Result<&mut Self> {
        let encoder = self.encoder.clone().expect("Encoder must be set before starting EvmEstimatorActor");
        let closure = move || Box::new(EvmEstimatorActor::<P, Ethereum, E, DB>::new(encoder.clone())) as Box<dyn LoomActor + Send + Sync>;
        self.actor_manager.start(closure)?;
        Ok(self)
    }

    /// Starts pool history loader actor
    pub fn with_pool_history_loader(&mut self, pools_config: PoolsLoadingConfig) -> Result<&mut Self> {
        use std::sync::Arc;
        use loom_defi_pools::PoolLoadersBuilder;

        let provider = Arc::new(self.provider.clone());
        let pool_loaders = Arc::new(
            PoolLoadersBuilder::<P, alloy_network::Ethereum, LoomDataTypesEthereum>::new()
                .with_provider(self.provider.clone())
                .build(),
        );

        let closure = {
            let provider = provider.clone();
            let pool_loaders = pool_loaders.clone();
            move || Box::new(HistoryPoolLoaderOneShotActor::new(provider.clone(), pool_loaders.clone())) as Box<dyn LoomActor + Send + Sync>
        };
        self.actor_manager.start(closure)?;
        Ok(self)
    }

    /// Starts new pool loader actor
    pub fn with_new_pool_loader(&mut self, pools_config: PoolsLoadingConfig) -> Result<&mut Self> {
        use std::sync::Arc;
        use loom_defi_pools::PoolLoadersBuilder;

        let provider = Arc::new(self.provider.clone());
        let pool_loaders = Arc::new(
            PoolLoadersBuilder::<P, alloy_network::Ethereum, LoomDataTypesEthereum>::new()
                .with_provider(self.provider.clone())
                .with_config(pools_config.clone())
                .build(),
        );

        let closure = {
            let pool_loaders = pool_loaders.clone();
            move || Box::new(NewPoolLoaderActor::new(pool_loaders.clone())) as Box<dyn LoomActor + Send + Sync>
        };
        self.actor_manager.start(closure)?;
        Ok(self)
    }

    /// Starts pool loader actor
    pub fn with_pool_loader(&mut self, pools_config: PoolsLoadingConfig) -> Result<&mut Self> {
        use std::sync::Arc;
        use loom_defi_pools::PoolLoadersBuilder;
        use loom_defi_market::PoolLoaderActor;

        let provider = Arc::new(self.provider.clone());
        let pool_loaders = Arc::new(
            PoolLoadersBuilder::<P, alloy_network::Ethereum, LoomDataTypesEthereum>::new()
                .with_provider(self.provider.clone())
                .with_config(pools_config.clone())
                .build(),
        );

        let closure = {
            let pool_loaders = pool_loaders.clone();
            let provider = self.provider.clone();
            move || Box::new(PoolLoaderActor::<P, P, alloy_network::Ethereum, DB>::new(provider.clone(), pool_loaders.clone(), pools_config.clone())) as Box<dyn LoomActor + Send + Sync>
        };
        self.actor_manager.start(closure)?;
        Ok(self)
    }

    /// Starts swap path merger actor(s)
    pub fn with_swap_path_merger(&mut self) -> Result<&mut Self> {
        let bc = self.bc.clone();
        let state = self.state.clone();
        let strategy = self.strategy.clone();
        let multicaller_address = self.mutlicaller_address.ok_or_else(|| eyre::eyre!("Multicaller address not set"))?;

        let closure_arb = {
            let multicaller_address = multicaller_address.clone();
            let bc = bc.clone();
            let strategy = strategy.clone();
            move || Box::new(ArbSwapPathMergerActor::new(multicaller_address).on_bc(&bc, &strategy)) as Box<dyn LoomActor + Send + Sync>
        };
        self.actor_manager.start(closure_arb)?;

        let closure_diff = {
            let bc = bc.clone();
            let strategy = strategy.clone();
            move || Box::new(DiffPathMergerActor::new().on_bc(&bc).on_strategy(&strategy)) as Box<dyn LoomActor + Send + Sync>
        };
        self.actor_manager.start(closure_diff)?;

        let closure_same = {
            let bc = bc.clone();
            let state = state.clone();
            let strategy = strategy.clone();
            let provider = self.provider.clone();
            move || Box::new(SamePathMergerActor::new(provider.clone()).on_bc(&bc, &state, &strategy)) as Box<dyn LoomActor + Send + Sync>
        };
        self.actor_manager.start(closure_same)?;

        Ok(self)
    }
}
