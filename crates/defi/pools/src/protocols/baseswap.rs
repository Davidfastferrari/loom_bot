use alloy_primitives::Address;
use loom_types_entities::PoolClass;

use crate::protocols::protocol::Protocol;

pub struct BaseSwapV2Protocol {}

impl Protocol for BaseSwapV2Protocol {
    fn name(&self) -> &'static str {
        "baseswap_v2"
    }

    fn pool_class(&self) -> PoolClass {
        PoolClass::UniswapV2
    }

    fn factory_address(&self) -> Address {
        // BaseSwap factory address on Base
        "0xFDa619b6d20975be80A10332cD39b9a4b0FAa8BB".parse().unwrap()
    }

    fn init_code_hash(&self) -> [u8; 32] {
        // BaseSwap init code hash
        hex::decode("f4ccce374816856d11f00e4069e7cada164065686fbef53c6167a63ec2fd8c5b")
            .unwrap()
            .try_into()
            .unwrap()
    }

    fn supports_chain_id(&self, chain_id: u64) -> bool {
        // Base chain ID
        chain_id == 8453
    }
}