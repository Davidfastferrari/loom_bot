// The file already exists, so this is a new method addition to BlockchainActors impl

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
    /// Starts pool loader actor
    pub fn with_pool_loader(&mut self, pools_config: PoolsLoadingConfig) -> Result<&mut Self> {
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
            move || Box::new(PoolLoaderActor::new(pool_loaders.clone())) as Box<dyn Actor + Send + Sync>
        };
        self.actor_manager.start(closure)?;
        Ok(self)
    }
}
