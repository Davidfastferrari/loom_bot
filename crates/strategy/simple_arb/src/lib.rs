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
    
    // Maximum path length (3-5 hops)
    let max_path_length = 4;
    
    // For each main token, find paths that start and end with it
    for start_token in main_tokens.iter() {
        let start_address = start_token.address();
        
        // Use depth-first search to find all cycles up to max_path_length
        find_cycles(
            &market_guard, 
            start_token.clone(), 
            start_address, 
            vec![start_token.clone()], 
            vec![], 
            HashSet::new(),
            max_path_length,
            &compose_channel_tx
        ).await?;
    }
    
    Ok(())
}

/// DFS to find all cycles with variable length
async fn find_cycles<DB: DatabaseRef<Error = ErrReport> + Send + Sync + Clone + 'static>(
    market: &Market,
    start_token: Arc<Token>,
    current_token_address: Address,
    current_path: Vec<Arc<Token>>,
    current_pools: Vec<Arc<PoolWrapper>>,
    visited_tokens: HashSet<Address>,
    max_depth: usize,
    compose_channel_tx: &Broadcaster<MessageSwapCompose<DB>>,
) -> Result<()> {
    // If we've reached max depth, stop
    if current_path.len() > max_depth {
        return Ok(());
    }
    
    // Get all pools that contain this token
    let pools = market.get_pools_by_token(&current_token_address);
    
    for pool in pools {
        // Skip if we've already used this pool
        if current_pools.contains(&pool) {
            continue;
        }
        
        // Get the other token in the pool
        let token_addresses = pool.get_token_addresses();
        let other_token_address = if token_addresses[0] == current_token_address {
            token_addresses[1]
        } else {
            token_addresses[0]
        };
        
        // If we've found a cycle back to the start token and path length >= 3
        if other_token_address == start_token.address() && current_path.len() >= 3 {
            // Create a complete cycle
            let mut complete_path = current_path.clone();
            complete_path.push(start_token.clone());
            
            let mut complete_pools = current_pools.clone();
            complete_pools.push(pool.clone());
            
            // Create the path
            let path = SwapPath {
                tokens: complete_path,
                pools: complete_pools,
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
                origin: Some("enhanced_arb_finder".to_string()),
                ..Default::default()
            };
            
            let compose_message = MessageSwapCompose::prepare(compose_data);
            if let Err(e) = compose_channel_tx.send(compose_message) {
                error!("Failed to send compose message: {}", e);
            }
        } else if !visited_tokens.contains(&other_token_address) {
            // Continue the search with the new token
            let other_token = match market.get_token(&other_token_address) {
                Some(token) => token,
                None => continue, // Skip if token not found
            };
            
            let mut new_path = current_path.clone();
            new_path.push(other_token.clone());
            
            let mut new_pools = current_pools.clone();
            new_pools.push(pool.clone());
            
            let mut new_visited = visited_tokens.clone();
            new_visited.insert(other_token_address);
            
            find_cycles(
                market,
                start_token.clone(),
                other_token_address,
                new_path,
                new_pools,
                new_visited,
                max_depth,
                compose_channel_tx
            ).await?;
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