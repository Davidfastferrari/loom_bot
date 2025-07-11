use alloy_primitives::{Address, U256};
use eyre::{eyre, ErrReport, Result};
use revm::primitives::Env;
use revm::DatabaseRef;
use super::utils::json_log;
use loom_core_actors::subscribe;
use loom_core_actors_macros::{Accessor, Consumer, Producer};
use tokio::sync::broadcast::error::RecvError;

use tracing::{debug, error, info};
use tracing::Level;
use loom_core_actors::{Broadcaster, SharedState, WorkerResult, Actor, ActorResult, Consumer, Producer, Accessor};

use loom_core_blockchain::{Blockchain, Strategy};
use loom_types_entities::{LatestBlock, Swap, SwapStep};
use loom_types_events::{MarketEvents, MessageSwapCompose, SwapComposeData, SwapComposeMessage};

use std::collections::HashMap;

const COINBASE: Address = Address::new([0x1f, 0x90, 0x90, 0xaa, 0xE2, 0x8b, 0x8a, 0x3d, 0xCe, 0xaD, 0xf2, 0x81, 0xB0, 0xF1, 0x28, 0x28, 0xe6, 0x76, 0xc3, 0x26]);

async fn arb_swap_steps_optimizer_task<DB: DatabaseRef + Send + Sync + Clone>(
    compose_channel_tx: Broadcaster<MessageSwapCompose<DB>>,
    state_db: &(dyn DatabaseRef<Error = ErrReport> + Send + Sync + 'static),
    evm_env: Env,
    request: SwapComposeData<DB>,
) -> Result<()> {
    json_log(Level::DEBUG, "Step Simulation started", &[
        ("swap", &format!("{:?}", request.swap)),
    ]);

    if let Swap::BackrunSwapSteps((sp0, sp1)) = request.swap {
        let start_time = chrono::Local::now();
        match SwapStep::optimize_swap_steps(&state_db, evm_env, &sp0, &sp1, None) {
            Ok((s0, s1)) => {
                let encode_request = MessageSwapCompose::prepare(SwapComposeData {
                    origin: Some("merger_searcher".to_string()),
                    tips_pct: None,
                    swap: Swap::BackrunSwapSteps((s0, s1)),
                    ..request
                });
                compose_channel_tx.send(encode_request).map_err(|_| eyre!("CANNOT_SEND"))?;
            }
            Err(e) => {
                json_log(Level::ERROR, "Optimization error", &[("error", &format!("{}", e))]);
                return Err(eyre!("OPTIMIZATION_ERROR"));
            }
        }
        json_log(Level::DEBUG, "Step Optimization finished", &[
            ("sp0", &format!("{:?}", sp0)),
            ("sp1", &format!("{:?}", sp1)),
            ("duration", &format!("{:?}", chrono::Local::now() - start_time)),
        ]);
    } else {
        json_log(Level::ERROR, "Incorrect swap_type", &[]);
        return Err(eyre!("INCORRECT_SWAP_TYPE"));
    }

    Ok(())
}

async fn diff_path_merger_worker<DB: DatabaseRef<Error = ErrReport> + Send + Sync + Clone + 'static>(
    latest_block: SharedState<LatestBlock>,
    market_events_rx: Broadcaster<MarketEvents>,
    compose_channel_rx: Broadcaster<MessageSwapCompose<DB>>,
    compose_channel_tx: Broadcaster<MessageSwapCompose<DB>>,
) -> WorkerResult {
    let mut market_events_rx_receiver = market_events_rx.subscribe();
    let mut compose_channel_rx_receiver = compose_channel_rx.subscribe();
    let mut ready_requests: Vec<SwapComposeData<DB>> = Vec::new();

    loop {
        tokio::select! {
            msg = market_events_rx_receiver.recv() => {
                let msg : Result<MarketEvents, RecvError> = msg;
                match msg {
                    Ok(event) => {
                        match event {
                            MarketEvents::BlockHeaderUpdate{..} =>{
                                json_log(Level::DEBUG, "Cleaning ready requests", &[]);
                                ready_requests = Vec::new();
                            }
                            MarketEvents::BlockStateUpdate{..}=>{
                                json_log(Level::DEBUG, "State updated", &[]);
                            }
                            _=>{}
                        }
                    }
                    Err(e)=>{
                        json_log(Level::ERROR, "Market event error", &[("error", &format!("{:?}", e))]);
                    }
                }

            },
            msg = compose_channel_rx_receiver.recv() => {
                let msg : Result<MessageSwapCompose<DB>, RecvError> = msg;
                match msg {
                    Ok(swap) => {

                        let compose_data = match swap.inner() {
                            SwapComposeMessage::Ready(data) => data,
                            _=>continue,
                        };

                        let swap_path = match &compose_data.swap {
                            Swap::BackrunSwapLine(path) => path,
                            _=>continue,
                        };

                        json_log(Level::INFO, "MessageSwapPathEncodeRequest received", &[
                            ("stuffing_txs_hashes", &format!("{:?}", compose_data.tx_compose.stuffing_txs_hashes)),
                            ("swap", &format!("{:?}", compose_data.swap)),
                        ]);

                        for req in ready_requests.iter() {

                            let req_swap = match &req.swap {
                                Swap::BackrunSwapLine(path)=>path,
                                _ => continue,
                            };

                            if !compose_data.same_stuffing(&req.tx_compose.stuffing_txs_hashes) {
                                continue
                            };

                        match SwapStep::merge_swap_paths( req_swap.clone(), swap_path.clone(), COINBASE ){
                            Ok((sp0, sp1)) => {
                                let latest_block_guard = latest_block.read().await;
                                let block_header = latest_block_guard.block_header.clone().unwrap();
                                drop(latest_block_guard);

                                let request = SwapComposeData{
                                    swap : Swap::BackrunSwapSteps((sp0,sp1)),
                                    ..compose_data.clone()
                                };

                                let mut evm_env = Env::default();
                                evm_env.block.number = U256::from(block_header.number + 1);
                                evm_env.block.timestamp = U256::from(block_header.timestamp + 12);

                                if let Some(db) = compose_data.poststate.clone() {
                                    let db_clone = db.clone();
                                    let compose_channel_clone = compose_channel_tx.clone();
                                    tokio::task::spawn( async move {
                                            arb_swap_steps_optimizer_task(
                                            compose_channel_clone,
                                            &db_clone,
                                            evm_env,
                                            request
                                        ).await
                                    });
                                }
                                break; // only first
                            }
                            Err(e)=>{
                                json_log(Level::ERROR, "SwapPath merge error", &[
                                    ("ready_requests_len", &ready_requests.len().to_string()),
                                    ("error", &format!("{:?}", e)),
                                ]);
                            }
                        }
                        }
                        ready_requests.push(compose_data.clone());
                        ready_requests.sort_by(|r0,r1| r1.swap.abs_profit().cmp(&r0.swap.abs_profit())  )

                    }
                    Err(e)=>{
                        json_log(Level::ERROR, "Compose channel receive error", &[("error", &format!("{:?}", e))]);
                    }
                }
            }
        }
    }
}

#[derive(Consumer, Producer, Accessor)]
pub struct DiffPathMergerActor<DB: Send + Sync + Clone + 'static> {
    #[accessor]
    latest_block: Option<SharedState<LatestBlock>>,
    #[consumer]
    market_events: Option<Broadcaster<MarketEvents>>,
    #[consumer]
    compose_channel_rx: Option<Broadcaster<MessageSwapCompose<DB>>>,

    #[producer]
    compose_channel_tx: Option<Broadcaster<MessageSwapCompose<DB>>>,
}

impl<DB> DiffPathMergerActor<DB>
where
    DB: DatabaseRef + Send + Sync + Clone + 'static,
{
    pub fn new() -> DiffPathMergerActor<DB> {
        DiffPathMergerActor {
            latest_block: None,
            market_events: None,
            compose_channel_rx: None,
            compose_channel_tx: None,
        }
    }
    pub fn on_bc(self, bc: &Blockchain, strategy: &Strategy<DB>) -> Self {
        Self {
            latest_block: Some(bc.latest_block()),
            market_events: Some(bc.market_events_channel()),
            compose_channel_tx: Some(strategy.swap_compose_channel()),
            compose_channel_rx: Some(strategy.swap_compose_channel()),
            ..self
        }
    }
}

impl<DB> Actor for DiffPathMergerActor<DB>
where
    DB: DatabaseRef<Error = ErrReport> + Send + Sync + Clone + 'static,
{
    fn start(&self) -> ActorResult {
        let latest_block = self.latest_block.clone()
            .ok_or_else(|| eyre::eyre!("DiffPathMergerActor: latest_block not set"))?;
        let market_events = self.market_events.clone()
            .ok_or_else(|| eyre::eyre!("DiffPathMergerActor: market_events not set"))?;
        let compose_channel_rx = self.compose_channel_rx.clone()
            .ok_or_else(|| eyre::eyre!("DiffPathMergerActor: compose_channel_rx not set"))?;
        let compose_channel_tx = self.compose_channel_tx.clone()
            .ok_or_else(|| eyre::eyre!("DiffPathMergerActor: compose_channel_tx not set"))?;

        let task = tokio::task::spawn(diff_path_merger_worker(
            latest_block,
            market_events,
            compose_channel_rx,
            compose_channel_tx,
        ));
        Ok(vec![task])
    }

    fn name(&self) -> &'static str {
        "DiffPathMergerActor"
    }
}
//</create_file>