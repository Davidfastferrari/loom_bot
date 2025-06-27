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
use loom_defi_pools::{PoolLoadersBuilder, PoolsLoadingConfig};
use loom_defi_preloader::MarketStatePreloadedOneShotActor;
use loom_defi_price::PriceActor;
use loom_evm_db::DatabaseLoomExt;
use loom_evm_utils::NWETH;
use loom_execution_estimator::{EvmEstimatorActor, GethEstimatorActor};
use loom_execution_multicaller::MulticallerSwapEncoder;
use std::sync::Arc;
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
    pub fn start<F>(&mut self, actor_factory: F) -> Result<&mut Self>
    where
        F: Fn() -> Box<dyn Actor + Send + Sync> + Send + Sync + 'static,
    {
        let closure: Arc<dyn Fn() -> Box<dyn Actor + Send + Sync> + Send + Sync> = Arc::new(actor_factory);
        self.actor_manager.start(closure)?;
        Ok(self)
    }

    /// Start a custom actor and wait for it to finish
    pub fn start_and_wait<F>(&mut self, actor_factory: F) -> Result<&mut Self>
    where
        F: Fn() -> Box<dyn Actor + Send + Sync> + Send + Sync + 'static,
    {
        let closure: Arc<dyn Fn() -> Box<dyn Actor + Send + Sync> + Send + Sync> = Arc::new(actor_factory);
        self.actor_manager.start_and_wait(closure)?;
        Ok(self)
    }

    /// Initialize signers with the default anvil Private Key
    pub fn initialize_signers_with_anvil(&mut self) -> Result<&mut Self> {
        use std::sync::Arc;
        let key: B256 = "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80".parse()?;

        let signers_clone = self.signers.clone();
        let key_vec = key.to_vec();
        let key_vec_clone = key_vec.clone();
        let signers_clone2 = signers_clone.clone();
        self.actor_manager.start(move || Box::new(InitializeSignersOneShotBlockingActor::new(Some(key_vec_clone)).with_signers(signers_clone2)))?;
        self.with_signers()?;
        Ok(self)
    }

    /// Initialize signers with the private key. Random key generated if param in None
    pub fn initialize_signers_with_key(&mut self, key: Option<Vec<u8>>) -> Result<&mut Self> {
        use std::sync::Arc;
        let signers_clone = self.signers.clone();
        let key_clone = key.clone();
        let key_clone2 = key_clone.clone();
        let signers_clone2 = signers_clone.clone();
        self.actor_manager.start(move || Box::new(InitializeSignersOneShotBlockingActor::new(key_clone2).with_signers(signers_clone2)))?;
        self.with_signers()?;
        Ok(self)
    }

    /// Initialize signers with multiple private keys
    pub fn initialize_signers_with_keys(&mut self, keys: Vec<Vec<u8>>) -> Result<&mut Self> {
        use std::sync::Arc;
        let signers_clone = self.signers.clone();
        for key in keys {
            let signers_clone2 = signers_clone.clone();
            let key_clone = key.clone();
            let key_clone2 = key_clone.clone();
            self.actor_manager.start(move || Box::new(InitializeSignersOneShotBlockingActor::new(Some(key_clone2)).with_signers(signers_clone2)))?;
        }
        self.with_signers()?;
        Ok(self)
    }

    /// Initialize signers with encrypted private key
    pub fn initialize_signers_with_encrypted_key(&mut self, key: Vec<u8>) -> Result<&mut Self> {
        use std::sync::Arc;
        let signers_clone = self.signers.clone();
        let key_clone = key.clone();
        self.actor_manager.start(move || {
            let actor = InitializeSignersOneShotBlockingActor::new_from_encrypted_key(key_clone);
            match actor {
                Ok(a) => Box::new(a.with_signers(signers_clone.clone())),
                Err(e) => panic!("Failed to create InitializeSignersOneShotBlockingActor: {:?}", e),
            }
        })?;
        self.with_signers()?;
        Ok(self)
    }

    /// Initializes signers with encrypted key form DATA env var
    pub fn initialize_signers_with_env(&mut self) -> Result<&mut Self> {
        use std::sync::Arc;
        let signers_clone = self.signers.clone();
        self.actor_manager.start(move || {
            let actor = InitializeSignersOneShotBlockingActor::new_from_encrypted_env();
            match actor {
                Ok(a) => Box::new(a.with_signers(signers_clone.clone())),
                Err(e) => panic!("Failed to create InitializeSignersOneShotBlockingActor: {:?}", e),
            }
        })?;
        self.with_signers()?;
        Ok(self)
    }

    /// Starts signer actor
    pub fn with_signers(&mut self) -> Result<&mut Self> {
        if !self.has_signers {
            self.has_signers = true;
            // Removed call to non-existent with_signers method
            let closure = CloneableClosure::new(move || Box::new(TxSignersActor::<LoomDataTypesEthereum>::new()));
            self.actor_manager.start(closure)?;
        }
        Ok(self)
    }

    /// Initializes encoder and start encoder actor
    pub fn with_swap_encoder(&mut self, swap_encoder: E) -> Result<&mut Self> {
        self.mutlicaller_address = Some(swap_encoder.address());
        self.encoder = Some(swap_encoder);
        let bc = self.bc.clone();
        let strategy = self.strategy.clone();
        let closure = CloneableClosure::new(move || Box::new(SwapRouterActor::<DB>::new().on_bc(&bc, &strategy)));
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
        let closure = CloneableClosure::new(move || Box::new(MarketStatePreloadedOneShotActor::<P, E, DB>::new(provider).on_bc(&bc, &state)));
        self.actor_manager.start(closure)?;
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
        use std::sync::Arc;
        let market_state_preloader = Arc::new(market_state_preloader);
        let bc = Arc::new(bc);
        let state = Arc::new(state);
        use std::sync::Arc;
        let market_state_preloader = Arc::new(market_state_preloader);
        let bc = Arc::new(bc);
        Ok(self)
    }

    /// Starts nonce and balance monitor
    pub fn with_nonce_and_balance_monitor(&mut self) -> Result<&mut Self> {
        use std::sync::Arc;
        let provider = Arc::new(self.provider.clone());
        let provider_clone = provider.clone();
        let closure = CloneableClosure::new(move || Box::new(NonceAndBalanceMonitorActor::new(provider_clone.clone())));
        self.actor_manager.start(closure)?;
        Ok(self)
    }

    pub fn with_nonce_and_balance_monitor_only_events(&mut self) -> Result<&mut Self> {
        use std::sync::Arc;
        let provider = Arc::new(self.provider.clone());
        let closure = {
            let provider = provider.clone();
            CloneableClosure::new(move || Box::new(NonceAndBalanceMonitorActor::new(provider.clone()).only_once()))
        };
        self.actor_manager.start(closure)?;
        Ok(self)
    }

    /// Starts block history actor
    pub fn with_block_history(&mut self) -> Result<&mut Self> {
        use std::sync::Arc;
        let provider = Arc::new(self.provider.clone());
        let bc = Arc::new(self.bc.clone());
        let state = Arc::new(self.state.clone());
        let closure = CloneableClosure::new(move || Box::new(BlockHistoryActor::new((*provider).clone()).on_bc(&bc, &state)));
        self.actor_manager.start(closure)?;
        Ok(self)
    }

    /// Starts token price calculator
    pub fn with_price_station(&mut self) -> Result<&mut Self> {
        use std::sync::Arc;
        let provider = Arc::new(self.provider.clone());
        let bc = Arc::new(self.bc.clone());
        let closure = CloneableClosure::new(move || Box::new(PriceActor::new(provider.clone()).on_bc(&bc)));
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
        let provider_clone = provider.clone();
        let bc_clone = bc.clone();
        let config_clone = config.clone();
        let closure = CloneableClosure::new(move || {
            let actor = NodeBlockActor::new((*provider).clone(), config_clone.clone());
            let actor = actor.on_bc(&bc_clone);
            Box::new(actor)
        });
        self.actor_manager.start(closure)?;
        Ok(self)
    }

    /// Starts receiving blocks events through direct Reth DB access
    #[cfg(feature = "db-access")]
    pub fn reth_node_with_blocks(&mut self, db_path: String, config: NodeBlockActorConfig) -> Result<&mut Self> {
        let provider = self.provider.clone();
        let bc = self.bc.clone();
        let closure = CloneableClosure::new(move || Box::new(RethDbAccessBlockActor::new(provider, config, db_path).on_bc(&bc)));
        self.actor_manager.start(closure)?;
        Ok(self)
    }

    /// Starts receiving blocks and mempool events through ExEx GRPC
    pub fn with_exex_events(&mut self) -> Result<&mut Self> {
        self.mempool()?;
        let bc = self.bc.clone();
        let closure = CloneableClosure::new(move || Box::new(NodeExExGrpcActor::new("http://[::1]:10000".to_string()).on_bc(&bc)));
        self.actor_manager.start(closure)?;
        Ok(self)
    }

    /// Starts mempool actor collecting pending txes from all mempools and pulling new tx hashes in mempool_events channel
    pub fn mempool(&mut self) -> Result<&mut Self> {
        if !self.has_mempool {
            self.has_mempool = true;
            let bc = self.bc.clone();
            let closure = CloneableClosure::new(move || Box::new(MempoolActor::new().on_bc(&bc)));
            self.actor_manager.start(closure)?;
        }
        Ok(self)
    }

    /// Starts local node pending tx provider
    pub fn with_local_mempool_events(&mut self) -> Result<&mut Self> {
        self.mempool()?;
        use std::sync::Arc;
        let provider = Arc::new(self.provider.clone());
        let bc = Arc::new(self.bc.clone());
        let closure = {
            let provider = provider.clone();
            let bc = bc.clone();
            CloneableClosure::new(move || Box::new(NodeMempoolActor::new(provider.clone()).on_bc(&bc)))
        };
        self.actor_manager.start(closure)?;
        Ok(self)
    }

    /// Starts remote node pending tx provider
    pub fn with_remote_mempool<PM>(&mut self, provider: PM) -> Result<&mut Self>
    where
        PM: Provider<Ethereum> + Send + Sync + Clone + 'static,
    {
        self.mempool()?;
        use std::sync::Arc;
        let bc = Arc::new(self.bc.clone());
        let provider = Arc::new(provider);
        let closure = {
            let bc = bc.clone();
            let provider = provider.clone();
            CloneableClosure::new(move || Box::new(NodeMempoolActor::new(provider.clone()).on_bc(&bc)))
        };
        self.actor_manager.start(closure)?;
        Ok(self)
    }

    /// Starts flashbots broadcaster
    pub fn with_flashbots_broadcaster(&mut self, allow_broadcast: bool) -> Result<&mut Self> {
        use std::sync::Arc;
        let provider = self.provider.clone();
        let relays = self.relays.clone();
        let flashbots = match relays.is_empty() {
            true => Flashbots::new(provider.clone(), "https://relay.flashbots.net", None).with_default_relays(),
            false => Flashbots::new(provider.clone(), "https://relay.flashbots.net", None).with_relays(relays),
        };

        // Wrap flashbots in Arc without cloning
        let flashbots = Arc::new(flashbots);
        let closure = CloneableClosure::new(move || Box::new(FlashbotsBroadcastActor::new((*flashbots).clone(), allow_broadcast)));
        self.actor_manager.start(closure)?;
        Ok(self)
    }

    /// Start composer : estimator, signer and broadcaster
    pub fn with_composers(&mut self, allow_broadcast: bool) -> Result<&mut Self> {
        self.with_evm_estimator()?.with_signers()?.with_flashbots_broadcaster(allow_broadcast)
    }

    /// Starts EVM estimator actor
    pub fn with_evm_estimator(&mut self) -> Result<&mut Self> {
        let bc = self.bc.clone();
        let strategy = self.strategy.clone();
        let provider = self.provider.clone();
        let encoder = self.encoder.clone().ok_or_else(|| eyre!("Encoder not initialized"))?;

        // Start EvmEstimatorActor
        use std::sync::Arc;
        let provider_arc = Arc::new(provider);
        let encoder_arc = Arc::new(encoder);
        let bc_arc = Arc::new(bc);
        let strategy_arc = Arc::new(strategy);

        let closure = {
            let provider = provider_arc.clone();
            let encoder = encoder_arc.clone();
            let bc = bc_arc.clone();
            let strategy = strategy_arc.clone();
            Box::new(EvmEstimatorActor::new_with_provider((*encoder).clone(), Some((*provider).clone())).on_bc(&(*bc), &(*strategy)))
        };
        self.actor_manager.start_and_wait(closure)?;
        Ok(self)
    }

    /// Starts pool health monitor
    pub fn with_health_monitor_pools(&mut self) -> Result<&mut Self> {
        use std::sync::Arc;
        let bc = Arc::new(self.bc.clone());
        let bc_clone = bc.clone();
        self.actor_manager.start(move || Box::new(PoolHealthMonitorActor::new().on_bc(&bc_clone)))?;
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
        use std::sync::Arc;
        let provider = Arc::new(self.provider.clone());
        let bc = Arc::new(self.bc.clone());
        self.actor_manager.start(move || Box::new(StuffingTxMonitorActor::new(provider.clone()).on_bc(&bc.clone())))?;
        Ok(self)
    }
}
