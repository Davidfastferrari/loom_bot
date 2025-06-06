use alloy_network::{primitives::HeaderResponse, Ethereum};
use alloy_provider::Provider;
use alloy_rpc_types::{BlockTransactionsKind, Header};
use loom_core_actors::{subscribe, Broadcaster, WorkerResult};
use loom_types_events::{BlockUpdate, Message, MessageBlock};
use tracing::{debug, error};

pub async fn new_block_with_tx_worker<P>(
    client: P,
    block_header_receiver: Broadcaster<Header>,
    sender: Broadcaster<MessageBlock>,
) -> WorkerResult
where
    P: Provider<Ethereum> + Send + Sync + 'static,
{
    use alloy_rpc_types::{BlockTransactionsKind, BlockTransactions};
    subscribe!(block_header_receiver);

    loop {
        if let Ok(block_header) = block_header_receiver.recv().await {
            let (block_number, block_hash) = (block_header.number, block_header.hash);
            debug!("BlockWithTx header received {} {}", block_number, block_hash);

            let mut err_counter = 0;

            while err_counter < 3 {
                // First fetch block with hashes only to reduce message size
                match client.get_block_by_hash(block_header.hash(), BlockTransactionsKind::Hashes).await {
                    Ok(Some(block_with_hashes)) => {
                        // Optionally fetch full block data if needed
                        match client.get_block_by_hash(block_header.hash(), BlockTransactionsKind::Full).await {
                            Ok(Some(mut full_block)) => {
                                // Merge or filter transactions if needed here
                                // For now, just send the full block
                                if let Err(e) = sender.send(Message::new_with_time(BlockUpdate { block: full_block })) {
                                    error!("Broadcaster error {}", e);
                                }
                            }
                            Ok(None) => {
                                error!("Full block data is empty");
                            }
                            Err(e) => {
                                error!("Error fetching full block data: {}", e);
                                err_counter += 1;
                                continue;
                            }
                        }
                        break;
                    }
                    Ok(None) => {
                        error!("Block with hashes is empty");
                        break;
                    }
                    Err(e) => {
                        error!("Error fetching block with hashes: {}", e);
                        err_counter += 1;
                    }
                }
            }

            debug!("BlockWithTx processing finished {} {}", block_number, block_hash);
        }
    }
}
