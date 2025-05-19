use alloy_primitives::{Address, U256};
use eyre::{eyre, ErrReport, Result};
use revm::DatabaseRef;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::sync::broadcast::error::RecvError;
use tracing::{debug, error, info, trace};

use loom_core_actors::{subscribe, Accessor, Actor, ActorResult, Broadcaster, Consumer, Producer, SharedState, WorkerResult};
use loom_core_actors_macros::{Accessor, Consumer, Producer};
use loom_core_blockchain::{Blockchain, Strategy};
use loom_types_entities::{LatestBlock, Market, PoolWrapper, Swap, SwapDirection, SwapLine, SwapPath, Token};
use loom_types_events::{MarketEvents, MessageSwapCompose, SwapComposeData, SwapComposeMessage};

// Simple arbitrage path finder that looks for cycles of length 3
pub async fn simple_arb_finder_worker<DB: DatabaseRef<Error = ErrReport> + Send + Sync + Clone + 'static>(
    market: SharedState<Market>,
    market_events_rx: Broadcaster<MarketEvents>,
    compose_channel_tx: Broadcaster<MessageSwapCompose<DB>>,
) -> WorkerResult {
    subscribe!(market_events_rx);

    loop {
        tokio::select! {
            msg = market_events_rx.recv() => {
                let msg: Result<MarketEvents, RecvError> = msg;
                match msg {
                    Ok(event) => {
                        match event {
                            MarketEvents::BlockHeaderUpdate{..} => {
                                // Find arbitrage opportunities on new block
                                if let Err(e) = find_arbitrage_paths(market.clone(), compose_channel_tx.clone()).await {
                                    error!("Error finding arbitrage paths: {}", e);
                                }
                            },
                            _ => {}
                        }
                    },
                    Err(e) => {
                        error!("Error receiving market event: {}", e);
                    }
                }
            }
        }
    }
}

async fn find_arbitrage_paths<DB: DatabaseRef<Error = ErrReport> + Send + Sync + Clone + 'static>(
    market: SharedState<Market>,
    compose_channel_tx: Broadcaster<MessageSwapCompose<DB>>,
) -> Result<()> {
    let market_guard = market.read().await;
    
    // Get all tokens
    let tokens: Vec<Arc<Token>> = market_guard.get_tokens();
    
    // Focus on main tokens for efficiency
    let main_tokens: Vec<Arc<Token>> = tokens.into_iter()
        .filter(|t| t.is_basic())
        .collect();
    
    if main_tokens.is_empty() {
        return Err(eyre!("No main tokens found"));
    }
    
    // For each main token, find paths that start and end with it
    for start_token in main_tokens.iter() {
        let start_address = start_token.address();
        
        // Get all pools that contain this token
        let pools = market_guard.get_pools_by_token(&start_address);
        
        for pool in pools {
            // Get the other token in the pool
            let token_addresses = pool.get_token_addresses();
            let other_token = if token_addresses[0] == start_address {
                token_addresses[1]
            } else {
                token_addresses[0]
            };
            
            // Find pools that contain the other token but not the start token
            let second_pools = market_guard.get_pools_by_token(&other_token)
                .into_iter()
                .filter(|p| !p.contains_token(&start_address))
                .collect::<Vec<_>>();
            
            for second_pool in second_pools {
                // Get the third token
                let second_token_addresses = second_pool.get_token_addresses();
                let third_token = if second_token_addresses[0] == other_token {
                    second_token_addresses[1]
                } else {
                    second_token_addresses[0]
                };
                
                // Find pools that connect the third token back to the start token
                let third_pools = market_guard.get_pools_by_token(&third_token)
                    .into_iter()
                    .filter(|p| p.contains_token(&start_address))
                    .collect::<Vec<_>>();
                
                for third_pool in third_pools {
                    // We have a potential cycle: start_token -> other_token -> third_token -> start_token
                    
                    // Create the path
                    let path = SwapPath {
                        tokens: vec![
                            start_token.clone(),
                            market_guard.get_token(&other_token).unwrap(),
                            market_guard.get_token(&third_token).unwrap(),
                            start_token.clone(),
                        ],
                        pools: vec![
                            pool.clone(),
                            second_pool.clone(),
                            third_pool.clone(),
                        ],
                        disabled: false,
                        score: Some(1.0),
                    };
                    
                    // Create a swap line
                    let swap_line = SwapLine {
                        path,
                        ..Default::default()
                    };
                    
                    // Send to the compose channel for further processing
                    let compose_data = SwapComposeData {
                        swap: Swap::BackrunSwapLine(swap_line),
                        origin: Some("simple_arb_finder".to_string()),
                        ..Default::default()
                    };
                    
                    let compose_message = MessageSwapCompose::prepare(compose_data);
                    if let Err(e) = compose_channel_tx.send(compose_message) {
                        error!("Failed to send compose message: {}", e);
                    }
                }
            }
        }
    }
    
    Ok(())
}

#[derive(Accessor, Consumer, Producer)]
pub struct SimpleArbFinderActor<DB: Clone + Send + Sync + 'static> {
    #[accessor]
    market: Option<SharedState<Market>>,
    #[consumer]
    market_events: Option<Broadcaster<MarketEvents>>,
    #[producer]
    compose_channel_tx: Option<Broadcaster<MessageSwapCompose<DB>>>,
}

impl<DB: Clone + Send + Sync + 'static> SimpleArbFinderActor<DB> {
    pub fn new() -> Self {
        Self {
            market: None,
            market_events: None,
            compose_channel_tx: None,
        }
    }
    
    pub fn on_bc(self, bc: &Blockchain, strategy: &Strategy<DB>) -> Self {
        Self {
            market: Some(bc.market()),
            market_events: Some(bc.market_events_channel()),
            compose_channel_tx: Some(strategy.swap_compose_channel()),
            ..self
        }
    }
}

impl<DB> Actor for SimpleArbFinderActor<DB>
where
    DB: DatabaseRef<Error = ErrReport> + Send + Sync + Clone + 'static,
{
    fn start(&self) -> ActorResult {
        let task = tokio::task::spawn(simple_arb_finder_worker(
            self.market.clone().unwrap(),
            self.market_events.clone().unwrap(),
            self.compose_channel_tx.clone().unwrap(),
        ));
        
        Ok(vec![task])
    }
    
    fn name(&self) -> &'static str {
        "SimpleArbFinderActor"
    }
}